use std::{collections::HashMap, ops::Deref};

use axum::{
    async_trait,
    extract::{FromRequestParts, Path, Query},
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    RequestPartsExt,
};

use pkarr::PublicKey;

use crate::core::error::{Error, Result};

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
        let headers_to_check = ["host", "pkarr-host"];

        for header in headers_to_check {
            if let Some(Ok(pkarr_host)) = parts.headers.get(header).map(|h| h.to_str()) {
                if let Ok(public_key) = PublicKey::try_from(pkarr_host) {
                    tracing::debug!(?pkarr_host);

                    return Ok(Pubky(public_key));
                }
            }
        }

        // TODO: return an error and remove all param based routes

        let params: Path<HashMap<String, String>> =
            parts.extract().await.map_err(IntoResponse::into_response)?;

        let pubky_id = params
            .get("pubky")
            .ok_or_else(|| (StatusCode::NOT_FOUND, "pubky param missing").into_response())?;

        let public_key = PublicKey::try_from(pubky_id.to_string())
            .map_err(Error::try_from)
            .map_err(IntoResponse::into_response)?;

        // TODO: return 404 if the user doesn't exist, but exclude signups.

        Ok(Pubky(public_key))
    }
}

#[derive(Debug)]
pub struct EntryPath(pub(crate) String);

impl EntryPath {
    pub fn as_str(&self) -> &str {
        self.as_ref()
    }
}

impl std::fmt::Display for EntryPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Deref for EntryPath {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[async_trait]
impl<S> FromRequestParts<S> for EntryPath
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let params: Path<HashMap<String, String>> =
            parts.extract().await.map_err(IntoResponse::into_response)?;

        // TODO: enforce path limits like no trailing '/'

        let path = params
            .get("path")
            .ok_or_else(|| (StatusCode::NOT_FOUND, "entry path missing").into_response())?;

        if parts.uri.to_string().starts_with("/pub/") {
            Ok(EntryPath(format!("pub/{}", path)))
        } else {
            Ok(EntryPath(path.to_string()))
        }
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
