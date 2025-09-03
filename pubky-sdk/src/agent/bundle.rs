use serde::{Deserialize, Serialize};

#[cfg(not(target_arch = "wasm32"))]
use std::{fs, io};

use super::core::PubkyAgent;
use crate::{
    PubkyClient, Result, Session,
    errors::{AuthError, RequestError},
};

/// A portable snapshot of a session-bound identity.
///
/// Contains everything needed to rehydrate a `PubkyAgent` later without
/// re-running sign-in or PubkyAuth. Useful to persist a user session across
/// restarts of your script.
///
/// - Native: includes the per-user session cookie secret.
/// - WASM: cookies are browser-managed; bundle only carries the `Session`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionBundle {
    session: Session,
    #[cfg(not(target_arch = "wasm32"))]
    cookie: String,
}

impl PubkyAgent {
    /// Export this agent’s session into a portable bundle.
    ///
    /// No network calls; cheap clone. Safe to persist (see security note).
    pub fn export(&self) -> SessionBundle {
        SessionBundle {
            session: self.session.clone(),
            #[cfg(not(target_arch = "wasm32"))]
            cookie: self.cookie.clone(),
        }
    }

    /// Rehydrate an agent from a previously saved [SessionBundle].
    ///
    /// Performs a cheap roundtrip to confirm the server
    /// still accepts the session (recommended on app start).
    pub async fn import(client: &PubkyClient, bundle: SessionBundle) -> Result<Self> {
        // Construct synchronously from parts
        let agent = PubkyAgent {
            client: client.clone(),
            session: bundle.session.clone(),
            #[cfg(not(target_arch = "wasm32"))]
            cookie: bundle.cookie.clone(),
        };

        // 200 => still valid; 404/410 => expired; other => transport/server error
        match agent.session_from_homeserver().await? {
            Some(_fresh) => {} // OK
            None => return Err(AuthError::RequestExpired.into()),
        }

        Ok(agent)
    }
}

impl SessionBundle {
    /// Serialize this bundle to pretty JSON.
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self).map_err(|e| {
            RequestError::DecodeJson {
                message: e.to_string(),
            }
            .into()
        })
    }

    /// Parse a bundle from JSON.
    pub fn from_json(s: &str) -> Result<Self> {
        serde_json::from_str(s).map_err(|e| {
            RequestError::DecodeJson {
                message: e.to_string(),
            }
            .into()
        })
    }

    /// Save this bundle to a file as pretty JSON (creates parent dirs if needed).
    pub fn save_file<P: AsRef<std::path::Path>>(&self, path: P) -> std::io::Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = fs::File::create(path)?;
        serde_json::to_writer_pretty(file, self)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    /// Load a bundle from a JSON file.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_file<P: AsRef<std::path::Path>>(path: P) -> Result<Self> {
        use std::{fs, io::Read};
        let mut file = fs::File::open(path).map_err(|e| RequestError::DecodeJson {
            message: e.to_string(),
        })?;
        let mut buf = String::new();
        file.read_to_string(&mut buf)
            .map_err(|e| RequestError::DecodeJson {
                message: e.to_string(),
            })?;
        Self::from_json(&buf)
    }
}
