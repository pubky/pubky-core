use std::{collections::HashMap, fmt::Display};

use axum::{
    extract::{FromRequestParts, Query},
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    RequestPartsExt,
};

use pkarr::PublicKey;

#[derive(Debug, Clone)]
pub struct PubkyHost(pub(crate) PublicKey);

impl PubkyHost {
    pub fn public_key(&self) -> &PublicKey {
        &self.0
    }
}

impl Display for PubkyHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

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

#[derive(Debug, Clone)]
pub struct ListQueryParams {
    pub limit: Option<u16>,
    pub cursor: Option<String>,
    pub shallow: bool,
}

impl ListQueryParams {
    /// Extracts the cursor from the query parameters.
    /// If the cursor is not a valid EntryPath, returns None.
    /// If the cursor is empty, returns None.
    pub fn extract_cursor(params: &Query<HashMap<String, String>>) -> Option<String> {
        let value = params.get("cursor")?;
        if value.is_empty() {
            // Treat `cursor=` as None
            return None;
        }

        let mut value = value.as_str();
        if let Some(stripped_value) = value.strip_prefix("pubky://") {
            value = stripped_value;
        }
        Some(value.to_string())
    }
}

impl<S> FromRequestParts<S> for ListQueryParams
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let params: Query<HashMap<String, String>> =
            parts.extract().await.map_err(IntoResponse::into_response)?;

        let shallow = params.contains_key("shallow");
        let limit = params
            .get("limit")
            // Treat `limit=` as None
            .and_then(|l| if l.is_empty() { None } else { Some(l) })
            .and_then(|l| l.parse::<u16>().ok());
        let cursor = Self::extract_cursor(&params);

        Ok(ListQueryParams {
            shallow,
            limit,
            cursor,
        })
    }
}
