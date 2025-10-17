//! Capabilities define *what* a bearer can access (a scoped path) and *how* (a set of actions).
//!
//! ## String format
//!
//! A single capability is serialized as: `"<scope>:<actions>"`
//!
//! - `scope` must start with `/` (e.g. `"/pub/my-cool-app/"`, `"/"`).
//! - `actions` is a compact string of letters, currently:
//!   - `r` => read (GET)
//!   - `w` => write (PUT/POST/DELETE)
//!
//! Examples:
//!
//! - Read+write everything: `"/:rw"`
//! - Read-only a file: `"/pub/foo.txt:r"`
//! - Read-write a directory: `"/pub/my-cool-app/:rw"`
//!
//! Multiple capabilities are serialized as a comma-separated list,
//! e.g. `"/pub/my-cool-app/:rw,/pub/foo.txt:r"`.
//!
//! ## Builder ergonomics
//!
//! ```rust
//! use pubky_common::capabilities::{Capability, Capabilities};
//!
//! // Single-cap builder
//! let cap = Capability::builder("/pub/my-cool-app/")
//!     .read()
//!     .write()
//!     .finish();
//! assert_eq!(cap.to_string(), "/pub/my-cool-app/:rw");
//!
//! // Multiple caps builder
//! let caps = Capabilities::builder()
//!     .read_write("/pub/my-cool-app/")
//!     .read("/pub/foo.txt")
//!     .finish();
//! assert_eq!(caps.to_string(), "/pub/my-cool-app/:rw,/pub/foo.txt:r");
//! ```

use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fmt::Display};
use url::Url;

/// A single capability: a `scope` and the allowed `actions` within it.
///
/// The wire/string representation is `"<scope>:<actions>"`, see module docs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capability {
    /// Scope of resources (e.g. a directory or file). Must start with `/`.
    pub scope: String,
    /// Allowed actions within `scope`. Serialized as a compact action string (e.g. `"rw"`).
    pub actions: Vec<Action>,
}

impl Capability {
    /// Shorthand for a root capability at `/` with read+write.
    ///
    /// Equivalent to `Capability { scope: "/".into(), actions: vec![Read, Write] }`.
    ///
    /// ```
    /// use pubky_common::capabilities::Capability;
    /// assert_eq!(Capability::root().to_string(), "/:rw");
    /// ```
    pub fn root() -> Self {
        Capability {
            scope: "/".to_string(),
            actions: vec![Action::Read, Action::Write],
        }
    }

    // ---- Shortcut constructors

    /// Construct a read-only capability for `scope`.
    ///
    /// The scope is normalized to start with `/` if it does not already.
    ///
    /// ```
    /// use pubky_common::capabilities::Capability;
    /// assert_eq!(Capability::read("pub/my.app").to_string(), "/pub/my.app:r");
    /// ```
    #[inline]
    pub fn read<S: Into<String>>(scope: S) -> Self {
        Self::builder(scope).read().finish()
    }

    /// Construct a write-only capability for `scope`.
    ///
    /// ```
    /// use pubky_common::capabilities::Capability;
    /// assert_eq!(Capability::write("/pub/tmp").to_string(), "/pub/tmp:w");
    /// ```
    #[inline]
    pub fn write<S: Into<String>>(scope: S) -> Self {
        Self::builder(scope).write().finish()
    }

    /// Construct a read+write capability for `scope`.
    ///
    /// ```
    /// use pubky_common::capabilities::Capability;
    /// assert_eq!(Capability::read_write("/").to_string(), "/:rw");
    /// ```
    #[inline]
    pub fn read_write<S: Into<String>>(scope: S) -> Self {
        Self::builder(scope).read().write().finish()
    }

    /// Start building a single capability for `scope`.
    ///
    /// The scope is normalized to have a leading `/`.
    ///
    /// ```
    /// use pubky_common::capabilities::Capability;
    /// let cap = Capability::builder("pub/my.app").read().finish();
    /// assert_eq!(cap.to_string(), "/pub/my.app:r");
    /// ```
    pub fn builder<S: Into<String>>(scope: S) -> CapabilityBuilder {
        CapabilityBuilder {
            scope: normalize_scope(scope.into()),
            actions: BTreeSet::new(),
        }
    }

    fn covers(&self, other: &Capability) -> bool {
        if !scope_covers(&self.scope, &other.scope) {
            return false;
        }

        other
            .actions
            .iter()
            .all(|action| self.actions.contains(action))
    }
}

/// Fluent builder for a single [`Capability`].
///
/// Use [`Capability::builder`] to construct, then chain `.read()/.write()` and `.finish()`.
#[derive(Debug, Default)]
pub struct CapabilityBuilder {
    scope: String,
    actions: BTreeSet<Action>,
}

impl CapabilityBuilder {
    /// Allow **read** (GET) within the scope.
    pub fn read(mut self) -> Self {
        self.actions.insert(Action::Read);
        self
    }

    /// Allow **write** (PUT/POST/DELETE) within the scope.
    pub fn write(mut self) -> Self {
        self.actions.insert(Action::Write);
        self
    }

    /// Allow a specific action. Useful if more actions are added in the future.
    pub fn allow(mut self, action: Action) -> Self {
        self.actions.insert(action);
        self
    }

    /// Finalize and produce the [`Capability`].
    ///
    /// Actions are de-duplicated and emitted in a stable order.
    pub fn finish(self) -> Capability {
        let v: Vec<Action> = self.actions.into_iter().collect();
        // BTreeSet sorts; keep stable & dedup’d
        Capability {
            scope: self.scope,
            actions: v,
        }
    }
}

/// Actions allowed on a given scope.
///
/// Display/serialization encodes these as single characters (`r`, `w`).
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Action {
    /// Can read the scope at the specified path (GET requests).
    Read,
    /// Can write to the scope at the specified path (PUT/POST/DELETE requests).
    Write,
    /// Unknown ability
    Unknown(char),
}

impl From<&Action> for char {
    fn from(value: &Action) -> Self {
        match value {
            Action::Read => 'r',
            Action::Write => 'w',
            Action::Unknown(char) => char.to_owned(),
        }
    }
}

impl TryFrom<char> for Action {
    type Error = Error;

    fn try_from(value: char) -> Result<Self, Error> {
        match value {
            'r' => Ok(Self::Read),
            'w' => Ok(Self::Write),
            _ => Err(Error::InvalidAction),
        }
    }
}

impl Display for Capability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}",
            self.scope,
            self.actions.iter().map(char::from).collect::<String>()
        )
    }
}

impl TryFrom<String> for Capability {
    type Error = Error;

    fn try_from(value: String) -> Result<Self, Error> {
        value.as_str().try_into()
    }
}

impl TryFrom<&str> for Capability {
    type Error = Error;
    /// Parse `"<scope>:<actions>"`. Scope must start with `/`; actions must be valid letters.
    ///
    /// ```
    /// use pubky_common::capabilities::Capability;
    /// let cap: Capability = "/pub/my-cool-app/:rw".try_into().unwrap();
    /// assert_eq!(cap.to_string(), "/pub/my-cool-app/:rw");
    /// ```
    fn try_from(value: &str) -> Result<Self, Error> {
        if value.matches(':').count() != 1 {
            return Err(Error::InvalidFormat);
        }

        if !value.starts_with('/') {
            return Err(Error::InvalidScope);
        }

        let actions_str = value.rsplit(':').next().unwrap_or("");

        let mut actions = Vec::new();

        for char in actions_str.chars() {
            let ability = Action::try_from(char)?;

            match actions.binary_search_by(|element| char::from(element).cmp(&char)) {
                Ok(_) => {}
                Err(index) => {
                    actions.insert(index, ability);
                }
            }
        }

        let scope = value[0..value.len() - actions_str.len() - 1].to_string();

        Ok(Capability { scope, actions })
    }
}

impl Serialize for Capability {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let string = self.to_string();

        string.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Capability {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let string: String = Deserialize::deserialize(deserializer)?;

        string.try_into().map_err(serde::de::Error::custom)
    }
}

#[derive(thiserror::Error, Debug, PartialEq, Eq)]
/// Error parsing a [Capability].
pub enum Error {
    #[error("Capability: Invalid scope: does not start with `/`")]
    /// Capability: Invalid scope: does not start with `/`
    InvalidScope,
    #[error("Capability: Invalid format should be <scope>:<abilities>")]
    /// Capability: Invalid format should be `<scope>:<abilities>`
    InvalidFormat,
    #[error("Capability: Invalid Action")]
    /// Capability: Invalid Action
    InvalidAction,
    #[error("Capabilities: Invalid capabilities format")]
    /// Capabilities: Invalid capabilities format
    InvalidCapabilities,
}

/// A wrapper around `Vec<Capability>` that controls how capabilities are
/// serialized and built.
///
/// Serialization is a single comma-separated string (e.g. `"/:rw,/pub/my-cool-app/:r"`),
/// which is convenient for logs, URLs, or compact text payloads. It also comes
/// with a fluent builder (`Capabilities::builder()`).
///
/// Note: this does **not** remove length prefixes in binary encodings; if you
/// need a varint-free trailing field in a custom binary format, implement a
/// bespoke encoder/decoder instead of serde.
#[derive(Clone, Default, Debug, PartialEq, Eq)]
#[must_use]
pub struct Capabilities(pub Vec<Capability>);

impl Capabilities {
    /// Returns true if the list contains `capability`.
    pub fn contains(&self, capability: &Capability) -> bool {
        self.0.contains(capability)
    }

    /// Returns `true` if the list is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns the number of entries.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns an iterator over the slice of [Capability].
    pub fn iter(&self) -> std::slice::Iter<'_, Capability> {
        self.0.iter()
    }

    /// Parse capabilities from the `caps` query parameter.
    ///
    /// Expects a comma-separated list of capability strings, e.g.:
    /// `?caps=/pub/my-cool-app/:rw,/foo:r`
    ///
    /// Invalid entries are ignored.
    ///
    /// # Examples
    /// ```
    /// # use url::Url;
    /// # use pubky_common::capabilities::Capabilities;
    /// let url = Url::parse("https://example/app?caps=/pub/my-cool-app/:rw,/foo:r").unwrap();
    /// let caps = Capabilities::from_url(&url);
    /// assert!(!caps.is_empty());
    /// ```
    pub fn from_url(url: &Url) -> Self {
        // Get the first `caps` entry if present.
        let value = url
            .query_pairs()
            .find_map(|(k, v)| (k == "caps").then(|| v.to_string()))
            .unwrap_or_default();

        // Parse comma-separated capabilities, skipping invalid pieces.
        let caps = value
            .split(',')
            .filter_map(|s| Capability::try_from(s).ok())
            .collect();

        Capabilities(sanitize_caps(caps))
    }

    /// Start a fluent builder for multiple capabilities.
    ///
    /// ```
    /// use pubky_common::capabilities::Capabilities;
    /// let caps = Capabilities::builder().read_write("/").finish();
    /// assert_eq!(caps.to_string(), "/:rw");
    /// ```
    pub fn builder() -> CapsBuilder {
        CapsBuilder::default()
    }
}

/// Fluent builder for multiple [`Capability`] entries.
///
/// Build with high-level helpers (`.read()/.write()/.read_write()`), or push prebuilt
/// capabilities with `.cap()`, or use `.capability(scope, |b| ...)` to build inline.
#[derive(Default, Debug)]
pub struct CapsBuilder {
    caps: Vec<Capability>,
}

impl CapsBuilder {
    /// Create a new empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Push a prebuilt capability
    pub fn cap(mut self, cap: Capability) -> Self {
        self.caps.push(cap);
        self
    }

    /// Build a capability inline and push it:
    ///
    /// ```
    /// use pubky_common::capabilities::Capabilities;
    /// let caps = Capabilities::builder()
    ///     .capability("/pub/my-cool-app/", |b| b.read().write())
    ///     .finish();
    /// assert_eq!(caps.to_string(), "/pub/my-cool-app/:rw");
    /// ```
    pub fn capability<F>(mut self, scope: impl Into<String>, f: F) -> Self
    where
        F: FnOnce(CapabilityBuilder) -> CapabilityBuilder,
    {
        let cap = f(Capability::builder(scope)).finish();
        self.caps.push(cap);
        self
    }

    /// Add a read-only capability for `scope`.
    pub fn read(mut self, scope: impl Into<String>) -> Self {
        self.caps.push(Capability::read(scope));
        self
    }

    /// Add a write-only capability for `scope`.
    pub fn write(mut self, scope: impl Into<String>) -> Self {
        self.caps.push(Capability::write(scope));
        self
    }

    /// Add a read+write capability for `scope`.
    pub fn read_write(mut self, scope: impl Into<String>) -> Self {
        self.caps.push(Capability::read_write(scope));
        self
    }

    /// Extend with an iterator of capabilities.
    pub fn extend<I: IntoIterator<Item = Capability>>(mut self, iter: I) -> Self {
        self.caps.extend(iter);
        self
    }

    /// Finalize and produce the [`Capabilities`] list.
    pub fn finish(self) -> Capabilities {
        Capabilities(sanitize_caps(self.caps))
    }
}

impl From<Vec<Capability>> for Capabilities {
    fn from(value: Vec<Capability>) -> Self {
        Self(value)
    }
}

impl From<Capabilities> for Vec<Capability> {
    fn from(value: Capabilities) -> Self {
        value.0
    }
}

impl TryFrom<&str> for Capabilities {
    type Error = Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let mut caps = vec![];

        for s in value.split(',') {
            if let Ok(cap) = Capability::try_from(s) {
                caps.push(cap);
            };
        }

        Ok(Capabilities(sanitize_caps(caps)))
    }
}

/// Allow `Capabilities::from(&url)` using the default `caps` key.
impl From<&Url> for Capabilities {
    fn from(url: &Url) -> Self {
        Capabilities::from_url(url)
    }
}

/// Allow `Capabilities::from(url)` (by value) using the default `caps` key.
impl From<Url> for Capabilities {
    fn from(url: Url) -> Self {
        Capabilities::from_url(&url)
    }
}

impl Display for Capabilities {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let string = self
            .0
            .iter()
            .map(|c| c.to_string())
            .collect::<Vec<_>>()
            .join(",");

        write!(f, "{string}")
    }
}

impl Serialize for Capabilities {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_string().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Capabilities {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let string: String = Deserialize::deserialize(deserializer)?;

        let mut caps = vec![];

        for s in string.split(',') {
            if let Ok(cap) = Capability::try_from(s) {
                caps.push(cap);
            };
        }

        Ok(Capabilities(sanitize_caps(caps)))
    }
}

// --- helpers ---

fn normalize_scope(mut s: String) -> String {
    if !s.starts_with('/') {
        s.insert(0, '/');
    }
    s
}

fn scope_covers(parent: &str, child: &str) -> bool {
    if parent == child {
        return true;
    }

    if !parent.ends_with('/') {
        return false;
    }

    child.starts_with(parent)
}

fn sanitize_caps(caps: Vec<Capability>) -> Vec<Capability> {
    let mut merged: Vec<Capability> = Vec::new();

    for mut cap in caps {
        if let Some(existing) = merged
            .iter_mut()
            .find(|existing| existing.scope == cap.scope)
        {
            let actions: BTreeSet<Action> = existing
                .actions
                .iter()
                .copied()
                .chain(cap.actions.iter().copied())
                .collect();
            existing.actions = actions.into_iter().collect();
            continue;
        }

        let actions: BTreeSet<Action> = cap.actions.iter().copied().collect();
        cap.actions = actions.into_iter().collect();
        merged.push(cap);
    }

    let mut sanitized: Vec<Capability> = Vec::new();

    'outer: for cap in merged.into_iter() {
        if sanitized.iter().any(|existing| existing.covers(&cap)) {
            continue 'outer;
        }

        sanitized.retain(|existing| !cap.covers(existing));
        sanitized.push(cap);
    }

    sanitized
}

#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;

    #[test]
    fn pubky_caps() {
        let cap = Capability {
            scope: "/pub/pubky.app/".to_string(),
            actions: vec![Action::Read, Action::Write],
        };

        // Read and write within directory `/pub/pubky.app/`.
        let expected_string = "/pub/pubky.app/:rw";

        assert_eq!(cap.to_string(), expected_string);

        assert_eq!(Capability::try_from(expected_string), Ok(cap))
    }

    #[test]
    fn root_capability_helper() {
        let cap = Capability::root();
        assert_eq!(cap.scope, "/");
        assert_eq!(cap.actions, vec![Action::Read, Action::Write]);
        assert_eq!(cap.to_string(), "/:rw");
        // And it round-trips through the string form:
        assert_eq!(Capability::try_from("/:rw"), Ok(cap));
    }

    #[test]
    fn single_capability_via_builder_and_shortcuts() {
        // Full builder:
        let cap1 = Capability::builder("/pub/my-cool-app/").read().write().finish();
        assert_eq!(cap1.to_string(), "/pub/my-cool-app/:rw");

        // Shortcuts:
        let cap_rw = Capability::read_write("/pub/my-cool-app/");
        let cap_r = Capability::read("/pub/file.txt");
        let cap_w = Capability::write("/pub/uploads/");

        assert_eq!(cap_rw, cap1);
        assert_eq!(cap_r.to_string(), "/pub/file.txt:r");
        assert_eq!(cap_w.to_string(), "/pub/uploads/:w");
    }

    #[test]
    fn multiple_caps_with_capsbuilder() {
        let caps = Capabilities::builder()
            .read("/pub/my-cool-app/") // "/pub/my-cool-app/:r"
            .write("/pub/uploads/") // "/pub/uploads/:w"
            .read_write("/pub/my-cool-app/data/") // "/pub/my-cool-app/data/:rw"
            .finish();

        // String form is comma-separated, in insertion order:
        assert_eq!(
            caps.to_string(),
            "/pub/my-cool-app/:r,/pub/uploads/:w,/pub/my-cool-app/data/:rw"
        );

        // Contains checks:
        assert!(caps.contains(&Capability::read("/pub/my-cool-app/")));
        assert!(caps.contains(&Capability::write("/pub/uploads/")));
        assert!(caps.contains(&Capability::read_write("/pub/my-cool-app/data/")));
        assert!(!caps.contains(&Capability::write("/nope")));
    }

    #[test]
    fn build_with_inline_capability_closure() {
        // Build a capability inline with fine-grained control, then push it:
        let caps = Capabilities::builder()
            .capability("/pub/my-cool-app/", |c| c.read().write())
            .finish();

        assert_eq!(caps.to_string(), "/pub/my-cool-app/:rw");
    }

    #[test]
    fn action_dedup_and_order_are_stable() {
        // Insert actions in noisy order; builder dedups & sorts (Read < Write).
        let cap = Capability::builder("/")
            .write()
            .read()
            .read()
            .write()
            .finish();
        assert_eq!(cap.actions, vec![Action::Read, Action::Write]);
        assert_eq!(cap.to_string(), "/:rw");
    }

    #[test]
    fn normalize_scope_adds_leading_slash() {
        // No leading slash? The helpers normalize it.
        let cap = Capability::read("pub/my.app");
        assert_eq!(cap.scope, "/pub/my.app");
        assert_eq!(cap.to_string(), "/pub/my.app:r");

        // CapsBuilder helpers also normalize:
        let caps = Capabilities::builder()
            .read_write("pub/my-cool-app/data")
            .finish();
        assert_eq!(caps.to_string(), "/pub/my-cool-app/data:rw");
    }

    #[test]
    fn parse_from_string_list() {
        // From a comma-separated string:
        let parsed = Capabilities::try_from("/:rw,/pub/my-cool-app/:r").unwrap();
        let built = Capabilities::builder()
            .read_write("/") // "/:rw"
            .read("/pub/my-cool-app/") // "/pub/my-cool-app/:r"
            .finish();

        assert_eq!(parsed, built);
    }

    #[test]
    fn parse_errors_are_informative() {
        // Invalid scope (doesn't start with '/'):
        let e = Capability::try_from("not/abs:rw").unwrap_err();
        assert!(matches!(e, Error::InvalidScope));

        // Invalid format (missing ':'):
        let e = Capability::try_from("/pub/my.app").unwrap_err();
        assert!(matches!(e, Error::InvalidFormat));

        // Invalid action:
        let e = Capability::try_from("/pub/my.app:rx").unwrap_err();
        assert!(matches!(e, Error::InvalidAction));
    }

    #[test]
    fn redundant_capabilities_builder_dedup() {
        let caps = Capabilities::builder()
            .read_write("/pub/example.com/")
            .read_write("/pub/example.com/")
            .write("/pub/example.com/subfolder")
            .finish();

        assert_eq!(caps.to_string(), "/pub/example.com/:rw");
    }

    #[test]
    fn redundant_capabilities_string_dedup() {
        let parsed = Capabilities::try_from(
            "/pub/example.com/:rw,/pub/example.com/:rw,/pub/example.com/subfolder:w",
        )
        .unwrap();

        let caps = Capabilities::builder()
            .read_write("/pub/example.com/")
            .finish();

        assert_eq!(caps.to_string(), "/pub/example.com/:rw");
        assert_eq!(parsed, caps);
    }

    #[test]
    fn redundant_capabilities_from_url_dedup() {
        let url = Url::parse(
            "https://example.test?caps=/pub/example.com/:rw,/pub/example.com/documents:w",
        )
        .unwrap();
        let caps = Capabilities::from_url(&url);

        assert_eq!(caps.to_string(), "/pub/example.com/:rw");
    }

    #[test]
    fn redundant_capabilities_merge_actions() {
        let caps = Capabilities::builder()
            .read("/pub/example.com/")
            .write("/pub/example.com/")
            .finish();

        assert_eq!(caps.to_string(), "/pub/example.com/:rw");
    }

    #[test]
    fn capabilities_len_and_is_empty() {
        let empty = Capabilities::builder().finish();
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);

        let one = Capabilities::builder().read("/").finish();
        assert!(!one.is_empty());
        assert_eq!(one.len(), 1);
    }

    // Requires dev-dependency: serde_json
    #[test]
    fn serde_roundtrip_as_string() {
        let caps = Capabilities::builder()
            .read_write("/pub/my-cool-app/")
            .read("/pub/file.txt")
            .finish();

        let json = serde_json::to_string(&caps).unwrap();
        // Serialized as a single string:
        assert_eq!(json, "\"/pub/my-cool-app/:rw,/pub/file.txt:r\"");

        let back: Capabilities = serde_json::from_str(&json).unwrap();
        assert_eq!(back, caps);
    }
}
