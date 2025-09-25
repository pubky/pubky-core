//! Typed addressing for files on a Pubky homeserver.
//!
//! # Why two address shapes?
//! Pubky paths come in two *disjoint* forms, each used by a different part of the API:
//!
//! - [`ResourcePath`]: an **absolute**, URL-safe path like `"/pub/my.app/file"`.
//!   It contains **no user** information and is used by *session-scoped* (authenticated)
//!   operations that act “as me”. Example: `session.storage().get("/pub/my.app/file")`.
//!
//! - [`PubkyResource`]: an **addressed resource** that pairs a user with an absolute path,
//!   e.g. `"<public_key>/pub/my.app/file"` or `pubky://<public_key>/pub/my.app/file`.
//!   It is used by *public* (unauthenticated) operations and any API that must
//!   target **another user’s** data. Example: `public.get("<pk>/pub/site/index.html")`.
//!
//! Keeping these distinct eliminates ambiguity and makes IDE auto-completion
//! tell you exactly what each method expects.
//!
//! We intentionally do **not** accept `https://_pubky.<pk>/...` here; higher-level
//! APIs handle resolution and URL formation for you.

use std::{fmt, str::FromStr};

use pkarr::PublicKey;
use url::Url;

use crate::{Error, errors::RequestError};

#[inline]
fn invalid(msg: impl Into<String>) -> Error {
    RequestError::Validation {
        message: msg.into(),
    }
    .into()
}

// ============================================================================
// ResourcePath
// ============================================================================

/// An **absolute, URL-safe** homeserver path (`/…`), with percent-encoding where needed.
///
/// - Always normalized to start with `/`.
/// - Rejects `.` and `..` segments (no path traversal).
/// - Rejects empty **internal** segments (i.e., `//`); preserves a trailing `/`.
/// - Percent-encodes segments using `url::Url` rules (UTF-8).
///
/// Accepts both `"pub/my.app/file"` and `"/pub/my.app/file"` and normalizes to an
/// absolute form.
///
/// ### Examples
/// ```no_run
/// # use pubky::ResourcePath;
/// // Parse from &str (relative becomes absolute)
/// let p = ResourcePath::parse("pub/my.app/file")?;
/// assert_eq!(p.to_string(), "/pub/my.app/file");
///
/// // Trailing slash is preserved
/// let dir = ResourcePath::parse("/pub/my.app/")?;
/// assert_eq!(dir.to_string(), "/pub/my.app/");
///
/// // Percent-encoding
/// let enc = ResourcePath::parse("pub/My File.txt")?;
/// assert_eq!(enc.to_string(), "/pub/My%20File.txt");
/// # Ok::<(), pubky::Error>(())
/// ```
///
/// ### Errors
/// Returns [`Error::Request`] (validation) on:
/// - empty input
/// - `//` within the path
/// - `.` or `..` segments
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ResourcePath(String);

impl ResourcePath {
    /// Parse, validate, and normalize to an absolute HTTP-safe path.
    ///
    /// See type docs for rules and examples.
    pub fn parse<S: AsRef<str>>(s: S) -> Result<Self, Error> {
        let raw = s.as_ref();
        if raw.is_empty() {
            return Err(invalid("path cannot be empty"));
        }

        // Normalize to absolute (keep "/" as-is).
        let input = if raw.starts_with('/') {
            raw.to_string()
        } else {
            format!("/{raw}")
        };
        if input == "/" {
            return Ok(ResourcePath("/".to_string()));
        }
        let wants_trailing = input.ends_with('/');

        // Build via URL path segments (handles percent-encoding).
        let mut u = Url::parse("dummy:///").map_err(|_| invalid("internal URL setup failed"))?;
        {
            let mut segs = u
                .path_segments_mut()
                .map_err(|_| invalid("internal URL path handling failed"))?;
            segs.clear();

            let mut parts = input.trim_start_matches('/').split('/').peekable();
            while let Some(seg) = parts.next() {
                if seg.is_empty() {
                    // Only allow the final empty segment (trailing slash)
                    if parts.peek().is_none() && wants_trailing {
                        break;
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

    /// Borrow the normalized absolute path as `&str`.
    #[inline]
    pub fn as_str(&self) -> &str {
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

// ============================================================================
// PubkyResource
// ============================================================================

/// An **addressed resource**: `(owner: PublicKey, path: ResourcePath)`.
///
/// This is the unambiguous “user + absolute path” form used when acting on
/// **another user’s** data (public reads, etc.).
///
/// Accepted inputs for `FromStr`:
/// - `<public_key>/<abs-path>`
/// - `pubky://<public_key>/<abs-path>`
///
/// Display renders as `<public_key>/<abs-path>`.
///
/// ### Examples
/// ```no_run
/// # use pkarr::Keypair;
/// # use pubky::{PubkyResource, ResourcePath};
/// // Build from parts
/// let pk = Keypair::random().public_key();
/// let r = PubkyResource::new(pk.clone(), "/pub/site/index.html")?;
/// assert_eq!(r.to_string(), format!("{pk}/pub/site/index.html"));
///
/// // Parse from string
/// let parsed: PubkyResource = format!("{pk}/pub/site/index.html").parse()?;
///
/// // `pubky://` form
/// let parsed2: PubkyResource = format!("pubky://{pk}/pub/site/index.html").parse()?;
///
/// # Ok::<(), pubky::Error>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PubkyResource {
    /// The resource owner’s public key.
    pub owner: PublicKey,
    /// The owner-relative, normalized absolute path.
    pub path: ResourcePath,
}

impl PubkyResource {
    /// Construct from `owner` and a path-like value (normalized to [`ResourcePath`]).
    pub fn new<S: AsRef<str>>(owner: PublicKey, path: S) -> Result<Self, Error> {
        Ok(Self {
            owner,
            path: ResourcePath::parse(path)?,
        })
    }

    /// Render as `pubky://<owner>/<abs-path>` (deep-link form).
    ///
    /// This is crate-internal but documented here for clarity.
    pub(crate) fn to_pubky_url(&self) -> String {
        let rel = self.path.as_str().trim_start_matches('/');
        format!("pubky://{}/{}", self.owner, rel)
    }
}

impl FromStr for PubkyResource {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // 1) pubky://<user>/<path>
        if let Some(rest) = s.strip_prefix("pubky://") {
            let (user_str, path) = rest
                .split_once('/')
                .ok_or_else(|| invalid("missing `<user>/<path>`"))?;
            let user = PublicKey::try_from(user_str)
                .map_err(|_| invalid(format!("invalid user public key: {user_str}")))?;
            return PubkyResource::new(user, path);
        }

        // 2) `<user>/<path>` (must have a slash separating pk and rest)
        if let Some((user_id, path)) = s.split_once('/') {
            let user = PublicKey::try_from(user_id)
                .map_err(|_| invalid("expected `<user>/<path>` or `pubky://<user>/<path>`"))?;
            return PubkyResource::new(user, path);
        }

        Err(invalid(
            "expected `<user>/<path>` or `pubky://<user>/<path>`",
        ))
    }
}

impl fmt::Display for PubkyResource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rel = self.path.as_str().trim_start_matches('/');
        write!(f, "{}/{}", self.owner, rel)
    }
}

// ============================================================================
// Conversion traits
// ============================================================================

/// Convert common input types into a normalized [`ResourcePath`] (absolute).
///
/// This trait is intentionally implemented for the “obvious” path-like things:
/// - `ResourcePath` / `&ResourcePath` (pass-through / clone)
/// - `&str`, `String`, and `&String`
///
/// It is used by *session-scoped* storage methods (`SessionStorage`) that act as
/// the current user and therefore do **not** need a `PublicKey`.
///
/// ### Examples
/// ```no_run
/// # use pubky::{IntoResourcePath, ResourcePath};
/// fn takes_abs<P: IntoResourcePath>(p: P) -> pubky::Result<ResourcePath> {
///     p.into_abs_path()
/// }
///
/// let a = takes_abs("/pub/my.app/file")?;
/// let b = takes_abs("pub/my.app/file")?;
/// assert_eq!(a, b);
/// # Ok::<(), pubky::Error>(())
/// ```
pub trait IntoResourcePath {
    /// Convert into a validated, normalized absolute [`ResourcePath`].
    fn into_abs_path(self) -> Result<ResourcePath, Error>;
}

impl IntoResourcePath for ResourcePath {
    #[inline]
    fn into_abs_path(self) -> Result<ResourcePath, Error> {
        Ok(self)
    }
}
impl IntoResourcePath for &ResourcePath {
    #[inline]
    fn into_abs_path(self) -> Result<ResourcePath, Error> {
        Ok(self.clone())
    }
}
impl IntoResourcePath for &str {
    fn into_abs_path(self) -> Result<ResourcePath, Error> {
        ResourcePath::from_str(self)
    }
}
impl IntoResourcePath for String {
    fn into_abs_path(self) -> Result<ResourcePath, Error> {
        ResourcePath::from_str(&self)
    }
}
impl IntoResourcePath for &String {
    fn into_abs_path(self) -> Result<ResourcePath, Error> {
        ResourcePath::from_str(self.as_str())
    }
}

/// Convert common input types into a normalized, **addressed** [`PubkyResource`].
///
/// Implementations:
/// - `PubkyResource` / `&PubkyResource` (pass-through / clone)
/// - `&str`, `String`, `&String` parsed as `<pk>/<abs-path>` or `pubky://<pk>/<abs-path>`
/// - `(PublicKey, P: AsRef<str>)` and `(&PublicKey, P: AsRef<str>)` to pair a key with a path
///
/// This trait is used by *public* storage methods (`PublicStorage`) and any API that must
/// reference **another user’s** data explicitly.
///
/// ### Examples
/// ```no_run
/// # use pkarr::Keypair;
/// # use pubky::{IntoPubkyResource, PubkyResource};
/// let user = Keypair::random().public_key();
///
/// // Pair (pk, path)
/// let r1 = (user.clone(), "/pub/site/index.html").into_pubky_resource()?;
///
/// // Parse `<pk>/<path>`
/// let r2: PubkyResource = format!("{}/pub/site/index.html", user).parse()?;
///
/// // Parse `pubky://`
/// let r3: PubkyResource = format!("pubky://{}/pub/site/index.html", user).parse()?;
/// # Ok::<(), pubky::Error>(())
/// ```
pub trait IntoPubkyResource {
    /// Convert into a validated, normalized [`PubkyResource`].
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
impl IntoPubkyResource for &String {
    fn into_pubky_resource(self) -> Result<PubkyResource, Error> {
        PubkyResource::from_str(self.as_str())
    }
}
impl<P: AsRef<str>> IntoPubkyResource for (PublicKey, P) {
    fn into_pubky_resource(self) -> Result<PubkyResource, Error> {
        PubkyResource::new(self.0, self.1.as_ref())
    }
}
impl<P: AsRef<str>> IntoPubkyResource for (&PublicKey, P) {
    fn into_pubky_resource(self) -> Result<PubkyResource, Error> {
        PubkyResource::new(self.0.clone(), self.1.as_ref())
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
            ResourcePath::parse("pub/my.app").unwrap().as_str(),
            "/pub/my.app"
        );
        // Keep absolute
        assert_eq!(
            ResourcePath::parse("/pub/my.app").unwrap().as_str(),
            "/pub/my.app"
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
    fn parse_addressed_user_both_forms() {
        let kp = Keypair::random();
        let user = kp.public_key();
        let s1 = format!("pubky://{}/pub/my.app/file", user);
        let s2 = format!("{}/pub/my.app/file", user);

        let p1 = PubkyResource::from_str(&s1).unwrap();
        let p2 = PubkyResource::from_str(&s2).unwrap();

        assert_eq!(p1.owner, user);
        assert_eq!(p2.owner, user);
        assert_eq!(p1.path.as_str(), "/pub/my.app/file");
        assert_eq!(p2.path.as_str(), "/pub/my.app/file");

        // Display: explicit user form
        assert_eq!(p1.to_string(), s2);
        // Deep-link rendering (no default needed; owner is known)
        assert_eq!(p1.to_pubky_url(), s1);
    }

    #[test]
    fn session_scoped_paths_and_rendering() {
        // Session-scoped absolute path is represented by ResourcePath
        let p_abs = ResourcePath::parse("/pub/my.app/file").unwrap();
        assert_eq!(p_abs.as_str(), "/pub/my.app/file");

        // PubkyResource::from_str("/...") must fail (owner is required)
        assert!(matches!(
            PubkyResource::from_str("/pub/my.app/file"),
            Err(Error::Request(RequestError::Validation { .. }))
        ));

        // To render a pubky:// URL, pair with an explicit owner
        let kp = Keypair::random();
        let user = kp.public_key();
        let r = PubkyResource::new(user.clone(), p_abs.as_str()).unwrap();
        assert_eq!(
            r.to_pubky_url(),
            format!("pubky://{}/pub/my.app/file", user)
        );
    }

    #[test]
    fn error_cases() {
        let kp = Keypair::random();
        let user = kp.public_key();

        // Invalid user key in `<user>/<path>`
        assert!(matches!(
            PubkyResource::from_str("not-a-key/pub/my.app"),
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
            ResourcePath::parse("/pub/my.app/").unwrap().as_str(),
            "/pub/my.app/"
        );
    }
}
