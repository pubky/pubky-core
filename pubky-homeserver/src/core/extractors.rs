use std::collections::HashMap;

use axum::{
    async_trait,
    extract::{FromRequestParts, Query},
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    RequestPartsExt,
};

use pkarr::PublicKey;

use crate::core::error::Result;

#[derive(Debug, Clone)]
pub struct PubkyHost(pub(crate) PublicKey);

impl PubkyHost {
    pub fn public_key(&self) -> &PublicKey {
        &self.0
    }
}

#[async_trait]
impl<S> FromRequestParts<S> for PubkyHost
where
    S: Sync + Send,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let pubky_host = parts
            .extensions
            .get::<PubkyHost>()
            .cloned()
            .ok_or((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Can't extract PubkyHost. Is `PubkyHostLayer` enabled?",
            ))
            .map_err(|e| e.into_response())?;

        Ok(pubky_host)
    }
}

#[derive(Debug)]
pub struct ListQueryParams {
    pub limit: Option<u16>,
    pub cursor: Option<String>,
    pub reverse: bool,
    pub shallow: bool,
}

#[async_trait]
impl<S> FromRequestParts<S> for ListQueryParams
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let params: Query<HashMap<String, String>> =
            parts.extract().await.map_err(IntoResponse::into_response)?;

        let reverse = params.contains_key("reverse");
        let shallow = params.contains_key("shallow");
        let limit = params
            .get("limit")
            // Treat `limit=` as None
            .and_then(|l| if l.is_empty() { None } else { Some(l) })
            .and_then(|l| l.parse::<u16>().ok());
        let cursor = params
            .get("cursor")
            .map(|c| c.as_str())
            // Treat `cursor=` as None
            .and_then(|c| {
                if c.is_empty() {
                    None
                } else {
                    Some(c.to_string())
                }
            });

        Ok(ListQueryParams {
            reverse,
            shallow,
            limit,
            cursor,
        })
    }
}
