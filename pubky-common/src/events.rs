//! Event types shared across Pubky crates.
//!
//! This module provides unified types for event streaming functionality,
//! used by both the homeserver and SDK.

use std::fmt::Display;
use std::str::FromStr;

use crate::crypto::Hash;
use serde::{Deserialize, Serialize};

/// Cursor for pagination in event queries.
///
/// The cursor represents the ID of an event and is used for pagination.
/// It can be parsed from a string representation of an integer.
///
/// Note: Uses `u64` internally, but Postgres BIGINT is signed (`i64`).
/// sea_query/sqlx binds `u64` values, which works correctly as long as
/// IDs stay within `i64::MAX` (~9.2 quintillion). Since event IDs are
/// auto-incrementing from 1, this is not a practical concern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct EventCursor(u64);

impl EventCursor {
    /// Create a new cursor from an event ID.
    #[must_use]
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    /// Get the underlying ID value.
    #[must_use]
    pub fn id(&self) -> u64 {
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

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(EventCursor(s.parse()?))
    }
}

impl From<u64> for EventCursor {
    fn from(id: u64) -> Self {
        EventCursor(id)
    }
}

impl TryFrom<&str> for EventCursor {
    type Error = std::num::ParseIntError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl TryFrom<String> for EventCursor {
    type Error = std::num::ParseIntError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

/// Type of event in the event stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventType {
    /// PUT event - resource created or updated, with its content hash.
    Put {
        /// Blake3 hash of the content.
        content_hash: Hash,
    },
    /// DELETE event - resource deleted.
    Delete,
}

impl EventType {
    /// Get the string representation of the event type.
    pub fn as_str(&self) -> &'static str {
        match self {
            EventType::Put { .. } => "PUT",
            EventType::Delete => "DEL",
        }
    }

    /// Get the content hash if this is a PUT event.
    pub fn content_hash(&self) -> Option<&Hash> {
        match self {
            EventType::Put { content_hash } => Some(content_hash),
            EventType::Delete => None,
        }
    }
}

impl Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_display_and_from_str() {
        let cursor = EventCursor::new(12345);
        assert_eq!(cursor.to_string(), "12345");

        let parsed: EventCursor = "67890".parse().unwrap();
        assert_eq!(parsed.id(), 67890);

        let from_u64: EventCursor = 111u64.into();
        assert_eq!(from_u64.id(), 111);

        let try_from_str = EventCursor::try_from("222").unwrap();
        assert_eq!(try_from_str.id(), 222);

        let try_from_string = EventCursor::try_from("333".to_string()).unwrap();
        assert_eq!(try_from_string.id(), 333);
    }

    #[test]
    fn cursor_ordering() {
        let c1 = EventCursor::new(1);
        let c2 = EventCursor::new(2);
        let c3 = EventCursor::new(2);

        assert!(c1 < c2);
        assert!(c2 > c1);
        assert_eq!(c2, c3);
    }

    #[test]
    fn event_type_display() {
        let put = EventType::Put {
            content_hash: Hash::from_bytes([0; 32]),
        };
        let del = EventType::Delete;

        assert_eq!(put.to_string(), "PUT");
        assert_eq!(del.to_string(), "DEL");
        assert_eq!(put.as_str(), "PUT");
        assert_eq!(del.as_str(), "DEL");
    }

    #[test]
    fn event_type_content_hash() {
        let hash = Hash::from_bytes([1; 32]);
        let put = EventType::Put {
            content_hash: hash.clone(),
        };
        let del = EventType::Delete;

        assert_eq!(put.content_hash(), Some(&hash));
        assert_eq!(del.content_hash(), None);
    }

    #[test]
    fn cursor_parse_error() {
        assert!("abc".parse::<EventCursor>().is_err());
        assert!("".parse::<EventCursor>().is_err());
        assert!("-1".parse::<EventCursor>().is_err());
        assert!("12.34".parse::<EventCursor>().is_err());
    }

    #[test]
    fn event_type_serde_roundtrip() {
        let put = EventType::Put {
            content_hash: Hash::from_bytes([1; 32]),
        };
        let json = serde_json::to_string(&put).expect("Failed to serialize PUT");
        let deserialized: EventType =
            serde_json::from_str(&json).expect("Failed to deserialize PUT");
        assert_eq!(put, deserialized);

        let del = EventType::Delete;
        let json = serde_json::to_string(&del).expect("Failed to serialize DELETE");
        let deserialized: EventType =
            serde_json::from_str(&json).expect("Failed to deserialize DELETE");
        assert_eq!(del, deserialized);
    }
}
