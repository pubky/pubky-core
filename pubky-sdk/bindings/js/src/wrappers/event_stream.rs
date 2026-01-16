use wasm_bindgen::prelude::*;

use crate::wrappers::resource::PubkyResource;

/// Type of event in the event stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventType {
    Put,
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
///
/// @example
/// ```typescript
/// for await (const event of stream) {
///   console.log(event.eventType);           // "PUT" or "DEL"
///   console.log(event.resource.owner.z32()); // User's public key
///   console.log(event.resource.path);        // "/pub/example.txt"
///   console.log(event.resource.toPubkyUrl()); // Full pubky:// URL
///   console.log(event.cursor);               // Cursor for pagination
/// }
/// ```
#[wasm_bindgen]
#[derive(Debug, Clone)]
pub struct Event {
    /// Type of event (PUT or DEL).
    event_type: EventType,
    /// The resource that was created, updated, or deleted.
    resource: PubkyResource,
    /// Cursor for pagination (event id as string).
    cursor: String,
    /// Content hash (blake3) in hex format (only for PUT events with available hash).
    content_hash: Option<String>,
}

#[wasm_bindgen]
impl Event {
    /// Get the event type ("PUT" or "DEL").
    #[wasm_bindgen(getter, js_name = "eventType")]
    pub fn event_type(&self) -> String {
        match self.event_type {
            EventType::Put => "PUT".to_string(),
            EventType::Delete => "DEL".to_string(),
        }
    }

    /// Get the resource that was created, updated, or deleted.
    #[wasm_bindgen(getter)]
    pub fn resource(&self) -> PubkyResource {
        self.resource.clone()
    }

    /// Get the cursor for pagination.
    #[wasm_bindgen(getter)]
    pub fn cursor(&self) -> String {
        self.cursor.clone()
    }

    /// Get the content hash (only for PUT events with available hash).
    #[wasm_bindgen(getter, js_name = "contentHash")]
    pub fn content_hash(&self) -> Option<String> {
        self.content_hash.clone()
    }
}

impl From<pubky::Event> for Event {
    fn from(value: pubky::Event) -> Self {
        Event {
            event_type: value.event_type.into(),
            resource: PubkyResource::from(value.resource),
            cursor: value.cursor.to_string(),
            content_hash: value.content_hash,
        }
    }
}
