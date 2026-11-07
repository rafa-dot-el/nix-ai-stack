//! Loading ~/.config/rafa/slack.yaml (rafa.d drop-ins first) and resolving the credentials.
use std::collections::BTreeMap;

use crate::{client::Client, config::Config, read::Slack};

pub const NOT_CONFIGURED: &str =
    "slack is not configured: add credentials.token (a secret ref, e.g. { command: [pass, show, slack/token] }) to ~/.config/rafa/slack.yaml";

/// The configuration, and a connected Slack or why there is none.
pub struct Setup {
    pub config: Config,
    pub slack: Result<Slack, String>,
    /// argv for the shared voice: rafa-voice say - -o
    pub tts: Vec<String>,
}

fn read_file(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))
}

fn run(argv: &[String]) -> Result<String, String> {
    let out = std::process::Command::new(&argv[0]).args(&argv[1..]).output().map_err(|e| format!("{}: {e}", argv[0]))?;
    if !out.status.success() {
        return Err(format!("{} exited {}: {}", argv[0], out.status, String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn load(env: &BTreeMap<String, String>, base: &str) -> Result<Setup, String> {
    let home = match env.get("XDG_CONFIG_HOME") {
        Some(x) if !x.is_empty() => x.clone(),
        _ => format!("{}/.config", env.get("HOME").map_or("", String::as_str)),
    };
    let dir = format!("{home}/rafa/slack.d");
    let mut dropins: Vec<String> = std::fs::read_dir(&dir)
        .map(|es| es.filter_map(Result::ok).filter_map(|e| e.file_name().to_str().map(str::to_string)).filter(|n| n.ends_with(".yaml")).collect())
        .unwrap_or_default();
    dropins.sort();
    let mut layers = Vec::new();
    for path in rafa_config::layer_paths("slack", &home, &dropins, None) {
        match std::fs::read_to_string(&path) {
            Ok(text) => layers.push(rafa_config::Layer::parse(&path, &text).map_err(|e| e.to_string())?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{path}: {e}")),
        }
    }
    let value = if layers.is_empty() {
        None
    } else {
        let merged = rafa_config::merge(&layers, &[], 1).map_err(|e| e.to_string())?;
        Some(rafa_config::expand(&merged.value, env, "slack configuration").map_err(|e| e.to_string())?)
    };
    let config = crate::config::from_value(value.as_ref()).map_err(|e| e.to_string())?;
    let slack = match &config.token {
        None => Err(NOT_CONFIGURED.to_string()),
        Some(token) => (|| {
            let token = token.resolve("credentials.token", read_file, run).map_err(|e| e.to_string())?;
            let cookie = config.cookie.as_ref().map(|c| c.resolve("credentials.cookie", read_file, run)).transpose().map_err(|e| e.to_string())?;
            Ok(Slack::new(Client::new(token, cookie, base).map_err(|e| e.to_string())?))
        })(),
    };
    // RAFA_VOICE: the shared voice's path (set by the Nix preset); empty means none.
    let tts = match env.get("RAFA_VOICE").map(String::as_str) {
        Some("") => vec![],
        voice => vec![voice.unwrap_or("rafa-voice").to_string(), "say".into(), "-".into(), "-o".into()],
    };
    Ok(Setup { config, slack, tts })
}

/// An RFC 4122 version 4 UUID from 16 random bytes (pure).
pub fn uuid_v4(mut b: [u8; 16]) -> String {
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])
}

/// A fresh client_msg_id (reads /dev/urandom).
pub fn new_msg_id() -> String {
    use std::io::Read;
    let mut b = [0u8; 16];
    if std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b)).is_err() {
        let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        b = (n ^ u128::from(std::process::id())).to_le_bytes();
    }
    uuid_v4(b)
}
