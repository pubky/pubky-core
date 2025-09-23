use reqwest::header::{CONTENT_LENGTH, CONTENT_TYPE, ETAG, HeaderMap, LAST_MODIFIED};
use std::time::SystemTime;

/// Typed metadata for a stored object (from a `HEAD` request).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceStats {
    /// `Content-Length` parsed as `u64`.
    pub content_length: Option<u64>,
    /// `Content-Type` as sent by the server (verbatim).
    pub content_type: Option<String>,
    /// `Last-Modified` parsed into `SystemTime` (RFC7231).
    pub last_modified: Option<SystemTime>,
    /// ETag string.
    pub etag: Option<String>,
}

impl ResourceStats {
    /// Build from response headers.
    pub fn from_headers(h: &HeaderMap) -> Self {
        let content_length = h
            .get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok());

        let content_type = h
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let last_modified = h
            .get(LAST_MODIFIED)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| httpdate::parse_http_date(s).ok());

        let etag = h.get(ETAG).and_then(|v| v.to_str().ok()).map(clean_etag);

        Self {
            content_length,
            content_type,
            last_modified,
            etag,
        }
    }
}

fn clean_etag(raw: &str) -> String {
    let s = raw.trim();

    // Weak: W/"abc" -> W/abc
    if s.starts_with("W/\"") && s.ends_with('"') && s.len() >= 4 {
        return format!("W/{}", &s[3..s.len() - 1]);
    }

    // Strong: "abc" -> abc
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        return s[1..s.len() - 1].to_string();
    }

    s.to_string()
}
