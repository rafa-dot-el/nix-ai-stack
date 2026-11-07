//! What every gworkspace command and tool needs: the config and one shared token cache.
use std::sync::Arc;

use gworkspace_api::Session;
use gworkspace_auth::{Auth, TOKEN_ENDPOINT};

use crate::config::Config;

/// Why a tool cannot run: no `auth:` configured.
pub fn not_configured(bin: &str) -> String {
    format!("{bin} is not configured: add an auth: section (client, refresh_token secret refs) to ~/.config/rafa/gworkspace.yaml, then run `{bin} auth`")
}

#[derive(Clone)]
pub struct Context {
    pub bin: &'static str,
    pub config: Config,
    /// `Ok(None)`: not configured; `Err`: configured, but the HTTP client cannot be built.
    pub auth: Result<Option<Arc<Auth>>, String>,
}

impl Context {
    pub fn new(bin: &'static str, config: Config) -> Context {
        let auth = match config.auth.clone() {
            None => Ok(None),
            Some(a) => Auth::new(a, config.endpoint("token", TOKEN_ENDPOINT)).map(|a| Some(Arc::new(a))).map_err(|e| e.to_string()),
        };
        Context { bin, config, auth }
    }

    /// The token cache, or why there is none.
    pub fn auth(&self) -> Result<&Arc<Auth>, String> {
        match &self.auth {
            Ok(Some(a)) => Ok(a),
            Ok(None) => Err(not_configured(self.bin)),
            Err(e) => Err(e.clone()),
        }
    }

    /// A session for `service` (an `endpoints:` key), with a fresh or cached token.
    pub async fn session(&self, service: &str, default_base: &str) -> Result<Session, String> {
        let token = self.auth()?.access_token().await.map_err(|e| e.to_string())?;
        Session::new(token, self.config.endpoint(service, default_base)).map_err(|e| e.to_string())
    }
}
