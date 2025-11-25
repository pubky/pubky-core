use opentelemetry::metrics::{Counter, Histogram, Meter, MeterProvider, UpDownCounter};
use opentelemetry_sdk::metrics::SdkMeterProvider;
use prometheus::{Encoder, Registry, TextEncoder};
use std::sync::Arc;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum MetricsInitError {
    #[error("Failed to build Prometheus exporter: {0}")]
    PrometheusExporter(String),
}

pub const EVENTS_DB_QUERY_DURATION: &str = "events_db_query_duration_ms";
pub const EVENT_STREAM_DB_QUERY_DURATION: &str = "event_stream_db_query_duration_ms";
pub const EVENT_STREAM_BROADCAST_LAGGED_COUNT: &str = "event_stream_broadcast_lagged_count";
pub const EVENT_STREAM_BROADCAST_HALF_FULL_COUNT: &str = "event_stream_broadcast_half_full_count";
pub const EVENT_STREAM_ACTIVE_CONNECTIONS: &str = "event_stream_active_connections";
pub const EVENT_STREAM_CONNECTION_DURATION: &str = "event_stream_connection_duration_ms";

#[derive(Clone, Debug)]
pub struct Metrics {
    registry: Arc<Registry>,
    _provider: Arc<SdkMeterProvider>,
    events_db_query_duration: Histogram<f64>,
    event_stream_db_query_duration: Histogram<f64>,
    event_stream_broadcast_lagged_count: Counter<u64>,
    event_stream_broadcast_half_full_count: Counter<u64>,
    event_stream_active_connections: UpDownCounter<i64>,
    event_stream_connection_duration: Histogram<f64>,
}

impl Metrics {
    pub fn new() -> Result<Self, MetricsInitError> {
        let (registry, provider, meter) = init_metrics()?;

        let events_db_query_duration = meter
            .f64_histogram(EVENTS_DB_QUERY_DURATION)
            .with_description("Duration of /events database queries in milliseconds")
            .build();

        let event_stream_db_query_duration = meter
            .f64_histogram(EVENT_STREAM_DB_QUERY_DURATION)
            .with_description("Duration of /events-stream database queries in milliseconds")
            .build();

        let event_stream_broadcast_lagged_count = meter
            .u64_counter(EVENT_STREAM_BROADCAST_LAGGED_COUNT)
            .with_description("Number of times event stream broadcast channel lagged")
            .build();

        let event_stream_broadcast_half_full_count = meter
            .u64_counter(EVENT_STREAM_BROADCAST_HALF_FULL_COUNT)
            .with_description(
                "Number of times event stream broadcast channel reached half capacity",
            )
            .build();

        let event_stream_active_connections = meter
            .i64_up_down_counter(EVENT_STREAM_ACTIVE_CONNECTIONS)
            .with_description("Number of active event stream connections")
            .build();

        let event_stream_connection_duration = meter
            .f64_histogram(EVENT_STREAM_CONNECTION_DURATION)
            .with_description("Duration of event stream connections in milliseconds")
            .build();

        Ok(Self {
            registry: Arc::new(registry),
            _provider: Arc::new(provider),
            events_db_query_duration,
            event_stream_db_query_duration,
            event_stream_broadcast_lagged_count,
            event_stream_broadcast_half_full_count,
            event_stream_active_connections,
            event_stream_connection_duration,
        })
    }

    // === /events endpoint metrics ===

    pub fn record_events_db_query(&self, duration_ms: u128) {
        self.events_db_query_duration
            .record(duration_ms as f64, &[]);
    }

    // === /events-stream endpoint metrics ===

    pub fn record_event_stream_db_query(&self, duration_ms: u128) {
        self.event_stream_db_query_duration
            .record(duration_ms as f64, &[]);
    }

    pub fn record_broadcast_lagged(&self) {
        self.event_stream_broadcast_lagged_count.add(1, &[]);
    }

    pub fn record_broadcast_half_full(&self) {
        self.event_stream_broadcast_half_full_count.add(1, &[]);
    }

    pub fn increment_active_connections(&self) {
        self.event_stream_active_connections.add(1, &[]);
    }

    pub fn decrement_active_connections(&self) {
        self.event_stream_active_connections.add(-1, &[]);
    }

    pub fn record_connection_closed(&self, duration_ms: u128) {
        self.event_stream_connection_duration
            .record(duration_ms as f64, &[]);
    }

    /// Render Prometheus metrics in text format
    pub fn render(&self) -> Result<String, String> {
        let metric_families = self.registry.gather();
        let encoder = TextEncoder::new();
        let mut buffer = Vec::new();

        encoder.encode(&metric_families, &mut buffer).map_err(|e| {
            tracing::error!("Failed to encode metrics: {:?}", e);
            format!("Failed to encode metrics: {}", e)
        })?;

        String::from_utf8(buffer).map_err(|e| {
            tracing::error!("Failed to convert metrics to UTF-8: {:?}", e);
            format!("Failed to convert metrics to UTF-8: {}", e)
        })
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
            .expect("Failed to initialize metrics - this should never fail with default config")
    }
}

/// Initialize OpenTelemetry with Prometheus exporter
/// Returns the Prometheus Registry, MeterProvider, and Meter for creating instruments
fn init_metrics() -> Result<(Registry, SdkMeterProvider, Meter), MetricsInitError> {
    let registry = Registry::new();
    let exporter = opentelemetry_prometheus::exporter()
        .with_registry(registry.clone())
        .build()
        .map_err(|e| MetricsInitError::PrometheusExporter(e.to_string()))?;
    let provider = SdkMeterProvider::builder().with_reader(exporter).build();
    let meter = provider.meter("pubky_homeserver");
    Ok((registry, provider, meter))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_recording() {
        let metrics = Metrics::new().expect("Failed to create metrics");

        // Record various metrics
        metrics.record_events_db_query(100);
        metrics.record_event_stream_db_query(200);
        metrics.increment_active_connections();
        metrics.record_broadcast_lagged();
        metrics.record_broadcast_half_full();
        metrics.record_connection_closed(30);

        let output = metrics.render().expect("Failed to render metrics");

        // Verify output is valid Prometheus format
        assert!(!output.is_empty());
        assert!(output.starts_with("#") || output.contains("# HELP"));
        // Verify all metric names appear in output
        assert!(
            output.contains(EVENTS_DB_QUERY_DURATION),
            "Missing {} in: {}",
            EVENTS_DB_QUERY_DURATION,
            output
        );
        assert!(
            output.contains(EVENT_STREAM_DB_QUERY_DURATION),
            "Missing {} in: {}",
            EVENT_STREAM_DB_QUERY_DURATION,
            output
        );
        assert!(
            output.contains(EVENT_STREAM_ACTIVE_CONNECTIONS),
            "Missing {} in: {}",
            EVENT_STREAM_ACTIVE_CONNECTIONS,
            output
        );
        assert!(
            output.contains(EVENT_STREAM_BROADCAST_LAGGED_COUNT),
            "Missing {} in: {}",
            EVENT_STREAM_BROADCAST_LAGGED_COUNT,
            output
        );
        assert!(
            output.contains(EVENT_STREAM_BROADCAST_HALF_FULL_COUNT),
            "Missing {} in: {}",
            EVENT_STREAM_BROADCAST_HALF_FULL_COUNT,
            output
        );
        assert!(
            output.contains(EVENT_STREAM_CONNECTION_DURATION),
            "Missing {} in: {}",
            EVENT_STREAM_CONNECTION_DURATION,
            output
        );
    }
}
