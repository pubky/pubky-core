use futures_util::StreamExt;
use wasm_bindgen::prelude::*;
use web_sys::ReadableStream;

use crate::wrappers::event_stream::Event;
use crate::wrappers::keys::PublicKey;

/// Builder for creating an event stream subscription.
///
/// Construct via `Pubky.eventStream()`.
///
/// @example
/// ```typescript
/// const stream = await pubky.eventStream()
///   .addUser(user1Pubkey, null)
///   .addUser(user2Pubkey, null)
///   .live()
///   .limit(100)
///   .path("/pub/")
///   .subscribe();
///
/// for await (const event of stream) {
///   console.log(event.eventType, event.resource.path);
/// }
/// ```
#[wasm_bindgen]
pub struct EventStreamBuilder(pub(crate) pubky::EventStreamBuilder);

#[wasm_bindgen]
impl EventStreamBuilder {
    /// Add a user to the event stream subscription.
    ///
    /// **Deprecated**: Use `eventStreamForUser()` for single-user streams or
    /// `addUsers()` for adding multiple users.
    ///
    /// You can add up to 50 users total.
    /// If the user is already in the subscription, their cursor position will be updated.
    ///
    /// @param {PublicKey} user - User public key
    /// @param {string | null} cursor - Optional cursor position (event ID as string) to start from
    /// @returns {EventStreamBuilder} - Builder for chaining
    /// @throws {Error} - If trying to add more than 50 users or if cursor is invalid
    /// @deprecated Use `eventStreamForUser()` or `addUsers()` instead
    #[wasm_bindgen(js_name = "addUser")]
    #[allow(deprecated)]
    pub fn add_user(
        self,
        user: &PublicKey,
        cursor: Option<String>,
    ) -> Result<EventStreamBuilder, JsValue> {
        let event_cursor = cursor
            .map(|c| {
                c.parse::<pubky::EventCursor>()
                    .map_err(|e| JsValue::from_str(&format!("Invalid cursor: {e}")))
            })
            .transpose()?;
        let builder = self
            .0
            .add_user(user.as_inner(), event_cursor)
            .map_err(|e| JsValue::from_str(&format!("Failed to add user: {e}")))?;
        Ok(EventStreamBuilder(builder))
    }

    /// Add multiple users to the event stream subscription at once.
    ///
    /// Each user can have an independent cursor position. If a user already exists,
    /// their cursor value is overwritten.
    ///
    /// @param {Array<[string, string | null]>} users - Array of [z32PublicKey, cursor] tuples
    /// @returns {EventStreamBuilder} - Builder for chaining
    /// @throws {Error} - If total users would exceed 50 or if any cursor/pubkey is invalid
    ///
    /// @example
    /// ```typescript
    /// const users: [string, string | null][] = [
    ///   [user1.z32(), null],
    ///   [user2.z32(), "100"],
    /// ];
    /// const stream = await pubky.eventStreamFor(homeserver)
    ///   .addUsers(users)
    ///   .live()
    ///   .subscribe();
    /// ```
    #[wasm_bindgen(js_name = "addUsers")]
    pub fn add_users(self, users: js_sys::Array) -> Result<EventStreamBuilder, JsValue> {
        // Parse all users first
        let mut parsed_users: Vec<(pubky::PublicKey, Option<pubky::EventCursor>)> = Vec::new();

        for item in users.iter() {
            let tuple = js_sys::Array::from(&item);
            if tuple.length() != 2 {
                return Err(JsValue::from_str(
                    "Each user entry must be a [PublicKey, cursor] tuple",
                ));
            }

            // Parse the public key from z32 string
            let user_str = tuple.get(0).as_string().ok_or_else(|| {
                JsValue::from_str("First element must be a z32 public key string")
            })?;
            let user = pubky::PublicKey::try_from(user_str)
                .map_err(|e| JsValue::from_str(&format!("Invalid public key: {e}")))?;

            let cursor_val = tuple.get(1);
            let event_cursor = if cursor_val.is_null() || cursor_val.is_undefined() {
                None
            } else {
                let cursor_str = cursor_val
                    .as_string()
                    .ok_or_else(|| JsValue::from_str("Cursor must be a string or null"))?;
                Some(
                    cursor_str
                        .parse::<pubky::EventCursor>()
                        .map_err(|e| JsValue::from_str(&format!("Invalid cursor: {e}")))?,
                )
            };

            parsed_users.push((user, event_cursor));
        }

        // Use add_users with references
        let user_refs: Vec<_> = parsed_users.iter().map(|(u, c)| (u, *c)).collect();
        let builder = self
            .0
            .add_users(user_refs)
            .map_err(|e| JsValue::from_str(&format!("Failed to add users: {e}")))?;

        Ok(EventStreamBuilder(builder))
    }

    /// Set maximum number of events to receive before closing the connection.
    ///
    /// If omitted:
    /// - With `live=false`: sends all historical events, then closes
    /// - With `live=true`: sends all historical events, then enters live mode (infinite stream)
    ///
    /// @param {number} limit - Maximum number of events (1-65535)
    /// @returns {EventStreamBuilder} - Builder for chaining
    #[wasm_bindgen]
    pub fn limit(self, limit: u16) -> Self {
        EventStreamBuilder(self.0.limit(limit))
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
    ///
    /// ## Cleanup
    /// To stop a live stream, use the reader's `cancel()` method:
    /// ```typescript
    /// const stream = await pubky.eventStream().addUser(user, null).live().subscribe();
    /// const reader = stream.getReader();
    ///
    /// while (true) {
    ///   const { done, value } = await reader.read();
    ///   if (shouldStop) {
    ///     await reader.cancel(); // Closes the connection
    ///     break;
    ///   }
    /// }
    /// ```
    ///
    /// @returns {EventStreamBuilder} - Builder for chaining
    #[wasm_bindgen]
    pub fn live(self) -> Self {
        EventStreamBuilder(self.0.live())
    }

    /// Return events in reverse chronological order (newest first).
    ///
    /// When called, events are delivered from newest to oldest, then the stream closes.
    ///
    /// Without this flag (default): Events are delivered oldest first.
    ///
    /// **Note**: Cannot be combined with `live()`.
    ///
    /// @returns {EventStreamBuilder} - Builder for chaining
    #[wasm_bindgen]
    pub fn reverse(self) -> Self {
        EventStreamBuilder(self.0.reverse())
    }

    /// Filter events by path prefix.
    ///
    /// Format: Path WITHOUT `pubky://` scheme or user pubkey (e.g., "/pub/files/" or "/pub/").
    /// Only events whose path starts with this prefix are returned.
    ///
    /// @param {string} path - Path prefix to filter by
    /// @returns {EventStreamBuilder} - Builder for chaining
    #[wasm_bindgen]
    pub fn path(self, path: String) -> Self {
        EventStreamBuilder(self.0.path(path))
    }

    /// Subscribe to the event stream.
    ///
    /// This performs the following steps:
    /// 1. Resolves the user's homeserver via DHT/PKDNS
    /// 2. Constructs the `/events-stream` URL with query parameters
    /// 3. Makes the HTTP request
    /// 4. Returns a Web ReadableStream of parsed events
    ///
    /// @returns {Promise<ReadableStream>} - A Web ReadableStream that yields Event objects
    ///
    /// @throws {PubkyError}
    /// - `{ name: "RequestError" }` if the homeserver cannot be resolved
    /// - `{ name: "ValidationError" }` if `live=true` and `reverse=true` (invalid combination)
    /// - Propagates HTTP request errors
    ///
    /// @example
    /// ```typescript
    /// const stream = await builder.subscribe();
    /// for await (const event of stream) {
    ///   console.log(`${event.eventType}: ${event.resource.path}`);
    /// }
    /// ```
    #[wasm_bindgen]
    pub async fn subscribe(self) -> Result<ReadableStream, JsValue> {
        // Call the underlying Rust implementation
        let rust_stream = self
            .0
            .subscribe()
            .await
            .map_err(|e| JsValue::from(crate::js_error::PubkyError::from(e)))?;

        let mapped_stream = rust_stream.map(|result| match result {
            Ok(event) => {
                let js_event = Event::from(event);
                Ok(JsValue::from(js_event))
            }
            Err(e) => {
                let pubky_err = crate::js_error::PubkyError::from(e);
                Err(JsValue::from(pubky_err))
            }
        });

        // Convert to Web ReadableStream using wasm_streams
        let wasm_stream = wasm_streams::ReadableStream::from_stream(mapped_stream);
        let web_sys_stream = wasm_stream.into_raw();
        Ok(web_sys_stream)
    }
}
