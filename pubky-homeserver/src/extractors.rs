use std::collections::HashMap;

use axum::{
    async_trait,
    extract::{FromRequestParts, Path},
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    RequestPartsExt,
};

use pkarr::PublicKey;

use crate::error::{Error, Result};

#[derive(Debug)]
pub struct Pubky(PublicKey);

impl Pubky {
    pub fn public_key(&self) -> &PublicKey {
        &self.0
    }
}

#[async_trait]
impl<S> FromRequestParts<S> for Pubky
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let params: Path<HashMap<String, String>> =
            parts.extract().await.map_err(IntoResponse::into_response)?;

        let pubky_id = params
            .get("pubky")
            .ok_or_else(|| (StatusCode::NOT_FOUND, "pubky param missing").into_response())?;

        let public_key = PublicKey::try_from(pubky_id.to_string())
            .map_err(Error::try_from)
            .map_err(IntoResponse::into_response)?;

        Ok(Pubky(public_key))
    }
}
