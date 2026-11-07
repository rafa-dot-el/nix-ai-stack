//! `rafa-voice`: one TTS/STT for the whole stack (plan 5.5). Pure: argv and config in, the
//! commands to run out; [`run`] is the thin exec layer.
use std::{collections::BTreeMap, path::PathBuf};

use rafa_config::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceConfig {
    /// Directory holding `<voice>.onnx` and `<voice>.onnx.json`.
    pub tts_dir: PathBuf,
    pub voice: String,
    /// A faster-whisper (CTranslate2) model directory.
    pub stt_model: PathBuf,
    pub language: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Text from the argument, or stdin when it is `-`.
    Say { text: String, out: PathBuf, voice: Option<String> },
    Transcribe { file: PathBuf, language: Option<String> },
    Help,
}

/// One process to run: program, arguments, optional stdin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub program: String,
    pub args: Vec<String>,
    pub stdin: Option<String>,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum VoiceError {
    #[error("{0}\n\n{USAGE}")]
    Usage(String),
    #[error("{path}: {reason}")]
    Config { path: String, reason: String },
    #[error(transparent)]
    Layer(#[from] rafa_config::ConfigError),
}

pub const USAGE: &str = "usage:
  rafa-voice say TEXT|- -o OUT.(wav|mp3|ogg|opus|flac) [--voice NAME]
  rafa-voice transcribe FILE [--language CODE]";

fn usage(msg: impl Into<String>) -> VoiceError {
    VoiceError::Usage(msg.into())
}

pub fn parse(argv: &[String]) -> Result<Command, VoiceError> {
    let Some((sub, rest)) = argv.split_first() else { return Ok(Command::Help) };
    let mut args = rest.iter();
    let value = |flag: &str, args: &mut std::slice::Iter<'_, String>| {
        args.next().cloned().ok_or_else(|| usage(format!("{flag} needs a value")))
    };
    match sub.as_str() {
        "--help" | "-h" | "help" => Ok(Command::Help),
        "say" => {
            let (mut text, mut out, mut voice) = (None, None, None);
            while let Some(a) = args.next() {
                match a.as_str() {
                    "-o" | "--output" => out = Some(PathBuf::from(value(a, &mut args)?)),
                    "--voice" => voice = Some(value(a, &mut args)?),
                    t if text.is_none() && (t == "-" || !t.starts_with('-')) => text = Some(t.to_string()),
                    other => return Err(usage(format!("unexpected argument {other:?} for `say`"))),
                }
            }
            match (text, out) {
                (Some(text), Some(out)) => Ok(Command::Say { text, out, voice }),
                (None, _) => Err(usage("`say` needs TEXT (or - for stdin)")),
                (_, None) => Err(usage("`say` needs -o OUT")),
            }
        }
        "transcribe" => {
            let (mut file, mut language) = (None, None);
            while let Some(a) = args.next() {
                match a.as_str() {
                    "--language" => language = Some(value(a, &mut args)?),
                    f if file.is_none() && !f.starts_with('-') => file = Some(PathBuf::from(f)),
                    other => return Err(usage(format!("unexpected argument {other:?} for `transcribe`"))),
                }
            }
            let file = file.ok_or_else(|| usage("`transcribe` needs FILE"))?;
            Ok(Command::Transcribe { file, language })
        }
        other => Err(usage(format!("unknown command {other:?}"))),
    }
}

fn setting(file: Option<&Value>, section: &str, key: &str) -> Result<Option<String>, VoiceError> {
    match file.and_then(|v| v.get(section)).and_then(|s| s.get(key)) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(VoiceError::Config { path: format!("{section}.{key}"), reason: "expected a string".into() }),
    }
}

/// `env`: RAFA_VOICE_TTS_DIR, RAFA_VOICE_TTS_VOICE, RAFA_VOICE_STT_MODEL (the wrapper's
/// Nix-pinned defaults); `file`: ~/.config/rafa/voice.yaml (tts.dir, tts.voice, stt.model,
/// stt.language), which wins.
pub fn config(env: &BTreeMap<String, String>, file: Option<&Value>) -> Result<VoiceConfig, VoiceError> {
    let pick = |section: &str, key: &str, var: &str| -> Result<String, VoiceError> {
        setting(file, section, key)?.or_else(|| env.get(var).cloned()).ok_or_else(|| VoiceError::Config {
            path: format!("{section}.{key}"),
            reason: format!("not set in voice.yaml and {var} is unset"),
        })
    };
    Ok(VoiceConfig {
        tts_dir: pick("tts", "dir", "RAFA_VOICE_TTS_DIR")?.into(),
        voice: setting(file, "tts", "voice")?.or_else(|| env.get("RAFA_VOICE_TTS_VOICE").cloned()).unwrap_or_else(|| "en_GB-alba-medium".into()),
        stt_model: pick("stt", "model", "RAFA_VOICE_STT_MODEL")?.into(),
        language: setting(file, "stt", "language")?,
    })
}

const FORMATS: [&str; 5] = ["wav", "mp3", "ogg", "opus", "flac"];

fn s(p: &std::path::Path) -> String {
    p.display().to_string()
}

/// Piper to WAV; when `out` is not .wav, a second step encodes it with ffmpeg.
pub fn say_plan(cfg: &VoiceConfig, text: &str, voice: Option<&str>, out: &std::path::Path, work: &std::path::Path) -> Result<Vec<Step>, VoiceError> {
    if text.trim().is_empty() {
        return Err(usage("nothing to say"));
    }
    let voice = voice.unwrap_or(&cfg.voice);
    if voice.is_empty() || !voice.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)) || voice.contains("..") {
        return Err(usage(format!("{voice:?} is not a voice name (letters, digits, - _ .)")));
    }
    let ext = out.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    if !FORMATS.contains(&ext.as_str()) {
        return Err(usage(format!("cannot write {:?}; use one of {}", out.display().to_string(), FORMATS.join(", "))));
    }
    let wav = if ext == "wav" { out.to_path_buf() } else { work.join("speech.wav") };
    let model = cfg.tts_dir.join(format!("{voice}.onnx"));
    let mut steps = vec![Step {
        program: "piper".into(),
        args: vec!["--model".into(), s(&model), "--output-file".into(), s(&wav)],
        stdin: Some(text.to_string()),
    }];
    if ext != "wav" {
        steps.push(Step {
            program: "ffmpeg".into(),
            args: ["-hide_banner", "-loglevel", "error", "-y", "-i"].iter().map(|a| a.to_string()).chain([s(&wav), s(out)]).collect(),
            stdin: None,
        });
    }
    Ok(steps)
}

/// `rafa-voice-stt` (faster-whisper, CPU, int8) writing `<work>/<stem>.txt`; the caller
/// prints that file. Without a language, faster-whisper detects it.
pub fn transcribe_plan(cfg: &VoiceConfig, file: &std::path::Path, language: Option<&str>, work: &std::path::Path) -> (Step, PathBuf) {
    let stem = file.file_stem().and_then(|x| x.to_str()).unwrap_or("audio");
    let txt = work.join(format!("{stem}.txt"));
    let mut args = vec!["--model".to_string(), s(&cfg.stt_model), "--output".to_string(), s(&txt)];
    if let Some(lang) = language.or(cfg.language.as_deref()) {
        args.extend(["--language".to_string(), lang.to_string()]);
    }
    args.push(s(file));
    (Step { program: "rafa-voice-stt".into(), args, stdin: None }, txt)
}

/// Run the steps in order; a failing step stops the rest.
pub fn run(steps: &[Step]) -> Result<(), String> {
    use std::{io::Write, process::Stdio};
    for step in steps {
        let mut child = std::process::Command::new(&step.program)
            .args(&step.args)
            .stdin(if step.stdin.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::null())
            .spawn()
            .map_err(|e| format!("{}: {e}", step.program))?;
        if let (Some(text), Some(mut stdin)) = (&step.stdin, child.stdin.take()) {
            // A step that exits without reading its input closes the pipe: its exit status,
            // checked below, is the error to report, not the broken pipe.
            match stdin.write_all(text.as_bytes()) {
                Err(e) if e.kind() != std::io::ErrorKind::BrokenPipe => return Err(format!("{}: {e}", step.program)),
                _ => {}
            }
        }
        let status = child.wait().map_err(|e| format!("{}: {e}", step.program))?;
        if !status.success() {
            return Err(format!("{} failed: {status}", step.program));
        }
    }
    Ok(())
}
