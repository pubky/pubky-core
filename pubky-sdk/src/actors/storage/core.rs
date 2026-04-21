use crate::PublicKey;
use crate::actors::session::credentials::SessionCredential;
use reqwest::{Method, RequestBuilder};
use std::sync::Arc;

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
    /// Cloned credential — sharing the same `Arc<dyn SessionCredential>` as
    /// the parent session is cheap and gives the storage layer access to the
    /// latest authentication material (with auto-refresh for JWT).
    pub(crate) credential: Arc<dyn SessionCredential>,
}

impl SessionStorage {
    /// Construct from an existing session.
    ///
    /// Equivalent to `session.storage()`.
    #[must_use]
    pub fn new(session: &PubkySession) -> Self {
        Self {
            client: session.client.clone(),
            user: session.info().public_key().clone(),
            credential: Arc::clone(session.credential()),
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
    /// - The session credential attaches the right authentication header
    ///   (cookie or bearer JWT) and refreshes the JWT proactively if needed.
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
        self.attach_credential(rb).await
    }

    /// Attach the session credential to a request builder.
    pub(crate) async fn attach_credential(&self, rb: RequestBuilder) -> Result<RequestBuilder> {
        self.credential.attach(rb, &self.client).await
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
