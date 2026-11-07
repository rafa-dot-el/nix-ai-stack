//! How a CLI command prints its result: human text by default, or --json / --yaml.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::Args)]
pub struct Format {
    /// Print JSON.
    #[arg(long, conflicts_with = "yaml")]
    pub json: bool,
    /// Print YAML.
    #[arg(long)]
    pub yaml: bool,
}

pub fn render<T: serde::Serialize>(value: &T, format: Format, human: impl FnOnce(&T) -> String) -> String {
    if format.json {
        serde_json::to_string_pretty(value).expect("serialisable")
    } else if format.yaml {
        serde_yaml_ng::to_string(value).expect("serialisable")
    } else {
        human(value)
    }
}

/// Replace `path` with `text`: a temporary file in the same directory, synced, renamed;
/// parent directories are created.
pub fn write_atomic(path: &std::path::Path, text: &str) -> Result<(), String> {
    use std::io::Write;
    let err = |e: std::io::Error| format!("{}: {e}", path.display());
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(std::path::Path::new("."));
    std::fs::create_dir_all(dir).map_err(err)?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = dir.join(format!(".{name}.tmp.{}", std::process::id()));
    let result = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result.map_err(err)
}
