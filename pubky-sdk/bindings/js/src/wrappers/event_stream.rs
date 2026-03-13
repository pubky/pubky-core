use base64::Engine;
use wasm_bindgen::prelude::*;

use crate::wrappers::resource::PubkyResource;

/// A single event from the event stream.
///
/// @example
/// ```typescript
/// for await (const event of stream) {
///   console.log(event.eventType);  // "PUT" or "DEL"
///   console.log(event.cursor);     // cursor string for pagination
///
///   if (event.eventType === "PUT") {
///     console.log("Hash:", event.contentHash);
///   }
///
///   // Access resource details
///   console.log(event.resource.owner.z32()); // User's public key
///   console.log(event.resource.path);        // "/pub/example.txt"
///   console.log(event.resource.toPubkyUrl()); // Full pubky:// URL
/// }
/// ```
#[wasm_bindgen]
#[derive(Debug, Clone)]
pub struct Event {
    /// Type of event ("PUT" or "DEL").
    event_type: String,
    /// The resource that was created, updated, or deleted.
    resource: PubkyResource,
    /// Cursor for pagination (event id as string).
    cursor: String,
    /// Content hash (blake3) in raw 32-byte base64 format (only for PUT events).
    content_hash: Option<String>,
}

#[wasm_bindgen]
impl Event {
    /// Get the event type ("PUT" or "DEL").
    #[wasm_bindgen(getter, js_name = "eventType")]
    pub fn event_type(&self) -> String {
        self.event_type.clone()
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

    /// Get the content hash (only for PUT events).
    /// Returns the blake3 hash in base64 format, or undefined for DELETE events.
    #[wasm_bindgen(getter, js_name = "contentHash")]
    pub fn content_hash(&self) -> Option<String> {
        self.content_hash.clone()
    }
}

impl From<pubky::Event> for Event {
    fn from(value: pubky::Event) -> Self {
        let (event_type, content_hash) = match &value.event_type {
            pubky::EventType::Put { content_hash } => {
                let hash_b64 =
                    base64::engine::general_purpose::STANDARD.encode(content_hash.as_bytes());
                ("PUT".to_string(), Some(hash_b64))
            }
            pubky::EventType::Delete => ("DEL".to_string(), None),
        };

        Event {
            event_type,
            resource: PubkyResource::from(value.resource),
            cursor: value.cursor.to_string(),
            content_hash,
        }
    }
}
