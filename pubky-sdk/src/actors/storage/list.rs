use reqwest::Method;
use url::Url;

use super::core::{PublicStorage, SessionStorage, dir_trailing_slash_error};
use crate::Result;
use crate::actors::storage::resource::{IntoPubkyResource, IntoResourcePath};
use crate::util::check_http_status;

impl SessionStorage {
    /// Directory listing **as me** (authenticated).
    ///
    /// Requirements:
    /// - Path **must** point to a directory and **must end with `/`**.
    ///
    /// Returns absolute entry URLs.
    ///
    /// # Example
    /// ```no_run
    /// # async fn example(session: pubky::PubkySession) -> pubky::Result<()> {
    /// let urls = session
    ///     .storage()
    ///     .list("/pub/my.app/")?
    ///     .limit(100)
    ///     .shallow(true)
    ///     .send()
    ///     .await?;
    /// for u in urls { println!("{u}"); }
    /// # Ok(()) }
    /// ```
    pub fn list<P: IntoResourcePath>(&self, path: P) -> Result<ListBuilder<'_>> {
        let url = self.to_url(path)?;
        if !url.path().ends_with('/') {
            return Err(dir_trailing_slash_error().into());
        }
        Ok(ListBuilder::session(self, url))
    }
}

impl PublicStorage {
    /// Directory listing **public** (unauthenticated).
    ///
    /// Requirements:
    /// - Address **must** point to a directory and **must end with `/`**.
    ///
    /// Returns absolute entry URLs.
    pub fn list<A: IntoPubkyResource>(&self, addr: A) -> Result<ListBuilder<'_>> {
        let url = self.to_url(addr)?;
        if !url.path().ends_with('/') {
            return Err(dir_trailing_slash_error().into());
        }
        Ok(ListBuilder::public(self, url))
    }
}

/// Internal scope for a listing request.
#[derive(Debug)]
enum ListScope<'a> {
    Session(&'a SessionStorage),
    Public(&'a PublicStorage),
}

/// Unified builder for homeserver `LIST` queries (works for session & public).
///
/// Configure optional flags like `reverse`, `shallow`, `limit`, and `cursor`,
/// then call [`send`](Self::send) to perform the request.
///
/// Returned entries are absolute `Url`s.
///
/// Built via:
/// - [`SessionStorage::list`] for authenticated “as me” listings.
/// - [`PublicStorage::list`] for unauthenticated public listings.
#[derive(Debug)]
#[must_use]
pub struct ListBuilder<'a> {
    scope: ListScope<'a>,
    url: Url,
    reverse: bool,
    shallow: bool,
    limit: Option<u16>,
    cursor: Option<String>,
}

impl<'a> ListBuilder<'a> {
    #[inline]
    fn session(storage: &'a SessionStorage, url: Url) -> Self {
        Self {
            scope: ListScope::Session(storage),
            url,
            reverse: false,
            shallow: false,
            limit: None,
            cursor: None,
        }
    }

    #[inline]
    fn public(storage: &'a PublicStorage, url: Url) -> Self {
        Self {
            scope: ListScope::Public(storage),
            url,
            reverse: false,
            shallow: false,
            limit: None,
            cursor: None,
        }
    }

    /// List newest-first instead of oldest-first.
    pub fn reverse(mut self, reverse: bool) -> Self {
        self.reverse = reverse;
        self
    }

    /// Do not recurse into subdirectories.
    pub fn shallow(mut self, shallow: bool) -> Self {
        self.shallow = shallow;
        self
    }

    /// Maximum number of entries to return (homeserver may cap).
    pub fn limit(mut self, limit: u16) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Resume listing from a previous `cursor` token.
    pub fn cursor(mut self, cursor: &str) -> Self {
        self.cursor = Some(cursor.to_string());
        self
    }

    /// Execute the LIST request and return entry URLs.
    pub async fn send(self) -> Result<Vec<Url>> {
        // 1) Build query params
        let mut url = self.url;
        {
            let mut q = url.query_pairs_mut();
            if self.reverse {
                q.append_key_only("reverse");
            }
            if self.shallow {
                q.append_key_only("shallow");
            }
            if let Some(limit) = self.limit {
                q.append_pair("limit", &limit.to_string());
            }
            if let Some(cursor) = self.cursor {
                q.append_pair("cursor", &cursor);
            }
        }

        // 2) Build request per scope
        let rb = match self.scope {
            ListScope::Public(storage) => {
                storage
                    .client
                    .cross_request(Method::GET, url.clone())
                    .await?
            }
            ListScope::Session(storage) => {
                let rb = storage
                    .client
                    .cross_request(Method::GET, url.clone())
                    .await?;
                #[cfg(not(target_arch = "wasm32"))]
                let rb = storage.with_session_cookie(rb);
                rb
            }
        };

        // 3) Send and parse
        let resp = rb.send().await?;
        let resp = check_http_status(resp).await?;

        let bytes = resp.bytes().await?;
        let mut out = Vec::new();
        for line in String::from_utf8_lossy(&bytes).lines() {
            out.push(Url::parse(line)?);
        }
        Ok(out)
    }
}
