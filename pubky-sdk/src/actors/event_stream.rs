//! Event stream actor for subscribing to multi-user event feeds.
//!
//! This module provides a builder-style API for subscribing to Server-Sent Events (SSE)
//! from a user's homeserver `/events-stream` endpoint.
//!
//! IMPORTANT: Only the first User's pubky is used to identify the Homeserver which this code calls.
//! It is the responsibility of the caller to ensure that all Users added are on the same Homeserver.
//!
//! # Example
//! ```no_run
//! use pubky::{Pubky, PublicKey, EventCursor};
//! use futures_util::StreamExt;
//!
//! # async fn example() -> pubky::Result<()> {
//! let pubky = Pubky::new()?;
//! let user1 = PublicKey::try_from("o1gg96ewuojmopcjbz8895478wdtxtzzuxnfjjz8o8e77csa1ngo").unwrap();
//! let user2 = PublicKey::try_from("pxnu33x7jtpx9ar1ytsi4yxbp6a5o36gwhffs8zoxmbuptici1jy").unwrap();
//!
//! let mut stream = pubky.event_stream()
//!     .add_user(&user1, None)?
//!     .add_user(&user2, Some(EventCursor::new(100)))?
//!     .live()
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

use std::fmt::Display;
use std::pin::Pin;
use std::str::FromStr;

use eventsource_stream::Eventsource;
use futures_util::{Stream, StreamExt};
use pkarr::PublicKey;
use reqwest::Method;
use url::Url;

use crate::{
    Pkdns, PubkyHttpClient, cross_log,
    errors::{Error, RequestError, Result},
};

/// Cursor for pagination in event queries.
///
/// The cursor represents the ID of an event and is used for pagination.
/// It can be parsed from a string representation of an integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct EventCursor(i64);

impl EventCursor {
    /// Create a new cursor from an event ID.
    #[must_use]
    pub fn new(id: i64) -> Self {
        Self(id)
    }

    /// Get the underlying ID value.
    #[must_use]
    pub fn id(&self) -> i64 {
        self.0
    }
}

impl Display for EventCursor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for EventCursor {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(EventCursor(s.parse()?))
    }
}

impl From<i64> for EventCursor {
    fn from(id: i64) -> Self {
        EventCursor(id)
    }
}

impl TryFrom<&str> for EventCursor {
    type Error = std::num::ParseIntError;

    fn try_from(s: &str) -> std::result::Result<Self, Self::Error> {
        s.parse()
    }
}

impl TryFrom<String> for EventCursor {
    type Error = std::num::ParseIntError;

    fn try_from(s: String) -> std::result::Result<Self, Self::Error> {
        s.parse()
    }
}

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
    /// Full pubky path (e.g., `<pubky://user_pubkey/pub/example.txt>`).
    pub path: String,
    /// Cursor for pagination (event ID).
    pub cursor: EventCursor,
    /// Content hash (blake3) in hex format (only for PUT events with available hash).
    pub content_hash: Option<String>,
}

/// Builder for creating an event stream subscription.
///
/// Construct via [`crate::Pubky::event_stream`].
#[derive(Clone, Debug)]
pub struct EventStreamBuilder {
    client: PubkyHttpClient,
    users: Vec<(PublicKey, Option<EventCursor>)>,
    limit: Option<u16>,
    live: bool,
    reverse: bool,
    path: Option<String>,
}

impl EventStreamBuilder {
    /// Create a new event stream builder.
    ///
    /// Typically called via [`crate::Pubky::event_stream`].
    #[must_use]
    pub fn new(client: PubkyHttpClient) -> Self {
        Self {
            client,
            users: Vec::new(),
            limit: None,
            live: false,
            reverse: false,
            path: None,
        }
    }

    /// Add a user to the event stream subscription.
    ///
    /// You can add up to 50 users total. Each user can have an independent cursor position.
    /// If a user is added who already exists then their cursor value is overwritten with the newest value.
    ///
    /// IMPORTANT: Only the first added User's pubky is used to identify the Homeserver.
    /// It is the responsibility of the caller to ensure that all Users added are on the same Homeserver.
    ///
    /// # Errors
    /// - Returns an error if trying to add more than 50 users
    pub fn add_user(mut self, user: &PublicKey, cursor: Option<EventCursor>) -> Result<Self> {
        if self.users.len() >= 50 {
            return Err(Error::from(RequestError::Validation {
                message: "Cannot subscribe to more than 50 users".into(),
            }));
        }

        if let Some(existing) = self.users.iter_mut().find(|(u, _)| u == user) {
            existing.1 = cursor;
            return Ok(self);
        }

        self.users.push((user.clone(), cursor));
        Ok(self)
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
    /// When called, the stream will:
    /// 1. First deliver all historical events (oldest first)
    /// 2. Then remain open to stream new events as they occur in real-time
    ///
    /// Without this flag (default): Stream only delivers historical events and closes.
    ///
    /// **Note**: Cannot be combined with `reverse()`.
    #[must_use]
    pub const fn live(mut self) -> Self {
        self.live = true;
        self
    }

    /// Return events in reverse chronological order (newest first).
    ///
    /// When called, events are delivered from newest to oldest, then the stream closes.
    /// This is useful for fetching recent history.
    ///
    /// Without this flag (default): Events are delivered oldest first.
    ///
    /// **Note**: Cannot be combined with `live()`.
    #[must_use]
    pub const fn reverse(mut self) -> Self {
        self.reverse = true;
        self
    }

    /// Filter events by path prefix.
    ///
    /// Format: Path WITHOUT `pubky://` scheme or user pubkey (e.g., "/pub/files/" or "/pub/").
    #[must_use]
    pub fn path<S: Into<String>>(mut self, path: S) -> Self {
        self.path = Some(path.into());
        self
    }

    /// Build the event stream request URL with all query parameters.
    ///
    /// Constructs a URL like:
    /// `https://{homeserver}/events-stream?user=pk1&user=pk2:cursor&limit=100&live=true&path=/pub/`
    fn build_request_url(&self, homeserver: &PublicKey) -> Result<Url> {
        let mut url = Url::parse(&format!("https://{homeserver}/events-stream"))?;

        {
            let mut query = url.query_pairs_mut();
            for (user, cursor) in &self.users {
                if let Some(c) = cursor {
                    query.append_pair("user", &format!("{user}:{c}"));
                } else {
                    query.append_pair("user", &user.to_string());
                }
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
        Ok(url)
    }

    /// Internal helper that contains the shared subscription logic.
    async fn subscribe_internal(self) -> Result<impl Stream<Item = Result<Event>>> {
        if self.live && self.reverse {
            return Err(Error::from(RequestError::Validation {
                message: "Cannot use live mode with reverse ordering".into(),
            }));
        }

        if self.users.is_empty() {
            return Err(Error::from(RequestError::Validation {
                message: "At least one user must be specified".into(),
            }));
        }
        if self.users.len() > 50 {
            return Err(Error::from(RequestError::Validation {
                message: "Cannot subscribe to more than 50 users".into(),
            }));
        }

        // Resolve homeserver for the first user
        let (first_user, _) = &self.users[0];
        let homeserver = Pkdns::with_client(self.client.clone())
            .get_homeserver_of(first_user)
            .await
            .ok_or_else(|| {
                Error::from(RequestError::Validation {
                    message: format!("Could not resolve homeserver for user {first_user}"),
                })
            })?;

        cross_log!(
            info,
            "Subscribing to event stream for {} user(s) on homeserver {}",
            self.users.len(),
            homeserver
        );

        let url = self.build_request_url(&homeserver)?;
        let response = self
            .client
            .cross_request(Method::GET, url)
            .await?
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let message = format!("Event stream request failed with status {status}");
            return Err(Error::from(RequestError::Server { status, message }));
        }

        let sse_stream = response.bytes_stream().eventsource();
        let event_stream = sse_stream.filter_map(|result| async move {
            match result {
                Ok(sse_event) => match parse_sse_event(&sse_event) {
                    Ok(event) => Some(Ok(event)),
                    Err(e) => {
                        cross_log!(warn, "Failed to parse SSE event: {}", e);
                        Some(Err(e))
                    }
                },
                Err(e) => {
                    cross_log!(error, "SSE stream error: {}", e);
                    Some(Err(Error::from(RequestError::Validation {
                        message: format!("SSE stream error: {e}"),
                    })))
                }
            }
        });

        Ok(event_stream)
    }

    /// Subscribe to the event stream.
    ///
    /// This performs the following steps:
    /// 1. Resolves the user's homeserver via DHT/PKDNS
    /// 2. Constructs the `/events-stream` URL with query parameters
    /// 3. Makes the HTTP request
    /// 4. Returns a stream of parsed events
    ///
    /// The native version returns a `Send`-compatible stream for use in multi-threaded contexts.
    ///
    /// # Errors
    /// - Returns [`Error::Request`] if the homeserver cannot be resolved
    /// - Returns [`Error::Request`] if `live=true` and `reverse=true` (invalid combination)
    /// - Propagates HTTP request errors
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn subscribe(self) -> Result<Pin<Box<dyn Stream<Item = Result<Event>> + Send>>> {
        let stream = self.subscribe_internal().await?;
        Ok(Box::pin(stream))
    }

    /// Subscribe to the event stream (WASM version).
    ///
    /// This performs the following steps:
    /// 1. Resolves the user's homeserver via DHT/PKDNS
    /// 2. Constructs the `/events-stream` URL with query parameters
    /// 3. Makes the HTTP request
    /// 4. Returns a stream of parsed events
    ///
    /// The WASM version returns a stream without the `Send` bound, as WASM is single-threaded.
    ///
    /// # Errors
    /// - Returns [`Error::Request`] if the homeserver cannot be resolved
    /// - Returns [`Error::Request`] if `live=true` and `reverse=true` (invalid combination)
    /// - Propagates HTTP request errors
    #[cfg(target_arch = "wasm32")]
    pub async fn subscribe(self) -> Result<Pin<Box<dyn Stream<Item = Result<Event>>>>> {
        let stream = self.subscribe_internal().await?;
        Ok(Box::pin(stream))
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
    // Second line is the cursor
    let cursor_str = lines
        .get(1)
        .and_then(|line| line.strip_prefix("cursor: "))
        .ok_or_else(|| {
            Error::from(RequestError::Validation {
                message: "SSE event missing cursor line".into(),
            })
        })?;
    let cursor = cursor_str.parse::<EventCursor>().map_err(|e| {
        Error::from(RequestError::Validation {
            message: format!("Invalid cursor format '{cursor_str}': {e}"),
        })
    })?;

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
