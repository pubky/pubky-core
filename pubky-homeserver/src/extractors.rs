use std::collections::HashMap;

use axum::{
    async_trait,
    extract::{FromRequestParts, Path},
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    RequestPartsExt,
};

use pkarr::PublicKey;

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
            .ok_or_else(|| (StatusCode::NOT_FOUND, "Pubky not found").into_response())?;

        let public_key = PublicKey::try_from(pubky_id.to_string())
            // TODO: convert Pkarr errors to app errors, in this case a params validation error
            .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid Pubky").into_response())?;

        Ok(Pubky(public_key))
    }
}
