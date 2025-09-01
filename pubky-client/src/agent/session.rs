use pubky_common::session::Session;
use reqwest::{Method, StatusCode};

use super::core::PubkyAgent;
use crate::{errors::Result, util::check_http_status};

impl PubkyAgent {
    /// Retrieve session for current pubky from homeserver. Fails if pubky is unknown.
    pub async fn session_from_homeserver(&self) -> Result<Option<Session>> {
        let response = self
            .drive()
            .request(Method::GET, "/session")
            .await?
            .send()
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let response = check_http_status(response).await?;
        let bytes = response.bytes().await?;
        Ok(Some(Session::deserialize(&bytes)?))
    }

    /// Sign out and invalidate this agent’s server-side session. Consumes the agent.
    pub async fn signout(self) -> Result<()> {
        let response = self.drive().delete("/session").await?;
        check_http_status(response).await?;
        Ok(())
    }
}
