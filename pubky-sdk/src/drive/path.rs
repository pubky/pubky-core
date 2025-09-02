//! Typed addressing for files on a Pubky homeserver.
//!
//! Accepted inputs for `PubkyPath::parse`:
//! - `<user_pubkey>/<path>`            (preferred; explicit user)
//! - `/absolute/path`                  (agent-scoped; user supplied elsewhere, must start with `/`)
//! - `pubky://<user_pubkey>/<path>`    (URL compliant)
//!
//! Note: We intentionally do **not** accept `https://_pubky.<pk>/...` here.

use std::{fmt, str::FromStr};

use pkarr::PublicKey;
use url::Url;

use crate::{Error, errors::RequestError};

const EXPECTED_FORMS: &str = "expected `<user>/<path>` (preferred), `/absolute/path` (agent scoped) or `pubky://<user>/<path>` (URL compliant)";

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
    /// Parse, validate, and normalize to an absolute HTTP-safe path.
    ///
    /// Rules:
    /// - Prepend a leading `/` if missing.
    /// - No empty *internal* segments (rejects `//`), but preserves a trailing `/`.
    /// - For safety, rejects `.` and `..` segments.
    /// - Canonicalizes/percent-encodes segments using `url::Url` (UTF-8).
    ///
    /// Note: Provide *raw* human-readable segments (e.g. `"pub/My File.txt"`). Any literal `%`
    /// will be encoded; pre-encoded sequences are **not** interpreted specially.
    pub fn parse<S: AsRef<str>>(s: S) -> Result<Self, Error> {
        let raw = s.as_ref();
        if raw.is_empty() {
            return Err(invalid("path cannot be empty"));
        }

        // Normalize to absolute.
        let input = if raw.starts_with('/') {
            raw.to_string()
        } else {
            format!("/{}", raw)
        };

        // Quick check for internal empty segments (we still allow a trailing '/').
        if input.contains("//") && !input.ends_with("//") {
            return Err(invalid("path contains empty segment ('//')"));
        }

        // Preserve whether user asked for a trailing slash (except for root).
        let wants_trailing = input.len() > 1 && input.ends_with('/');

        // Rebuild via URL segments for RFC-compliant encoding.
        let mut u = Url::parse("https://example.invalid")
            .map_err(|_| invalid("internal URL setup failed"))?;
        {
            let mut segs = u
                .path_segments_mut()
                .map_err(|_| url::ParseError::RelativeUrlWithCannotBeABaseBase)
                .map_err(|_| invalid("internal URL path handling failed"))?;
            segs.clear();

            // Skip the leading empty segment from the absolute path.
            for seg in input.trim_start_matches('/').split('/') {
                if seg.is_empty() {
                    // allow trailing slash only
                    continue;
                }
                if seg == "." || seg == ".." {
                    return Err(invalid("path cannot contain '.' or '..'"));
                }
                segs.push(seg); // will percent-encode as needed
            }

            if wants_trailing {
                // Empty segment encodes a trailing slash.
                segs.push("");
            }
        }

        Ok(FilePath(u.path().to_string()))
    }

    /// Borrow this normalized absolute path as `&str`.
    ///
    /// Zero-cost: returns a slice into the internal `String` without allocating.
    ///
    /// # Example
    /// ```
    /// # use pubky::FilePath;
    /// let p = FilePath::parse("pub/app")?;
    /// assert_eq!(p.as_str(), "/pub/app");
    /// # Ok::<_, pubky::Error>(())
    /// ```
    pub fn as_str(&self) -> &str {
        &self.0
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
    pub(crate) user: Option<PublicKey>,
    path: FilePath,
}

impl PubkyPath {
    /// Construct from optional `PublicKey` and any string-y path.
    pub fn new<S: AsRef<str>>(user: Option<PublicKey>, path: S) -> Result<Self, Error> {
        Ok(Self {
            user,
            path: FilePath::parse(path)?,
        })
    }

    /// `pubky://<user>/<path>` requires a user; provide `default` to fill if missing.
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

impl FromStr for PubkyPath {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // 1) Legacy scheme: pubky://<user>/<path>
        if let Some(rest) = s.strip_prefix("pubky://") {
            let (user_str, path) = rest
                .split_once('/')
                .ok_or_else(|| invalid("missing `<user>/<path>`"))?;

            let user = PublicKey::try_from(user_str)
                .map_err(|_| invalid(format!("invalid user public key: {user_str}")))?;
            return PubkyPath::new(Some(user), path);
        }

        // 2) `<user>/<path>`?
        if let Some((user_id, path)) = s.split_once('/') {
            if let Ok(user) = PublicKey::try_from(user_id) {
                return PubkyPath::new(Some(user), path);
            } else if !s.starts_with('/') {
                return Err(invalid(EXPECTED_FORMS));
            }
        }

        // 3) Agent-scoped path: must start with '/'
        if s.starts_with('/') {
            return PubkyPath::new(None, s);
        }

        Err(invalid(EXPECTED_FORMS))
    }
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
/// Minimal, ergonomic conversions accepted by high-level APIs.
///
/// Keep the typed `PublicKey` tuple forms (safe & explicit), plus common string forms.
/// We intentionally **do not** accept `(&str, P)` to avoid “stringly-typed pubky” misuse.
pub trait IntoPubkyPath {
    fn into_pubky_path(self) -> Result<PubkyPath, Error>;
}

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
impl IntoPubkyPath for &str {
    fn into_pubky_path(self) -> Result<PubkyPath, Error> {
        PubkyPath::from_str(self)
    }
}
impl IntoPubkyPath for String {
    fn into_pubky_path(self) -> Result<PubkyPath, Error> {
        PubkyPath::from_str(&self)
    }
}
impl<P: AsRef<str>> IntoPubkyPath for (PublicKey, P) {
    fn into_pubky_path(self) -> Result<PubkyPath, Error> {
        PubkyPath::new(Some(self.0), self.1.as_ref())
    }
}
impl<P: AsRef<str>> IntoPubkyPath for (&PublicKey, P) {
    fn into_pubky_path(self) -> Result<PubkyPath, Error> {
        PubkyPath::new(Some(self.0.clone()), self.1.as_ref())
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

        let p1 = PubkyPath::from_str(&s1).unwrap();
        let p2 = PubkyPath::from_str(&s2).unwrap();

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
        let p_abs = PubkyPath::from_str("/pub/app/file").unwrap();
        assert!(p_abs.user.is_none());
        assert_eq!(p_abs.path.as_str(), "/pub/app/file");

        // Relative agent-scoped (no leading slash) is rejected
        assert!(matches!(
            PubkyPath::from_str("pub/app/file"),
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
            PubkyPath::from_str("not-a-key/pub/app"),
            Err(Error::Request(RequestError::Validation { .. }))
        ));

        // Double-slash inside path
        let s_bad = format!("{}/pub//app", user);
        assert!(matches!(
            PubkyPath::from_str(&s_bad),
            Err(Error::Request(RequestError::Validation { .. }))
        ));
    }

    #[test]
    fn percent_encoding_and_unicode() {
        assert_eq!(
            FilePath::parse("pub/My File.txt").unwrap().as_str(),
            "/pub/My%20File.txt"
        );
        assert_eq!(
            FilePath::parse("/ä/β/漢").unwrap().as_str(),
            "/%C3%A4/%CE%B2/%E6%BC%A2"
        );
    }

    #[test]
    fn rejects_dot_segments_but_allows_trailing_slash() {
        assert!(FilePath::parse("/a/./b").is_err());
        assert!(FilePath::parse("/a/../b").is_err());
        assert_eq!(FilePath::parse("/pub/app/").unwrap().as_str(), "/pub/app/");
    }
}
