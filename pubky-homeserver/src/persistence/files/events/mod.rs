//! Event system for file change notifications.
//!
//! - [`EventEntity`]: Represents a PUT or DEL event with path, content hash, and cursor ID.
//! - [`EventsLayer`]: OpenDAL middleware that intercepts writes/deletes to create events.
//! - [`EventRepository`]: Database queries for historical event retrieval and cursor pagination.
//! - [`EventsService`]: In-memory broadcast channel (capacity 1000) for real-time SSE
//!   streaming, combined with database persistence for historical replay.

mod events_entity;
mod events_layer;
pub(crate) mod events_repository;
mod events_service;

pub use events_entity::EventEntity;
pub use events_layer::EventsLayer;
pub use events_repository::{EventIden, EventRepository, EVENT_TABLE};
pub use events_service::{EventsService, MAX_EVENT_STREAM_USERS};

// Re-export from pubky_common for convenience
pub use pubky_common::events::{EventCursor, EventType};
