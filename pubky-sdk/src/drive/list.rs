use reqwest::Method;
use url::Url;

use super::core::PubkyStorage;
use super::resource::IntoPubkyResource;

use crate::Result;
use crate::util::check_http_status;

impl PubkyStorage {
    /// Directory listing helper.
    ///
    /// The homeserver default limit is 100. The max list limit is 1000.
    ///
    /// # Examples
    /// ```no_run
    /// # async fn example(drive: pubky::PubkyStorage) -> pubky::Result<()> {
    /// let urls = drive.list("/pub/app/")?.limit(100).shallow(true).send().await?;
    /// for u in urls { println!("{u}"); }
    /// # Ok(()) }
    /// ```
    pub fn list<P: IntoPubkyResource>(&self, path: P) -> Result<ListBuilder<'_>> {
        Ok(ListBuilder {
            drive: self,
            url: self.to_url(path)?,
            reverse: false,
            shallow: false,
            limit: None,
            cursor: None,
        })
    }
}

/// Builder for homeserver `LIST` queries.
///
/// Configure optional flags like `reverse`, `shallow`, `limit`, and `cursor`,
/// then call [`send`](Self::send) to perform the request.
///
/// Returned entries are absolute `Url`s.
///
/// See [`PubkyStorage::list`] for examples.
#[derive(Debug)]
#[must_use]
pub struct ListBuilder<'a> {
    drive: &'a PubkyStorage,
    url: Url,
    reverse: bool,
    shallow: bool,
    limit: Option<u16>,
    cursor: Option<String>,
}

impl<'a> ListBuilder<'a> {
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
    ///
    /// Directory semantics are enforced: if the path didn’t end with `/`, it will be normalized.
    pub async fn send(self) -> Result<Vec<Url>> {
        // Resolve now (absolute stays absolute, relative is based on agent’s homeserver)
        let mut url = self.url;

        // Ensure directory semantics using URL segments (drop last segment, keep trailing slash)
        if !url.path().ends_with('/') {
            {
                let mut segs = url
                    .path_segments_mut()
                    .map_err(|_| url::ParseError::RelativeUrlWithCannotBeABaseBase)?;
                segs.pop_if_empty(); // remove possible trailing empty
                segs.pop(); // drop last non-empty segment
                segs.push(""); // ensure trailing slash
            }
        }

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

        // Build the request without re-parsing the URL back through IntoPubkyResource
        let rb = self
            .drive
            .client
            .cross_request(Method::GET, url.clone())
            .await?;
        // Attach cookie only when hitting this agent’s homeserver (native)
        #[cfg(not(target_arch = "wasm32"))]
        let rb = self.drive.maybe_attach_session_cookie(&url, rb);

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
