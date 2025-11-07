//! Event stream actor for subscribing to single-user event feeds.
//!
//! This module provides a builder-style API for subscribing to Server-Sent Events (SSE)
//! from a user's homeserver `/events-stream` endpoint.
//!
//! # Example
//! ```no_run
//! use pubky::{Pubky, PublicKey};
//! use futures_util::StreamExt;
//!
//! # async fn example() -> pubky::Result<()> {
//! let pubky = Pubky::new()?;
//! let user = PublicKey::try_from("o1gg96ewuojmopcjbz8895478wdtxtzzuxnfjjz8o8e77csa1ngo")?;
//!
//! let mut stream = pubky.event_stream_for(&user)
//!     .live(true)
//!     .limit(100)
//!     .path("/pub/")
//!     .subscribe()
//!     .await?;
//!
//! while let Some(result) = stream.next().await {
//!     let event = result?;
//!     println!("Event: {:?} at {}", event.event_type, event.path);
//! }
//! # Ok(())
//! # }
//! ```

use std::pin::Pin;

use eventsource_stream::Eventsource;
use futures_util::{Stream, StreamExt};
use pkarr::PublicKey;
use reqwest::Method;
use url::Url;

use crate::{
    Pkdns, PubkyHttpClient, cross_log,
    errors::{Error, RequestError, Result},
};

/// Type of event in the event stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventType {
    /// PUT event - resource created or updated.
    Put,
    /// DELETE event - resource deleted.
    Delete,
}

impl std::fmt::Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EventType::Put => write!(f, "PUT"),
            EventType::Delete => write!(f, "DEL"),
        }
    }
}

/// A single event from the event stream.
#[derive(Debug, Clone)]
pub struct Event {
    /// Type of event (PUT or DEL).
    pub event_type: EventType,
    /// Full pubky path (e.g., "pubky://user_pubkey/pub/example.txt").
    pub path: String,
    /// Cursor for pagination (event id as string).
    pub cursor: String,
    /// Content hash (blake3) in hex format (only for PUT events with available hash).
    pub content_hash: Option<String>,
}

/// Builder for creating an event stream subscription.
///
/// Construct via [`crate::Pubky::event_stream_for`].
#[derive(Clone, Debug)]
pub struct EventStreamBuilder {
    client: PubkyHttpClient,
    user: PublicKey,
    limit: Option<u16>,
    live: bool,
    reverse: bool,
    path: Option<String>,
    cursor: Option<String>,
}

impl EventStreamBuilder {
    /// Create a new event stream builder for a specific user.
    ///
    /// Typically called via [`crate::Pubky::event_stream_for`].
    #[must_use]
    pub fn new(client: PubkyHttpClient, user: PublicKey) -> Self {
        Self {
            client,
            user,
            limit: None,
            live: false,
            reverse: false,
            path: None,
            cursor: None,
        }
    }

    /// Set maximum number of events to receive before closing the connection.
    ///
    /// If omitted:
    /// - With `live=false`: sends all historical events, then closes
    /// - With `live=true`: sends all historical events, then enters live mode (infinite stream)
    #[must_use]
    pub const fn limit(mut self, limit: u16) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Enable live streaming mode.
    ///
    /// - `false` (default): Fetch historical events and close connection (batch mode)
    /// - `true`: Fetch historical events, then stream new events in real-time
    ///
    /// **Note**: Cannot be combined with `reverse(true)`.
    #[must_use]
    pub const fn live(mut self, live: bool) -> Self {
        self.live = live;
        self
    }

    /// Return events in reverse chronological order (newest first).
    ///
    /// Default: `false` (oldest first).
    ///
    /// **Note**: Cannot be combined with `live(true)`.
    #[must_use]
    pub const fn reverse(mut self, reverse: bool) -> Self {
        self.reverse = reverse;
        self
    }

    /// Filter events by path prefix.
    ///
    /// Format: Path WITHOUT `pubky://` scheme or user pubkey (e.g., "/pub/files/" or "/pub/").
    /// Only events whose path starts with this prefix are returned.
    #[must_use]
    pub fn path<S: Into<String>>(mut self, path: S) -> Self {
        self.path = Some(path.into());
        self
    }

    /// Set the starting cursor position.
    ///
    /// Format: Event ID as string (e.g., "42") or user_pubkey:cursor (e.g., "pubkey:42").
    /// Events after this cursor will be returned.
    #[must_use]
    pub fn cursor<S: Into<String>>(mut self, cursor: S) -> Self {
        self.cursor = Some(cursor.into());
        self
    }

    /// Subscribe to the event stream.
    ///
    /// This performs the following steps:
    /// 1. Resolves the user's homeserver via DHT/PKDNS
    /// 2. Constructs the `/events-stream` URL with query parameters
    /// 3. Makes the HTTP request
    /// 4. Returns a stream of parsed events
    ///
    /// # Errors
    /// - Returns [`Error::Request`] if the homeserver cannot be resolved
    /// - Returns [`Error::Request`] if `live=true` and `reverse=true` (invalid combination)
    /// - Propagates HTTP request errors
    pub async fn subscribe(self) -> Result<Pin<Box<dyn Stream<Item = Result<Event>> + Send>>> {
        // Validate parameters
        if self.live && self.reverse {
            return Err(Error::from(RequestError::Validation {
                message: "Cannot use live mode with reverse ordering".into(),
            }));
        }

        // Resolve homeserver
        let homeserver = Pkdns::with_client(self.client.clone())
            .get_homeserver_of(&self.user)
            .await
            .ok_or_else(|| {
                Error::from(RequestError::Validation {
                    message: format!("Could not resolve homeserver for user {}", self.user),
                })
            })?;

        cross_log!(
            info,
            "Subscribing to event stream for user {} on homeserver {}",
            self.user,
            homeserver
        );

        // Build URL with query parameters
        let mut url = Url::parse(&format!("https://{}/events-stream", homeserver))?;

        {
            let mut query = url.query_pairs_mut();

            // Add user parameter with optional cursor
            if let Some(cursor) = &self.cursor {
                query.append_pair("user", &format!("{}:{}", self.user, cursor));
            } else {
                query.append_pair("user", &self.user.to_string());
            }

            if let Some(limit) = self.limit {
                query.append_pair("limit", &limit.to_string());
            }

            if self.live {
                query.append_pair("live", "true");
            }

            if self.reverse {
                query.append_pair("reverse", "true");
            }

            if let Some(path) = &self.path {
                query.append_pair("path", path);
            }
        }

        cross_log!(debug, "Event stream URL: {}", url);

        // Make request
        let response = self
            .client
            .cross_request(Method::GET, url)
            .await?
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let message = format!("Event stream request failed with status {}", status);
            return Err(Error::from(RequestError::Server { status, message }));
        }

        // Create SSE stream
        let sse_stream = response.bytes_stream().eventsource();

        // Map SSE events to our Event type
        let event_stream = sse_stream.filter_map(|result| async move {
            match result {
                Ok(sse_event) => {
                    // Parse the SSE event
                    match parse_sse_event(&sse_event) {
                        Ok(event) => Some(Ok(event)),
                        Err(e) => {
                            cross_log!(warn, "Failed to parse SSE event: {}", e);
                            Some(Err(e))
                        }
                    }
                }
                Err(e) => {
                    cross_log!(error, "SSE stream error: {}", e);
                    Some(Err(Error::from(RequestError::Validation {
                        message: format!("SSE stream error: {}", e),
                    })))
                }
            }
        });

        Ok(Box::pin(event_stream))
    }
}

/// Parse a Server-Sent Event into our Event type.
///
/// SSE format:
/// ```text
/// event: PUT
/// data: pubky://user_pubkey/pub/example.txt
/// data: cursor: 42
/// data: content_hash: abc123... (optional)
/// ```
fn parse_sse_event(sse: &eventsource_stream::Event) -> Result<Event> {
    let event_type = match sse.event.as_str() {
        "PUT" => EventType::Put,
        "DEL" => EventType::Delete,
        _ => {
            return Err(Error::from(RequestError::Validation {
                message: format!("Unknown event type: {}", sse.event),
            }));
        }
    };

    let lines: Vec<&str> = sse.data.lines().collect();
    if lines.is_empty() {
        return Err(Error::from(RequestError::Validation {
            message: "SSE event data is empty".into(),
        }));
    }

    // First line is the path
    let path = lines[0].to_string();

    // Second line should be the cursor
    let cursor = lines
        .get(1)
        .and_then(|line| line.strip_prefix("cursor: "))
        .ok_or_else(|| {
            Error::from(RequestError::Validation {
                message: "SSE event missing cursor line".into(),
            })
        })?
        .to_string();

    // Third line (optional) is the content_hash
    let content_hash = lines
        .get(2)
        .and_then(|line| line.strip_prefix("content_hash: "))
        .map(ToString::to_string);

    Ok(Event {
        event_type,
        path,
        cursor,
        content_hash,
    })
}
