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
    pub limit: Option<u16>,
    pub reverse: bool,
    /// If true, enter live streaming mode after historical events.
    /// If false, close connection after historical events are exhausted.
    pub live: bool,
    /// Vec of (user_pubkey, optional_cursor) pairs
    /// Format: "user=pubkey" or "user=pubkey:cursor"
    pub user_cursors: Vec<(String, Option<String>)>,
    /// Optional path prefix filter
    /// Format: Path WITHOUT `pubky://` scheme or user pubkey (e.g., "/pub/files/" or "/pub/")
    ///   - The prefix must start with "/" and is matched against the WebDAV path stored in the database
    ///   - Example: `path=/pub/` will only return events under the `/pub/` directory
    ///   - Example: `path=/pub/files/` will only return events under the `/pub/files/` directory
    pub path: Option<String>,
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
        let mut user_values: Vec<String> = Vec::new();

        for (key, value) in form_urlencoded::parse(query.as_bytes()) {
            let key_str = key.as_ref();
            if key_str == "user" {
                // "user" can be repeated multiple times
                if !value.is_empty() {
                    user_values.push(value.into_owned());
                }
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

        let live = if let Some(live_str) = single_params.get("live") {
            parse_bool(live_str).map_err(|e| *e)?
        } else {
            false
        };

        let limit = match single_params.get("limit") {
            Some(l) if !l.is_empty() => l.parse::<u16>().ok(),
            _ => None,
        };

        let path =
            single_params.get("path").and_then(
                |p| {
                    if p.is_empty() {
                        None
                    } else {
                        Some(p.clone())
                    }
                },
            );

        // Parse user values into (pubkey, optional_cursor) pairs
        // Format: "pubkey" or "pubkey:cursor"
        let user_cursors = user_values
            .into_iter()
            .map(|value| {
                if let Some((pubkey, cursor)) = value.split_once(':') {
                    (pubkey.to_string(), Some(cursor.to_string()))
                } else {
                    (value, None)
                }
            })
            .collect::<Vec<(String, Option<String>)>>();

        Ok(EventStreamQueryParams {
            limit,
            reverse,
            live,
            user_cursors,
            path,
        })
    }
}
