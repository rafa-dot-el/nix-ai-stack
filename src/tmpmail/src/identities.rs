//! The saved inboxes. Each holds a GuerrillaMail `sid_token`, a session credential: the file
//! lives in $XDG_STATE_HOME/rafa/tmpmail/ and is written 0600, atomically.
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Identity {
    /// The assigned address; also the identity's name.
    pub email: String,
    pub sid_token: String,
    /// Unix seconds, when it was created here.
    pub created_at: u64,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Identities {
    #[serde(default)]
    pub identities: Vec<Identity>,
}

impl Identities {
    /// Add, or replace the one with the same address.
    pub fn upsert(&mut self, identity: Identity) {
        match self.identities.iter_mut().find(|i| i.email == identity.email) {
            Some(existing) => *existing = identity,
            None => self.identities.push(identity),
        }
    }

    /// Remove by address; whether one was removed.
    pub fn remove(&mut self, email: &str) -> bool {
        let before = self.identities.len();
        self.identities.retain(|i| i.email != email);
        self.identities.len() != before
    }

    /// The named identity, or the only one when no name is given.
    pub fn resolve(&self, email: Option<&str>) -> Result<&Identity, String> {
        match email {
            Some(addr) => self.identities.iter().find(|i| i.email == addr).ok_or_else(|| format!("no saved identity {addr} (see `tmpmail list-identities`)")),
            None => match self.identities.as_slice() {
                [] => Err("no identities yet: run `tmpmail new-identity` first".into()),
                [only] => Ok(only),
                many => Err(format!("{} identities saved: name one with --email (see `tmpmail list-identities`)", many.len())),
            },
        }
    }
}

/// Parse the file's text (empty means none) (pure).
pub fn parse(text: &str, origin: &str) -> Result<Identities, String> {
    if text.trim().is_empty() {
        return Ok(Identities::default());
    }
    serde_yaml_ng::from_str(text).map_err(|e| format!("{origin}: {e}"))
}

/// Read the file; missing means none.
pub fn load(path: &Path) -> Result<Identities, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text, &path.display().to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Identities::default()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Write the file 0600 through a temporary file and a rename; the directory is created 0700.
pub fn save(path: &Path, ids: &Identities) -> Result<(), String> {
    use std::{io::Write, os::unix::fs::{DirBuilderExt, OpenOptionsExt}};
    let err = |p: &Path, e: std::io::Error| format!("{}: {e}", p.display());
    let dir = path.parent().ok_or_else(|| format!("{}: no directory", path.display()))?;
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir).map_err(|e| err(dir, e))?;
    let text = serde_yaml_ng::to_string(ids).map_err(|e| format!("{}: {e}", path.display()))?;
    let tmp = dir.join(format!(".identities.yaml.{}", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let written = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp).and_then(|mut f| {
        f.write_all(text.as_bytes())?;
        f.sync_all()
    });
    if let Err(e) = written.and_then(|()| std::fs::rename(&tmp, path)) {
        let _ = std::fs::remove_file(&tmp);
        return Err(err(path, e));
    }
    Ok(())
}
