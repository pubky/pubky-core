use pkarr::PublicKey;
use reqwest::{Method, RequestBuilder};
use url::Url;

use super::resource::{IntoPubkyResource, IntoResourcePath, PubkyResource, ResourcePath};
use crate::{
    PubkyHttpClient, PubkySession,
    errors::{RequestError, Result},
    global::global_client,
};

/// Storage that acts **as the signed-in user** (authenticated).
///
/// Accepts **absolute paths** (`ResourcePath`) only; the user is implied by the session.
/// Writes are allowed.
///
/// Returned by [`PubkySession::storage()`].
#[derive(Debug, Clone)]
pub struct SessionStorage {
    pub(crate) client: PubkyHttpClient,
    pub(crate) user: PublicKey,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) cookie: String,
}

impl SessionStorage {
    /// Construct from an existing session.
    ///
    /// Equivalent to `session.storage()`.
    pub fn new(session: &PubkySession) -> SessionStorage {
        SessionStorage {
            client: session.client.clone(),
            user: session.info.public_key().clone(),
            #[cfg(not(target_arch = "wasm32"))]
            cookie: session.cookie.clone(),
        }
    }

    /// Convenience: unauthenticated public reader using the same client.
    pub fn public(&self) -> PublicStorage {
        PublicStorage {
            client: self.client.clone(),
        }
    }

    /// Resolve an **absolute** path into a concrete `pubky://…` URL for this session’s user.
    pub(crate) fn to_url<P: IntoResourcePath>(&self, p: P) -> Result<Url> {
        let path: ResourcePath = p.into_abs_path()?;
        let addr = PubkyResource::new(self.user.clone(), path.as_str())?;
        let url_str = addr.to_pubky_url();
        Ok(Url::parse(&url_str)?)
    }

    /// Build a request for this storage.
    ///
    /// - Paths are **absolute** (session-scoped).
    /// - On native targets, the session cookie is attached **always** as the URL points
    ///   to this user’s homeserver (cookies never leak across users).
    pub(crate) async fn request<P: IntoResourcePath>(
        &self,
        method: Method,
        path: P,
    ) -> Result<RequestBuilder> {
        let url = self.to_url(path)?;
        let rb = self.client.cross_request(method, url).await?;

        // Always attach session cookie on native; this handle is scoped to *my* user.
        #[cfg(not(target_arch = "wasm32"))]
        let rb = {
            let cookie_name = self.user.to_string();
            rb.header(
                reqwest::header::COOKIE,
                format!("{cookie_name}={}", self.cookie),
            )
        };

        Ok(rb)
    }
}

/// Storage that reads **public data for any user** (unauthenticated).
///
/// Accepts **addressed resources** (`PubkyResource`: user + absolute path).
/// Writes are not available.
#[derive(Debug, Clone)]
pub struct PublicStorage {
    pub(crate) client: PubkyHttpClient,
}

impl PublicStorage {
    /// Create a public (unauthenticated) storage handle using the global client.
    pub fn new() -> Result<PublicStorage> {
        Ok(PublicStorage {
            client: global_client()?,
        })
    }

    /// Resolve an addressed resource into a concrete `pubky://…` URL.
    pub(crate) fn to_url<A: IntoPubkyResource>(&self, addr: A) -> Result<Url> {
        let addr: PubkyResource = addr.into_pubky_resource()?;
        let url_str = addr.to_pubky_url();
        Ok(Url::parse(&url_str)?)
    }

    /// Build a request for this public storage (no cookies).
    pub(crate) async fn request<A: IntoPubkyResource>(
        &self,
        method: Method,
        addr: A,
    ) -> Result<RequestBuilder> {
        let url = self.to_url(addr)?;
        let rb = self.client.cross_request(method, url).await?;
        Ok(rb)
    }
}

/// Helper: validation error for directory listings without trailing slash.
#[inline]
pub(crate) fn dir_trailing_slash_error() -> RequestError {
    RequestError::Validation {
        message: "directory listings must end with `/`".into(),
    }
}
