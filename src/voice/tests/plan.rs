use std::{collections::BTreeMap, path::{Path, PathBuf}};

use voice::{config, parse, say_plan, transcribe_plan, Command, Step, VoiceConfig, VoiceError};

fn p(args: &str) -> Result<Command, VoiceError> {
    parse(&args.split_whitespace().map(String::from).collect::<Vec<_>>())
}

fn cfg() -> VoiceConfig {
    VoiceConfig { tts_dir: "/v/tts".into(), voice: "en_GB-alba-medium".into(), stt_model: "/v/stt".into(), language: Some("en".into()) }
}

#[test]
fn say_and_transcribe_parse() {
    assert_eq!(p("say hello -o /t/a.mp3").unwrap(), Command::Say { text: "hello".into(), out: "/t/a.mp3".into(), voice: None });
    assert_eq!(
        p("say - -o a.wav --voice en_US-x").unwrap(),
        Command::Say { text: "-".into(), out: "a.wav".into(), voice: Some("en_US-x".into()) }
    );
    assert_eq!(p("transcribe a.ogg --language pl").unwrap(), Command::Transcribe { file: "a.ogg".into(), language: Some("pl".into()) });
    assert_eq!(p("").unwrap(), Command::Help);
    assert_eq!(p("--help").unwrap(), Command::Help);
}

#[test]
fn text_with_spaces_is_one_argument() {
    let c = parse(&["say".into(), "hello there, world".into(), "-o".into(), "x.wav".into()]).unwrap();
    assert!(matches!(c, Command::Say { ref text, .. } if text == "hello there, world"));
}

#[test]
fn usage_errors() {
    for bad in ["say hello", "say -o x.wav", "say a -o", "transcribe", "transcribe a --frob", "sing", "say a -o x.wav extra"] {
        assert!(matches!(p(bad), Err(VoiceError::Usage(_))), "{bad:?}");
    }
}

#[test]
fn config_file_wins_over_the_wrapper_defaults() {
    let env = BTreeMap::from([
        ("RAFA_VOICE_TTS_DIR".to_string(), "/nix/tts".to_string()),
        ("RAFA_VOICE_TTS_VOICE".to_string(), "en_GB-alba-medium".to_string()),
        ("RAFA_VOICE_STT_MODEL".to_string(), "/nix/stt".to_string()),
    ]);
    let c = config(&env, None).unwrap();
    assert_eq!((c.tts_dir.as_path(), c.voice.as_str(), c.stt_model.as_path(), c.language), (Path::new("/nix/tts"), "en_GB-alba-medium", Path::new("/nix/stt"), None));
    let file = rafa_config::Layer::parse("voice.yaml", "version: 1\ntts: { voice: en_US-amy-low, dir: /home/v }\nstt: { language: pl }\n").unwrap();
    let c = config(&env, Some(&file.value)).unwrap();
    assert_eq!((c.tts_dir.as_path(), c.voice.as_str(), c.stt_model.as_path(), c.language.as_deref()), (Path::new("/home/v"), "en_US-amy-low", Path::new("/nix/stt"), Some("pl")));
}

#[test]
fn missing_settings_and_bad_shapes_name_their_key() {
    let e = config(&BTreeMap::new(), None).unwrap_err();
    assert!(e.to_string().contains("tts.dir"), "{e}");
    let file = rafa_config::Layer::parse("v", "tts: { voice: [a] }\n").unwrap();
    let env = BTreeMap::from([("RAFA_VOICE_TTS_DIR".to_string(), "/a".to_string()), ("RAFA_VOICE_STT_MODEL".to_string(), "/b".to_string())]);
    assert!(config(&env, Some(&file.value)).unwrap_err().to_string().contains("tts.voice"));
}

#[test]
fn say_to_wav_is_one_piper_step_with_the_text_on_stdin() {
    let steps = say_plan(&cfg(), "hi", None, Path::new("/o/a.wav"), Path::new("/w")).unwrap();
    assert_eq!(
        steps,
        vec![Step {
            program: "piper".into(),
            args: vec!["--model".into(), "/v/tts/en_GB-alba-medium.onnx".into(), "--output-file".into(), "/o/a.wav".into()],
            stdin: Some("hi".into()),
        }]
    );
}

#[test]
fn say_to_mp3_encodes_with_ffmpeg_and_a_voice_override_is_used() {
    let steps = say_plan(&cfg(), "hi", Some("en_US-x"), Path::new("/o/a.mp3"), Path::new("/w")).unwrap();
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0].args[1], "/v/tts/en_US-x.onnx");
    assert_eq!(steps[0].args[3], "/w/speech.wav");
    assert_eq!(steps[1].program, "ffmpeg");
    assert_eq!(steps[1].args, vec!["-hide_banner", "-loglevel", "error", "-y", "-i", "/w/speech.wav", "/o/a.mp3"]);
}

#[test]
fn unknown_formats_and_unsafe_voice_names_are_refused() {
    assert!(matches!(say_plan(&cfg(), "hi", None, Path::new("/o/a.exe"), Path::new("/w")), Err(VoiceError::Usage(_))));
    assert!(matches!(say_plan(&cfg(), "hi", Some("../../etc/x"), Path::new("/o/a.wav"), Path::new("/w")), Err(VoiceError::Usage(_))));
    assert!(matches!(say_plan(&cfg(), "", None, Path::new("/o/a.wav"), Path::new("/w")), Err(VoiceError::Usage(_))));
}

#[test]
fn transcribe_runs_faster_whisper_offline_on_the_pinned_model() {
    let (step, txt) = transcribe_plan(&cfg(), Path::new("/in/note.ogg"), None, Path::new("/w"));
    assert_eq!(step.program, "rafa-voice-stt");
    assert_eq!(step.args, vec!["--model", "/v/stt", "--output", "/w/note.txt", "--language", "en", "/in/note.ogg"]);
    assert_eq!(txt, PathBuf::from("/w/note.txt"));
    let (step, _) = transcribe_plan(&VoiceConfig { language: None, ..cfg() }, Path::new("/in/a.wav"), Some("pl"), Path::new("/w"));
    assert!(step.args.windows(2).any(|w| w == ["--language", "pl"]));
    let (step, _) = transcribe_plan(&VoiceConfig { language: None, ..cfg() }, Path::new("/in/a.wav"), None, Path::new("/w"));
    assert!(!step.args.contains(&"--language".to_string()), "no language: detected");
}
