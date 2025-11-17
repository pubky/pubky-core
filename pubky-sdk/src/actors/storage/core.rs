use pkarr::PublicKey;
use reqwest::{Method, RequestBuilder};

use super::resource::{IntoPubkyResource, IntoResourcePath, PubkyResource, ResourcePath};
use crate::{
    PubkyHttpClient, PubkySession, cross_log,
    errors::{RequestError, Result},
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
    pub(crate) cookie: (String, String),
}

impl SessionStorage {
    /// Construct from an existing session.
    ///
    /// Equivalent to `session.storage()`.
    #[must_use]
    pub fn new(session: &PubkySession) -> Self {
        Self {
            client: session.client.clone(),
            user: session.info.public_key().clone(),
            #[cfg(not(target_arch = "wasm32"))]
            cookie: session.cookie.clone(),
        }
    }

    /// Convenience: unauthenticated public reader using the same client.
    #[must_use]
    pub fn public(&self) -> PublicStorage {
        PublicStorage {
            client: self.client.clone(),
        }
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
        let path: ResourcePath = path.into_abs_path()?;
        let resource = PubkyResource::new(self.user.clone(), path.as_str())?;
        let url = resource.to_transport_url()?;
        cross_log!(debug, "Session storage {} request {}", method, url);
        let rb = self.client.cross_request(method, url).await?;

        #[cfg(not(target_arch = "wasm32"))]
        let rb = self.with_session_cookie(rb);

        Ok(rb)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn with_session_cookie(&self, rb: RequestBuilder) -> RequestBuilder {
        let (cookie_name, cookie_value) = &self.cookie;
        rb.header(
            reqwest::header::COOKIE,
            format!("{cookie_name}={cookie_value}"),
        )
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
    /// Create a public (unauthenticated) storage handle using a new client.
    ///
    /// Tip: If you already have a `Pubky` facade, prefer `pubky.public_storage()`
    /// to reuse its underlying client and configuration.
    ///
    /// # Errors
    /// - Returns [`crate::errors::Error`] if the underlying [`PubkyHttpClient`] cannot be constructed.
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: PubkyHttpClient::new()?,
        })
    }

    /// Build a request for this public storage (no cookies).
    pub(crate) async fn request<A: IntoPubkyResource>(
        &self,
        method: Method,
        addr: A,
    ) -> Result<RequestBuilder> {
        let resource: PubkyResource = addr.into_pubky_resource()?;
        let url = resource.to_transport_url()?;
        cross_log!(debug, "Public storage {} request {}", method, url);
        let rb = self.client.cross_request(method, url).await?;
        Ok(rb)
    }
}

/// Helper: validation error for directory listings without trailing slash.
#[inline]
pub fn dir_trailing_slash_error() -> RequestError {
    RequestError::Validation {
        message: "directory listings must end with `/`".into(),
    }
}
