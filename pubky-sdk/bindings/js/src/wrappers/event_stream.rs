use serde::{Deserialize, Serialize};
use tsify::Tsify;
use wasm_bindgen::prelude::*;

/// Type of event in the event stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub enum EventType {
    #[serde(rename = "PUT")]
    Put,
    #[serde(rename = "DEL")]
    Delete,
}

impl From<pubky::EventType> for EventType {
    fn from(value: pubky::EventType) -> Self {
        match value {
            pubky::EventType::Put => EventType::Put,
            pubky::EventType::Delete => EventType::Delete,
        }
    }
}

/// A single event from the event stream.
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    /// Type of event (PUT or DEL).
    pub event_type: EventType,
    /// Full pubky path (e.g., "pubky://user_pubkey/pub/example.txt").
    pub path: String,
    /// Cursor for pagination (event id as string).
    pub cursor: String,
    /// Content hash (blake3) in hex format (only for PUT events with available hash).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
}

impl From<pubky::Event> for Event {
    fn from(value: pubky::Event) -> Self {
        Event {
            event_type: value.event_type.into(),
            path: value.path,
            cursor: value.cursor.to_string(),
            content_hash: value.content_hash,
        }
    }
}
