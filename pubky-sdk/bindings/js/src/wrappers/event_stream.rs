use base64::Engine;
use wasm_bindgen::prelude::*;

use crate::wrappers::resource::PubkyResource;

/// Type of event in the event stream.
///
/// Use the helper methods to check the event type and access the content hash.
///
/// @example
/// ```typescript
/// if (event.eventType.isPut()) {
///   console.log("PUT event with hash:", event.eventType.contentHash());
/// } else if (event.eventType.isDelete()) {
///   console.log("DELETE event");
/// }
/// ```
#[wasm_bindgen]
#[derive(Debug, Clone)]
pub struct EventType {
    kind: EventKind,
    /// Content hash (blake3) in base64 format (only for PUT events).
    content_hash: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EventKind {
    Put,
    Delete,
}

#[wasm_bindgen]
impl EventType {
    /// Returns true if this is a PUT event (resource created or updated).
    #[wasm_bindgen(js_name = "isPut")]
    pub fn is_put(&self) -> bool {
        self.kind == EventKind::Put
    }

    /// Returns true if this is a DELETE event (resource deleted).
    #[wasm_bindgen(js_name = "isDelete")]
    pub fn is_delete(&self) -> bool {
        self.kind == EventKind::Delete
    }

    /// Get the content hash in base64 format.
    /// Returns the blake3 hash for PUT events, or undefined for DELETE events.
    #[wasm_bindgen(js_name = "contentHash", getter)]
    pub fn content_hash(&self) -> Option<String> {
        self.content_hash.clone()
    }

    /// Get the string representation ("PUT" or "DEL").
    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string_js(&self) -> String {
        match self.kind {
            EventKind::Put => "PUT".to_string(),
            EventKind::Delete => "DEL".to_string(),
        }
    }
}

impl From<&pubky::EventType> for EventType {
    fn from(value: &pubky::EventType) -> Self {
        match value {
            pubky::EventType::Put { content_hash } => {
                let hash_b64 =
                    base64::engine::general_purpose::STANDARD.encode(content_hash.as_bytes());
                EventType {
                    kind: EventKind::Put,
                    content_hash: Some(hash_b64),
                }
            }
            pubky::EventType::Delete => EventType {
                kind: EventKind::Delete,
                content_hash: None,
            },
        }
    }
}

/// Cursor for pagination in event queries.
///
/// The cursor represents the ID of an event and is used for pagination.
///
/// @example
/// ```typescript
/// // Get cursor from an event
/// const cursor = event.cursor;
/// console.log(cursor.id());      // numeric ID as string
/// console.log(cursor.toString()); // same as id()
///
/// // Create a cursor for querying
/// const cursor = EventCursor.from("12345");
/// ```
#[wasm_bindgen]
#[derive(Debug, Clone)]
pub struct EventCursor(pubky::EventCursor);

#[wasm_bindgen]
impl EventCursor {
    /// Create a cursor from a string representation of the event ID.
    #[wasm_bindgen(js_name = "from")]
    pub fn from_str(id: &str) -> Result<EventCursor, JsValue> {
        id.parse::<pubky::EventCursor>()
            .map(EventCursor)
            .map_err(|e| JsValue::from_str(&format!("Invalid cursor: {}", e)))
    }

    /// Get the event ID as a string.
    /// Returns a string to safely handle large numbers in JavaScript.
    #[wasm_bindgen]
    pub fn id(&self) -> String {
        self.0.id().to_string()
    }

    /// Get the string representation (same as id()).
    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string_js(&self) -> String {
        self.0.to_string()
    }
}

impl From<pubky::EventCursor> for EventCursor {
    fn from(value: pubky::EventCursor) -> Self {
        EventCursor(value)
    }
}

impl EventCursor {
    /// Get the inner native cursor (for Rust-side usage).
    pub fn into_inner(self) -> pubky::EventCursor {
        self.0
    }
}

/// A single event from the event stream.
///
/// @example
/// ```typescript
/// for await (const event of stream) {
///   // Check event type
///   if (event.eventType.isPut()) {
///     console.log("PUT:", event.resource.toPubkyUrl());
///     console.log("Hash:", event.eventType.contentHash());
///   } else {
///     console.log("DEL:", event.resource.toPubkyUrl());
///   }
///
///   // Access resource details
///   console.log(event.resource.owner.z32()); // User's public key
///   console.log(event.resource.path);        // "/pub/example.txt"
///   console.log(event.resource.toPubkyUrl()); // Full pubky:// URL
///   console.log(event.cursor.id());          // Cursor for pagination
/// }
/// ```
#[wasm_bindgen]
#[derive(Debug, Clone)]
pub struct Event {
    /// Type of event (PUT or DELETE).
    event_type: EventType,
    /// The resource that was created, updated, or deleted.
    resource: PubkyResource,
    /// Cursor for pagination.
    cursor: EventCursor,
}

#[wasm_bindgen]
impl Event {
    /// Get the event type.
    #[wasm_bindgen(getter, js_name = "eventType")]
    pub fn event_type(&self) -> EventType {
        self.event_type.clone()
    }

    /// Get the resource that was created, updated, or deleted.
    #[wasm_bindgen(getter)]
    pub fn resource(&self) -> PubkyResource {
        self.resource.clone()
    }

    /// Get the cursor for pagination.
    #[wasm_bindgen(getter)]
    pub fn cursor(&self) -> EventCursor {
        self.cursor.clone()
    }
}

impl From<pubky::Event> for Event {
    fn from(value: pubky::Event) -> Self {
        Event {
            event_type: EventType::from(&value.event_type),
            resource: PubkyResource::from(value.resource),
            cursor: EventCursor::from(value.cursor),
        }
    }
}
