//! The built rafa-voice with stand-in tools on PATH that record their argv and stdin.
use std::{os::unix::fs::PermissionsExt, path::Path, process::Command};

const BIN: &str = env!("CARGO_BIN_EXE_rafa-voice");

fn fake(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\necho \"$*\" >> {}/{name}.argv\n{body}\n", dir.display())).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

struct Rig {
    dir: tempfile::TempDir,
}

impl Rig {
    fn new() -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        // piper: stdin is the text; --output-file is $4
        fake(d, "piper", &format!("cat > {}/piper.stdin; printf RIFF > \"$4\"", d.display()));
        fake(d, "ffmpeg", "eval last=\\${$#}; printf ID3 > \"$last\"");
        fake(d, "rafa-voice-stt", "echo '  hello from the fake model  ' > \"$4\"");
        Rig { dir }
    }
    fn run(&self, args: &[&str], stdin: Option<&str>) -> std::process::Output {
        let d = self.dir.path();
        let mut c = Command::new(BIN);
        c.args(args)
            .env_clear()
            .env("PATH", format!("{}:/bin:/usr/bin:{}", d.display(), std::env::var("PATH").unwrap_or_default()))
            .env("HOME", d)
            .env("TMPDIR", d)
            .env("RAFA_VOICE_TTS_DIR", "/pinned/tts")
            .env("RAFA_VOICE_STT_MODEL", "/pinned/stt");
        if let Ok(cov) = std::env::var("LLVM_PROFILE_FILE") {
            c.env("LLVM_PROFILE_FILE", cov);
        }
        if let Some(text) = stdin {
            use std::io::Write;
            let mut child = c.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
            child.stdin.take().unwrap().write_all(text.as_bytes()).unwrap();
            return child.wait_with_output().unwrap();
        }
        c.output().unwrap()
    }
    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.dir.path().join(name)).unwrap_or_default()
    }
}

#[test]
fn say_to_mp3_runs_piper_then_ffmpeg_and_cleans_up() {
    let r = Rig::new();
    let out = r.dir.path().join("a.mp3");
    let o = r.run(&["say", "hello there", "-o", out.to_str().unwrap()], None);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "ID3");
    assert_eq!(r.read("piper.stdin"), "hello there");
    assert!(r.read("piper.argv").starts_with("--model /pinned/tts/en_GB-alba-medium.onnx --output-file "));
    assert!(r.read("ffmpeg.argv").ends_with(&format!("{}\n", out.display())));
    let leftovers: Vec<_> = std::fs::read_dir(r.dir.path()).unwrap().filter_map(Result::ok).filter(|e| e.path().is_dir() && e.file_name().to_string_lossy().starts_with("rafa-voice-")).collect();
    assert!(leftovers.is_empty(), "scratch directory left behind");
    assert!(String::from_utf8_lossy(&o.stderr).contains("[+]"));
}

#[test]
fn say_from_stdin_and_voice_yaml_overrides() {
    let r = Rig::new();
    std::fs::create_dir_all(r.dir.path().join(".config/rafa")).unwrap();
    std::fs::write(r.dir.path().join(".config/rafa/voice.yaml"), "version: 1\ntts: { voice: en_US-amy-low, dir: /home/tts }\n").unwrap();
    let out = r.dir.path().join("b.wav");
    let o = r.run(&["say", "-", "-o", out.to_str().unwrap()], Some("from stdin"));
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(r.read("piper.stdin"), "from stdin");
    assert!(r.read("piper.argv").starts_with("--model /home/tts/en_US-amy-low.onnx"), "{}", r.read("piper.argv"));
    assert!(r.read("ffmpeg.argv").is_empty(), "a WAV needs no encoding");
}

#[test]
fn transcribe_prints_the_text() {
    let r = Rig::new();
    let o = r.run(&["transcribe", "/in/note.ogg", "--language", "pl"], None);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(String::from_utf8_lossy(&o.stdout), "hello from the fake model\n");
    assert!(r.read("rafa-voice-stt.argv").starts_with("--model /pinned/stt --output "));
}

#[test]
fn failures_exit_nonzero_with_the_reason() {
    let r = Rig::new();
    fake(r.dir.path(), "piper", "exit 3");
    let o = r.run(&["say", "x", "-o", "/tmp/never.wav"], None);
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("piper failed"), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(r.run(&["sing"], None).status.code(), Some(2));
    assert!(String::from_utf8_lossy(&r.run(&["--help"], None).stdout).contains("rafa-voice say"));
    std::fs::create_dir_all(r.dir.path().join(".config/rafa")).unwrap();
    std::fs::write(r.dir.path().join(".config/rafa/voice.yaml"), "tts: [broken\n").unwrap();
    assert_eq!(r.run(&["say", "x", "-o", "/tmp/a.wav"], None).status.code(), Some(1));
}
