//! Events module - handles event streaming and notifications
//!
//! This module provides a cohesive abstraction for managing events in the homeserver.
mod events_service;
mod stream_params;

pub use events_service::EventsService;
pub use stream_params::EventStreamQueryParams;
