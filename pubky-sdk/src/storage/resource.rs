//! Typed addressing for files on a Pubky homeserver.
//!
//! Accepted inputs for `PubkyResource::parse`:
//! - `<user_pubkey>/<path>`            (preferred; explicit user)
//! - `/absolute/path`                  (session-scoped; user supplied elsewhere, must start with `/`)
//!
//! Note: We intentionally do **not** accept `https://_pubky.<pk>/...` here.

use std::{fmt, str::FromStr};

use pkarr::PublicKey;
use url::Url;

use crate::{Error, errors::RequestError};

const EXPECTED_FORMS: &str = "expected `<user>/<path>` (preferred), `/absolute/path` (session scoped) or `pubky://<user>/<path>` (legacy)";

#[inline]
fn invalid(msg: impl Into<String>) -> Error {
    RequestError::Validation {
        message: msg.into(),
    }
    .into()
}

/// Absolute, URL-safe homeserver path (always starts with `/`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ResourcePath(String);

impl ResourcePath {
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
    fn parse<S: AsRef<str>>(s: S) -> Result<Self, Error> {
        let raw = s.as_ref();
        if raw.is_empty() {
            return Err(invalid("path cannot be empty"));
        }

        // Normalize to absolute (keep "/" as-is).
        let input = if raw.starts_with('/') {
            raw.to_string()
        } else {
            format!("/{}", raw)
        };
        if input == "/" {
            return Ok(ResourcePath("/".to_string()));
        }
        let wants_trailing = input.ends_with('/');

        // Build via URL segments (handles percent-encoding; no dot-seg normalization here).
        // Use a dummy URL base.
        let mut u = Url::parse("dummy:///").map_err(|_| invalid("internal URL setup failed"))?;

        {
            // Clear and rebuild path from validated segments.
            let mut segs = u
                .path_segments_mut()
                .map_err(|_| invalid("internal URL path handling failed"))?;
            segs.clear();

            let mut parts = input.trim_start_matches('/').split('/').peekable();
            while let Some(seg) = parts.next() {
                if seg.is_empty() {
                    // Empty segment inside the path => "//" (not allowed).
                    // Allow only the final empty segment that represents a trailing slash.
                    if parts.peek().is_none() && wants_trailing {
                        break; // we'll encode trailing slash below
                    }
                    return Err(invalid("path contains empty segment ('//')"));
                }
                if seg == "." || seg == ".." {
                    return Err(invalid("path cannot contain '.' or '..'"));
                }
                segs.push(seg);
            }

            if wants_trailing {
                segs.push(""); // encode trailing slash
            }
        }

        Ok(ResourcePath(u.path().to_string()))
    }

    /// Borrow this normalized absolute path as `&str`.
    ///
    /// Zero-cost: returns a slice into the internal `String` without allocating.
    fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for ResourcePath {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl fmt::Display for ResourcePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A parsed homeserver address.
/// - `user: Some(..)` when the input was `<user>/...`
/// - `user: None`    when the input was an session-scoped path (e.g. `/foo/bar` or `foo/bar`)
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PubkyResource {
    pub(crate) user: Option<PublicKey>,
    path: ResourcePath,
}

impl PubkyResource {
    /// Construct from optional `PublicKey` and any string-y path.
    pub fn new<S: AsRef<str>>(user: Option<PublicKey>, path: S) -> Result<Self, Error> {
        Ok(Self {
            user,
            path: ResourcePath::parse(path)?,
        })
    }

    /// `pubky://<user>/<path>` requires a user; provide `default` to fill if missing.
    pub(crate) fn to_pubky_url(&self, default: Option<&PublicKey>) -> Result<String, Error> {
        let user = match (&self.user, default) {
            (Some(u), _) => u,
            (None, Some(d)) => d,
            (None, None) => return Err(invalid("missing user for pubky URL rendering")),
        };
        let rel = self.path.as_str().trim_start_matches('/');
        Ok(format!("pubky://{}/{}", user, rel))
    }
}

impl FromStr for PubkyResource {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // 1) Legacy scheme: pubky://<user>/<path>
        if let Some(rest) = s.strip_prefix("pubky://") {
            let (user_str, path) = rest
                .split_once('/')
                .ok_or_else(|| invalid("missing `<user>/<path>`"))?;

            let user = PublicKey::try_from(user_str)
                .map_err(|_| invalid(format!("invalid user public key: {user_str}")))?;
            return PubkyResource::new(Some(user), path);
        }

        // 2) `<user>/<path>`?
        if let Some((user_id, path)) = s.split_once('/') {
            if let Ok(user) = PublicKey::try_from(user_id) {
                return PubkyResource::new(Some(user), path);
            } else if !s.starts_with('/') {
                return Err(invalid(EXPECTED_FORMS));
            }
        }

        // 3) Agent-scoped path: must start with '/'
        if s.starts_with('/') {
            return PubkyResource::new(None, s);
        }

        Err(invalid(EXPECTED_FORMS))
    }
}

impl fmt::Display for PubkyResource {
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
/// Use this trait to normalize user input into a validated [`PubkyResource`]
/// without having to call `FromStr` manually. Implementations exist for:
/// - `&str` and `String` (parsed forms described above)
/// - `(PublicKey, P: AsRef<str>)` and `(&PublicKey, P: AsRef<str>)`
///   to pair an explicit user with a relative path
pub trait IntoPubkyResource {
    /// Convert `self` into a validated [`PubkyResource`].
    ///
    /// Normalizes to an absolute, percent-encoded homeserver path and, if present,
    /// binds the explicit user. Errors with [`Error::Request`] (validation) when the
    /// input is malformed (e.g., contains `//`, `.` / `..`, or a bad public key).
    ///
    /// Examples (pseudo):
    /// - `"pub/my.app/file".into_pubky_resource()` -> session-scoped resource
    /// - `(user_pk, "pub/app/file").into_pubky_resource()` ⇒ explicit user + relative path
    fn into_pubky_resource(self) -> Result<PubkyResource, Error>;
}

impl IntoPubkyResource for PubkyResource {
    #[inline]
    fn into_pubky_resource(self) -> Result<PubkyResource, Error> {
        Ok(self)
    }
}
impl IntoPubkyResource for &PubkyResource {
    #[inline]
    fn into_pubky_resource(self) -> Result<PubkyResource, Error> {
        Ok(self.clone())
    }
}
impl IntoPubkyResource for &str {
    fn into_pubky_resource(self) -> Result<PubkyResource, Error> {
        PubkyResource::from_str(self)
    }
}
impl IntoPubkyResource for String {
    fn into_pubky_resource(self) -> Result<PubkyResource, Error> {
        PubkyResource::from_str(&self)
    }
}
impl<P: AsRef<str>> IntoPubkyResource for (PublicKey, P) {
    fn into_pubky_resource(self) -> Result<PubkyResource, Error> {
        PubkyResource::new(Some(self.0), self.1.as_ref())
    }
}
impl<P: AsRef<str>> IntoPubkyResource for (&PublicKey, P) {
    fn into_pubky_resource(self) -> Result<PubkyResource, Error> {
        PubkyResource::new(Some(self.0.clone()), self.1.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkarr::Keypair;

    #[test]
    fn file_path_normalization_and_rejections() {
        // Normalize relative
        assert_eq!(
            ResourcePath::parse("pub/app").unwrap().as_str(),
            "pub/my.app"
        );
        // Keep absolute
        assert_eq!(
            ResourcePath::parse("pub/my.app").unwrap().as_str(),
            "pub/my.app"
        );
        // Reject empty
        assert!(matches!(
            ResourcePath::parse(""),
            Err(Error::Request(RequestError::Validation { .. }))
        ));
        // Reject double-slash
        assert!(matches!(
            ResourcePath::parse("/pub//app"),
            Err(Error::Request(RequestError::Validation { .. }))
        ));
    }

    #[test]
    fn parse_explicit_user_both_forms() {
        let kp = Keypair::random();
        let user = kp.public_key();
        let s1 = format!("pubky://{}pub/my.app/file", user);
        let s2 = format!("{}pub/my.app/file", user);

        let p1 = PubkyResource::from_str(&s1).unwrap();
        let p2 = PubkyResource::from_str(&s2).unwrap();

        assert_eq!(p1.user, Some(user.clone()));
        assert_eq!(p2.user, Some(user.clone()));
        assert_eq!(p1.path.as_str(), "pub/my.app/file");
        assert_eq!(p2.path.as_str(), "pub/my.app/file");

        // Display: explicit user form
        assert_eq!(p1.to_string(), s2);
        // URL rendering without default is fine when user exists
        assert_eq!(p1.to_pubky_url(None).unwrap(), s1);
    }

    #[test]
    fn parse_session_scoped_paths() {
        // Absolute, session-scoped path is OK
        let p_abs = PubkyResource::from_str("pub/my.app/file").unwrap();
        assert!(p_abs.user.is_none());
        assert_eq!(p_abs.path.as_str(), "pub/my.app/file");

        // Relative session-scoped (no leading slash) is rejected
        assert!(matches!(
            PubkyResource::from_str("pub/app/file"),
            Err(Error::Request(RequestError::Validation { .. }))
        ));

        // Rendering a pubky:// URL from an session-scoped path requires a default user
        let kp = Keypair::random();
        let user = kp.public_key();
        let url = p_abs.to_pubky_url(Some(&user)).unwrap();
        assert_eq!(url, format!("pubky://{}pub/my.app/file", user));
    }

    #[test]
    fn error_cases() {
        let kp = Keypair::random();
        let user = kp.public_key();

        // Invalid user key in `<user>/<path>`
        assert!(matches!(
            PubkyResource::from_str("not-a-keypub/my.app"),
            Err(Error::Request(RequestError::Validation { .. }))
        ));

        // Double-slash inside path
        let s_bad = format!("{}/pub//app", user);
        assert!(matches!(
            PubkyResource::from_str(&s_bad),
            Err(Error::Request(RequestError::Validation { .. }))
        ));
    }

    #[test]
    fn percent_encoding_and_unicode() {
        assert_eq!(
            ResourcePath::parse("pub/My File.txt").unwrap().as_str(),
            "/pub/My%20File.txt"
        );
        assert_eq!(
            ResourcePath::parse("/ä/β/漢").unwrap().as_str(),
            "/%C3%A4/%CE%B2/%E6%BC%A2"
        );
    }

    #[test]
    fn rejects_dot_segments_but_allows_trailing_slash() {
        assert!(ResourcePath::parse("/a/./b").is_err());
        assert!(ResourcePath::parse("/a/../b").is_err());
        assert_eq!(
            ResourcePath::parse("pub/my.app/").unwrap().as_str(),
            "pub/my.app/"
        );
    }
}
