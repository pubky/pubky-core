//! Optional Prometheus metrics server.
//!
//! Exposes a `/metrics` endpoint with counters and histograms for event stream
//! connections, database query latencies, and broadcast channel health.

mod app;
pub mod routes;

pub use app::{MetricsServer, MetricsServerBuildError};
