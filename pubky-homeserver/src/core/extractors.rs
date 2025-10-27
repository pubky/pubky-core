use std::{collections::HashMap, fmt::Display};

use axum::{
    body::Body,
    extract::{FromRequestParts, Query},
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    RequestPartsExt,
};
use url::form_urlencoded;

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
    pub reverse: bool,
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

#[derive(Debug, Clone)]
pub struct EventStreamQueryParams {
    pub cursor: Option<String>,
    pub limit: Option<u16>,
    pub reverse: bool,
    pub users: Vec<String>,
}

impl EventStreamQueryParams {}

/// Parse a boolean value from a string.
/// Returns an error if the value is not a valid boolean.
fn parse_bool(value: &str) -> Result<bool, Box<Response>> {
    match value.to_lowercase().as_str() {
        "true" => Ok(true),
        "yes" => Ok(true),
        "1" => Ok(true),
        "" => Ok(true),
        "false" => Ok(false),
        "no" => Ok(false),
        "0" => Ok(false),
        _ => Err(Box::new(
            Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Body::from("Invalid boolean parameter"))
                .unwrap(),
        )),
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

        let reverse = if let Some(reverse) = params.get("reverse") {
            parse_bool(reverse).map_err(|e| *e)?
        } else {
            false
        };

        let shallow = if let Some(shallow) = params.get("shallow") {
            parse_bool(shallow).map_err(|e| *e)?
        } else {
            false
        };

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
            reverse,
        })
    }
}

impl<S> FromRequestParts<S> for EventStreamQueryParams
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let query = parts.uri.query().unwrap_or("");
        let mut single_params: HashMap<String, String> = HashMap::new();
        let mut multi_params: HashMap<String, Vec<String>> = HashMap::new();

        for (key, value) in form_urlencoded::parse(query.as_bytes()) {
            let key_str = key.as_ref();
            if key_str == "user" {
                // "user" can be repeated multiple times
                multi_params
                    .entry(key.into_owned())
                    .or_default()
                    .push(value.into_owned());
            } else {
                // All other params are single-value (last one wins if repeated)
                single_params.insert(key.into_owned(), value.into_owned());
            }
        }

        let reverse = if let Some(reverse_str) = single_params.get("reverse") {
            parse_bool(reverse_str).map_err(|e| *e)?
        } else {
            false
        };

        let limit = single_params
            .get("limit")
            // Treat `limit=` as None
            .and_then(|l| if l.is_empty() { None } else { Some(l) })
            .and_then(|l| l.parse::<u16>().ok());

        let cursor = single_params
            .get("cursor")
            // Treat `cursor=` as None
            .filter(|value| !value.is_empty())
            .map(|value| value.to_string());

        let users = multi_params
            .get("user")
            .map(|users| {
                users
                    .iter()
                    .filter(|u| !u.is_empty())
                    .cloned()
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default();

        Ok(EventStreamQueryParams {
            limit,
            cursor,
            reverse,
            users,
        })
    }
}
