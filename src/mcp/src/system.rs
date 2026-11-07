//! The effectful boundary: config files, systemd notify/activation, the pool watcher.
use std::{
    collections::BTreeMap,
    ffi::CString,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::ffi::OsStrExt,
    },
    path::Path,
};

use rafa_config::{expand, layer_paths, merge, Layer};
use tokio::io::unix::AsyncFd;

use crate::config::{from_value, McpConfig};

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("{path}: {source}")]
    Read { path: String, source: std::io::Error },
    #[error(transparent)]
    Config(#[from] rafa_config::ConfigError),
    #[error(transparent)]
    Mcp(#[from] crate::config::McpConfigError),
}

/// `$XDG_CONFIG_HOME`, else `$HOME/.config`.
pub fn config_home(env: &BTreeMap<String, String>) -> String {
    match env.get("XDG_CONFIG_HOME") {
        Some(x) if !x.is_empty() => x.clone(),
        _ => format!("{}/.config", env.get("HOME").map_or("", String::as_str)),
    }
}

fn read_optional(path: &str) -> Result<Option<String>, LoadError> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(LoadError::Read { path: path.to_string(), source }),
    }
}

/// The layers of `mcp.yaml`, read but not merged.
#[derive(Debug)]
pub struct Layers {
    /// `rafa/mcp.d/*.yaml` in name order (Nix-managed, read-only for `mcp config`).
    pub dropins: Vec<Layer>,
    /// The user's own file, where `mcp config` writes; `None` when it does not exist.
    pub user: Option<Layer>,
    pub user_path: std::path::PathBuf,
    pub project: Option<Layer>,
}

impl Layers {
    /// Every present layer in load order.
    pub fn all(&self) -> Vec<Layer> {
        self.dropins.iter().chain(&self.user).chain(&self.project).cloned().collect()
    }
}

pub fn read_layers(env: &BTreeMap<String, String>, project: Option<&Path>) -> Result<Layers, LoadError> {
    let home = config_home(env);
    let dropin_dir = format!("{home}/rafa/mcp.d");
    let mut names: Vec<String> = match std::fs::read_dir(&dropin_dir) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .filter_map(|e| e.file_name().to_str().map(str::to_string))
            .filter(|n| n.ends_with(".yaml"))
            .collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(source) => return Err(LoadError::Read { path: dropin_dir, source }),
    };
    names.sort();
    let read = |path: &str| -> Result<Option<Layer>, LoadError> {
        match read_optional(path)? {
            Some(text) => Ok(Some(Layer::parse(path, &text)?)),
            None => Ok(None),
        }
    };
    let mut dropins = Vec::new();
    for path in layer_paths("mcp", &home, &names, None).iter().take(names.len()) {
        dropins.extend(read(path)?);
    }
    let user_path = format!("{home}/rafa/mcp.yaml");
    let project = match project {
        Some(dir) => read(&format!("{}/.rafa/mcp.yaml", dir.display()))?,
        None => None,
    };
    Ok(Layers { dropins, user: read(&user_path)?, user_path: user_path.into(), project })
}

/// Load drop-ins, the user file and (only when given) the project file; merge; expand; type.
/// Missing files are skipped; unreadable ones are errors.
pub fn load(env: &BTreeMap<String, String>, project: Option<&Path>) -> Result<McpConfig, LoadError> {
    let layers = read_layers(env, project)?.all();
    let merged = merge(&layers, &["servers", "profiles"], 1)?;
    let value = expand(&merged.value, env, "mcp configuration")?;
    Ok(from_value(&value)?)
}

/// Replace `path` with `text`: a 0600 temporary file in the same directory, synced, renamed.
pub fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    use std::{io::Write, os::unix::fs::{DirBuilderExt, OpenOptionsExt}};
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    let tmp = dir.join(format!(".{}.tmp.{}", path.file_name().and_then(|n| n.to_str()).unwrap_or("file"), std::process::id()));
    let result = (|| {
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

pub use mcp_serve::{activated_fd, notify};

/// Wakes on every create/delete/move in `dir` (inotify), so the pool syncs without polling.
pub struct DirWatch {
    fd: AsyncFd<OwnedFd>,
}

impl DirWatch {
    pub fn new(dir: &Path) -> std::io::Result<DirWatch> {
        let path = CString::new(dir.as_os_str().as_bytes())?;
        // SAFETY: plain syscalls; the returned fd is owned from here on.
        let raw = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
        if raw < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        let mask = libc::IN_CREATE | libc::IN_DELETE | libc::IN_MOVED_FROM | libc::IN_MOVED_TO | libc::IN_DELETE_SELF;
        if unsafe { libc::inotify_add_watch(fd.as_raw_fd(), path.as_ptr(), mask) } < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: `fd` is a valid inotify fd owned by the AsyncFd, never replaced or closed elsewhere.
        Ok(DirWatch { fd: unsafe { AsyncFd::register(fd)? } })
    }

    /// Wait for the next batch of changes (all queued events are consumed).
    pub async fn changed(&mut self) -> std::io::Result<()> {
        let mut buf = [0u8; 4096];
        loop {
            let mut guard = self.fd.readable().await?;
            let mut got = false;
            loop {
                // SAFETY: reads into a local buffer of the given length.
                let n = unsafe { libc::read(guard.get_inner().as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
                if n > 0 {
                    got = true;
                    continue;
                }
                let err = std::io::Error::last_os_error();
                if n < 0 && err.kind() == std::io::ErrorKind::WouldBlock {
                    guard.clear_ready();
                    break;
                }
                return Err(err);
            }
            if got {
                return Ok(());
            }
        }
    }
}
