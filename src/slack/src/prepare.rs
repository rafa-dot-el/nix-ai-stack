//! From a Spec to a Body and Options (effects: files read within allowed_roots, TTS via the
//! shared rafa-voice, the schedule's time parsed), then the whole send with a dry run.
use crate::{
    config::Config,
    send::{Body, Options},
    spec::Spec,
};

/// `at`: RFC 3339, or `YYYY-MM-DD HH:MM` in `tz` (default UTC) -> Unix seconds (pure).
pub fn post_at(at: &str, tz: Option<&str>) -> Result<i64, String> {
    use chrono::TimeZone;
    if let Ok(t) = chrono::DateTime::parse_from_rfc3339(at.trim()) {
        return Ok(t.timestamp());
    }
    let local = chrono::NaiveDateTime::parse_from_str(at.trim(), "%Y-%m-%d %H:%M")
        .map_err(|_| format!("schedule.at {at:?}: use RFC 3339 or YYYY-MM-DD HH:MM (with tz)"))?;
    let zone: chrono_tz::Tz = match tz {
        None => chrono_tz::UTC,
        Some(name) => name.parse().map_err(|_| format!("unknown timezone {name:?}"))?,
    };
    zone.from_local_datetime(&local).earliest().map(|t| t.timestamp()).ok_or_else(|| format!("{at} does not exist in {zone}"))
}

/// A thread reference: a permalink (-> its ts, or its thread_ts) or a bare ts (pure).
pub fn thread_ts(thread: &str) -> Result<String, String> {
    let t = thread.trim();
    if t.starts_with("https://") {
        let link = crate::target::permalink(t)?;
        return Ok(link.thread_ts.unwrap_or(link.ts));
    }
    let ok = t.split_once('.').is_some_and(|(a, b)| !a.is_empty() && !b.is_empty() && a.chars().chain(b.chars()).all(|c| c.is_ascii_digit()));
    if ok { Ok(t.to_string()) } else { Err(format!("thread {thread:?}: a message link or a ts like 1712345678.123456")) }
}

fn canonical_roots(cfg: &Config) -> Vec<std::path::PathBuf> {
    cfg.allowed_roots.iter().filter_map(|r| std::fs::canonicalize(r).ok()).collect()
}

/// A local file named by the spec: canonical (symlinks and .. resolved), inside the roots,
/// within the size limit.
fn checked(path: &str, cfg: &Config, roots: &[std::path::PathBuf]) -> Result<(std::path::PathBuf, u64), String> {
    let canon = std::fs::canonicalize(path).map_err(|e| format!("{path}: {e}"))?;
    let size = std::fs::metadata(&canon).map_err(|e| format!("{path}: {e}"))?.len();
    crate::send::check_file(&canon, size, roots, cfg.max_bytes)?;
    Ok((canon, size))
}

fn read_text(path: &str, cfg: &Config, roots: &[std::path::PathBuf]) -> Result<String, String> {
    let (canon, _) = checked(path, cfg, roots)?;
    std::fs::read_to_string(&canon).map_err(|e| format!("{path}: {e}"))
}

fn speak(text: &str, tts: &[String]) -> Result<crate::send::File, String> {
    use std::io::Write;
    let (program, args) = tts.split_first().ok_or("no TTS command (rafa-voice) configured")?;
    let dir = std::env::temp_dir().join(format!("slack-say-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)));
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let out = dir.join("voice-note.mp3");
    let mut child = std::process::Command::new(program)
        .args(args)
        .arg(&out)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("{program}: {e}"))?;
    child.stdin.take().expect("piped").write_all(text.as_bytes()).map_err(|e| format!("{program}: {e}"))?;
    let status = child.wait().map_err(|e| format!("{program}: {e}"))?;
    if !status.success() {
        return Err(format!("{program} failed: {status}"));
    }
    let size = std::fs::metadata(&out).map_err(|e| format!("{}: {e}", out.display()))?.len();
    Ok(crate::send::File { path: out.display().to_string(), name: "voice-note.mp3".into(), size, title: Some("Voice note".into()), alt: None, snippet_type: None })
}

/// Read and check everything the spec names. `tts`: argv that writes speech for its stdin
/// text to the path it is given last (`rafa-voice say - -o`).
pub fn prepare(spec: &Spec, cfg: &Config, tts: &[String], client_msg_id: &str) -> Result<(Body, Options), String> {
    crate::spec::validate(spec)?;
    let roots = canonical_roots(cfg);
    let markdown = match (&spec.markdown, &spec.markdown_file) {
        (Some(md), _) => Some(md.clone()),
        (None, Some(path)) => Some(read_text(path, cfg, &roots)?),
        (None, None) => None,
    };
    let tables = spec.tables.iter().map(|t| crate::blocks::parse_csv(&read_text(&t.csv, cfg, &roots)?)).collect::<Result<Vec<_>, _>>()?;
    let mut files = Vec::new();
    for a in &spec.attach {
        let (canon, size) = checked(&a.path, cfg, &roots)?;
        let name = canon.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
        let snippet_type = (a.kind.as_deref() == Some("snippet")).then(|| a.lang.clone().unwrap_or_else(|| "text".into()));
        files.push(crate::send::File { path: canon.display().to_string(), name, size, title: a.title.clone(), alt: a.alt.clone(), snippet_type });
    }
    if let Some(text) = &spec.say {
        if !cfg.tts {
            return Err("say: tts is disabled in slack.yaml (tts.enable)".into());
        }
        files.push(speak(text, tts)?);
    }
    let opts = Options {
        thread_ts: spec.thread.as_deref().map(thread_ts).transpose()?,
        broadcast: spec.broadcast,
        unfurl_links: spec.unfurl.links,
        unfurl_media: spec.unfurl.media,
        post_at: spec.schedule.as_ref().map(|s| post_at(&s.at, s.tz.as_deref())).transpose()?,
        client_msg_id: client_msg_id.to_string(),
    };
    Ok((Body { markdown, text: spec.text.clone(), blocks: spec.blocks.clone(), tables, files }, opts))
}
