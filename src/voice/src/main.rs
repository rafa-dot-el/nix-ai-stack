use std::{collections::BTreeMap, io::Read, process::ExitCode};

use voice::{config, parse, run, say_plan, transcribe_plan, Command, USAGE};

fn voice_yaml(env: &BTreeMap<String, String>) -> Result<Option<rafa_config::Value>, String> {
    let home = match env.get("XDG_CONFIG_HOME") {
        Some(x) if !x.is_empty() => x.clone(),
        _ => format!("{}/.config", env.get("HOME").map_or("", String::as_str)),
    };
    let path = format!("{home}/rafa/voice.yaml");
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(Some(rafa_config::Layer::parse(&path, &text).map_err(|e| e.to_string())?.value)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{path}: {e}")),
    }
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let command = match parse(&argv) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[-] rafa-voice: {e}");
            return ExitCode::from(2);
        }
    };
    let env: BTreeMap<String, String> = std::env::vars().collect();
    let result = (|| -> Result<(), String> {
        if command == Command::Help {
            println!("{USAGE}");
            return Ok(());
        }
        let cfg = config(&env, voice_yaml(&env)?.as_ref()).map_err(|e| e.to_string())?;
        let work = tempfile_dir()?;
        let outcome = match &command {
            Command::Say { text, out, voice } => {
                let text = if text == "-" {
                    let mut s = String::new();
                    std::io::stdin().read_to_string(&mut s).map_err(|e| e.to_string())?;
                    s
                } else {
                    text.clone()
                };
                let steps = say_plan(&cfg, &text, voice.as_deref(), out, &work).map_err(|e| e.to_string())?;
                run(&steps).map(|()| eprintln!("[+] rafa-voice: wrote {}", out.display()))
            }
            Command::Transcribe { file, language } => {
                let (step, txt) = transcribe_plan(&cfg, file, language.as_deref(), &work);
                run(&[step]).and_then(|()| {
                    let text = std::fs::read_to_string(&txt).map_err(|e| format!("{}: {e}", txt.display()))?;
                    println!("{}", text.trim());
                    Ok(())
                })
            }
            Command::Help => unreachable!(),
        };
        let _ = std::fs::remove_dir_all(&work);
        outcome
    })();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("[-] rafa-voice: {e}");
            ExitCode::FAILURE
        }
    }
}

/// A private scratch directory under $TMPDIR (mode 0700).
fn tempfile_dir() -> Result<std::path::PathBuf, String> {
    use std::os::unix::fs::DirBuilderExt;
    let base = std::env::temp_dir();
    for n in 0..100u32 {
        let dir = base.join(format!("rafa-voice-{}-{n}", std::process::id()));
        match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("{}: {e}", dir.display())),
        }
    }
    Err("no scratch directory".into())
}
