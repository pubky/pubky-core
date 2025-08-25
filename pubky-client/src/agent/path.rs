//! Typed addressing for files on a Pubky homeserver.
//
// Accepted inputs for `PubkyPath::parse`:
// 1) `<user_pubkey>/<path>`            (preferred)
// 2) `/absolute/or/relative/path`      (agent-scoped: no user yet, own user will be used)
// 3) `pubky://<user_pubkey>/<path>`    (legacy)
//
// We intentionally do NOT accept `https://_pubky.<pk>/...` here.

use std::{fmt, str::FromStr};

use pkarr::PublicKey;

use crate::{Error, errors::RequestError};

#[inline]
fn invalid(msg: impl Into<String>) -> Error {
    RequestError::Validation {
        message: msg.into(),
    }
    .into()
}

/// Absolute homeserver path (always starts with `/`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FilePath(String);

impl FilePath {
    /// Parse and normalize to an absolute path.
    pub fn parse<S: AsRef<str>>(s: S) -> Result<Self, Error> {
        let raw = s.as_ref();
        if raw.is_empty() {
            return Err(invalid("path cannot be empty"));
        }

        // Normalize: prepend `/` if missing.
        let out = if raw.starts_with('/') {
            raw.to_string()
        } else {
            format!("/{}", raw)
        };

        // Cheap sanity checks.
        if !out.starts_with('/') {
            return Err(invalid("path must start with '/'"));
        }
        if out.contains("//") {
            return Err(invalid("path contains '//'"));
        }

        Ok(FilePath(out))
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[inline]
    pub fn into_string(self) -> String {
        self.0
    }
}

impl FromStr for FilePath {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl fmt::Display for FilePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A parsed homeserver address.
/// - `user: Some(..)` when the input was `pubky://<user>/...` or `<user>/...`
/// - `user: None`    when the input was an agent-scoped path (e.g. `/foo/bar` or `foo/bar`)
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PubkyPath {
    pub user: Option<PublicKey>,
    pub path: FilePath,
}

impl PubkyPath {
    /// Construct from optional `PublicKey` and any string-y path.
    pub fn new<S: AsRef<str>>(user: Option<PublicKey>, path: S) -> Result<Self, Error> {
        Ok(Self {
            user,
            path: FilePath::parse(path)?,
        })
    }

    /// Parse all accepted forms:
    /// - `pubky://<user>/<path>`
    /// - `<user>/<path>`
    /// - `/absolute/path`               (agent-scoped; requires leading '/')
    pub fn parse(s: &str) -> Result<Self, Error> {
        // 1) Legacy scheme: pubky://<user>/<path>
        if let Some(rest) = s.strip_prefix("pubky://") {
            let (user_str, raw_path) = rest
                .split_once('/')
                .ok_or_else(|| invalid("missing `<user>/<path>`"))?;

            let user = PublicKey::try_from(user_str)
                .map_err(|_| invalid(format!("invalid user public key: {user_str}")))?;
            let path = FilePath::parse(raw_path)?;
            return Ok(PubkyPath {
                user: Some(user),
                path,
            });
        }

        // 2) `<user>/<path>`?
        if let Some((first, rest)) = s.split_once('/') {
            if let Ok(user) = PublicKey::try_from(first) {
                let path = FilePath::parse(rest)?;
                return Ok(PubkyPath {
                    user: Some(user),
                    path,
                });
            } else {
                // If it *looks* like `<something>/<path>` but the "something" is not a pubkey,
                // and there's no leading '/', reject it (agent-scoped requires '/').
                if !s.starts_with('/') {
                    return Err(invalid(
                        "expected `<user>/<path>` (with a valid public key) or `/absolute/path`",
                    ));
                }
            }
        }

        // 3) Agent-scoped path: must start with '/'
        if s.starts_with('/') {
            let path = FilePath::parse(s)?;
            return Ok(PubkyPath { user: None, path });
        }

        // Otherwise, reject (no leading '/' and not `<user>/<path>`).
        Err(invalid(
            "expected `pubky://<user>/<path>`, `<user>/<path>`, or `/absolute/path`",
        ))
    }

    /// Resolve (if needed) with a default user, returning a fully-qualified address.
    pub fn with_default_user(&self, default: &PublicKey) -> ResolvedPubkyPath {
        ResolvedPubkyPath {
            user: self.user.clone().unwrap_or_else(|| default.clone()),
            path: self.path.clone(),
        }
    }

    /// `pubky://<user>/<path>` — requires a user; provide `default` to fill if missing.
    pub fn to_pubky_url(&self, default: Option<&PublicKey>) -> Result<String, Error> {
        let user = match (&self.user, default) {
            (Some(u), _) => u,
            (None, Some(d)) => d,
            (None, None) => return Err(invalid("missing user for pubky URL rendering")),
        };
        let rel = self.path.as_str().trim_start_matches('/');
        Ok(format!("pubky://{}/{}", user, rel))
    }
}

/// A fully-qualified address (always has a user).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ResolvedPubkyPath {
    pub user: PublicKey,
    pub path: FilePath,
}

impl fmt::Display for PubkyPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.user {
            Some(u) => {
                let rel = self.path.as_str().trim_start_matches('/');
                write!(f, "{}/{}", u, rel)
            }
            None => write!(f, "{}", self.path.as_str()),
        }
    }
}

// --- Conversions ---
impl TryFrom<&str> for PubkyPath {
    type Error = Error;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        PubkyPath::parse(s)
    }
}
impl TryFrom<String> for PubkyPath {
    type Error = Error;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        PubkyPath::parse(&s)
    }
}

impl TryFrom<&PubkyPath> for PubkyPath {
    type Error = Error;
    fn try_from(p: &PubkyPath) -> Result<Self, Self::Error> {
        Ok(p.clone())
    }
}

pub trait IntoPubkyPath {
    fn into_pubky_path(self) -> Result<PubkyPath, Error>;
}

// infallible for owned/borrowed PubkyPath
impl IntoPubkyPath for PubkyPath {
    #[inline]
    fn into_pubky_path(self) -> Result<PubkyPath, Error> {
        Ok(self)
    }
}
impl IntoPubkyPath for &PubkyPath {
    #[inline]
    fn into_pubky_path(self) -> Result<PubkyPath, Error> {
        Ok(self.clone())
    }
}

// fallible parsers
impl IntoPubkyPath for &str {
    fn into_pubky_path(self) -> Result<PubkyPath, Error> {
        PubkyPath::parse(self)
    }
}
impl IntoPubkyPath for String {
    fn into_pubky_path(self) -> Result<PubkyPath, Error> {
        PubkyPath::parse(&self)
    }
}

// For convenience: convert all sort of tuples. Flexible dev experience.

// PublicKey + &str
impl IntoPubkyPath for (PublicKey, &str) {
    fn into_pubky_path(self) -> Result<PubkyPath, Error> {
        PubkyPath::new(Some(self.0), self.1)
    }
}

// &PublicKey + &str
impl IntoPubkyPath for (&PublicKey, &str) {
    fn into_pubky_path(self) -> Result<PubkyPath, Error> {
        PubkyPath::new(Some(self.0.clone()), self.1)
    }
}

// PublicKey + String
impl IntoPubkyPath for (PublicKey, String) {
    fn into_pubky_path(self) -> Result<PubkyPath, Error> {
        PubkyPath::new(Some(self.0), self.1)
    }
}

// &PublicKey + String
impl IntoPubkyPath for (&PublicKey, String) {
    fn into_pubky_path(self) -> Result<PubkyPath, Error> {
        PubkyPath::new(Some(self.0.clone()), self.1)
    }
}

// (&str, &str) where the first &str is a public key string
impl IntoPubkyPath for (&str, &str) {
    fn into_pubky_path(self) -> Result<PubkyPath, Error> {
        let user = PublicKey::try_from(self.0).map_err(|_| RequestError::Validation {
            message: format!("invalid user public key: {}", self.0),
        })?;
        PubkyPath::new(Some(user), self.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkarr::Keypair;

    #[test]
    fn file_path_normalization_and_rejections() {
        // Normalize relative
        assert_eq!(FilePath::parse("pub/app").unwrap().as_str(), "/pub/app");
        // Keep absolute
        assert_eq!(FilePath::parse("/pub/app").unwrap().as_str(), "/pub/app");
        // Reject empty
        assert!(matches!(
            FilePath::parse(""),
            Err(Error::Request(RequestError::Validation { .. }))
        ));
        // Reject double-slash
        assert!(matches!(
            FilePath::parse("/pub//app"),
            Err(Error::Request(RequestError::Validation { .. }))
        ));
    }

    #[test]
    fn parse_explicit_user_both_forms() {
        let kp = Keypair::random();
        let user = kp.public_key();
        let s1 = format!("pubky://{}/pub/app/file", user);
        let s2 = format!("{}/pub/app/file", user);

        let p1 = PubkyPath::parse(&s1).unwrap();
        let p2 = PubkyPath::parse(&s2).unwrap();

        assert_eq!(p1.user, Some(user.clone()));
        assert_eq!(p2.user, Some(user.clone()));
        assert_eq!(p1.path.as_str(), "/pub/app/file");
        assert_eq!(p2.path.as_str(), "/pub/app/file");

        // Display: explicit user form
        assert_eq!(p1.to_string(), s2);
        // URL rendering without default is fine when user exists
        assert_eq!(p1.to_pubky_url(None).unwrap(), s1);
    }

    #[test]
    fn parse_agent_scoped_paths() {
        // Absolute, agent-scoped path is OK
        let p_abs = PubkyPath::parse("/pub/app/file").unwrap();
        assert!(p_abs.user.is_none());
        assert_eq!(p_abs.path.as_str(), "/pub/app/file");

        // Relative agent-scoped (no leading slash) is rejected
        assert!(matches!(
            PubkyPath::parse("pub/app/file"),
            Err(Error::Request(RequestError::Validation { .. }))
        ));

        // Rendering a pubky:// URL from an agent-scoped path requires a default user
        let kp = Keypair::random();
        let user = kp.public_key();
        let url = p_abs.to_pubky_url(Some(&user)).unwrap();
        assert_eq!(url, format!("pubky://{}/pub/app/file", user));
    }

    #[test]
    fn error_cases() {
        let kp = Keypair::random();
        let user = kp.public_key();

        // Invalid user key in `<user>/<path>`
        assert!(matches!(
            PubkyPath::parse("not-a-key/pub/app"),
            Err(Error::Request(RequestError::Validation { .. }))
        ));

        // Double-slash inside path
        let s_bad = format!("{}/pub//app", user);
        assert!(matches!(
            PubkyPath::parse(&s_bad),
            Err(Error::Request(RequestError::Validation { .. }))
        ));
    }
}
