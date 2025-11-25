use opentelemetry::metrics::{Counter, Histogram, Meter, MeterProvider, UpDownCounter};
use opentelemetry_sdk::metrics::SdkMeterProvider;
use prometheus::{Encoder, Registry, TextEncoder};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct Metrics {
    registry: Arc<Registry>,
    _provider: Arc<SdkMeterProvider>,
    events_db_query_duration: Histogram<f64>,
    event_stream_db_query_duration: Histogram<f64>,
    event_stream_broadcast_lagged_count: Counter<u64>,
    event_stream_active_connections: UpDownCounter<i64>,
    event_stream_connection_duration: Histogram<f64>,
}

impl Metrics {
    pub fn new() -> Self {
        let (registry, provider, meter) = init_metrics();

        // Initialize all metric instruments
        let events_db_query_duration = meter
            .f64_histogram("events_db_query_duration_ms")
            .with_description("Duration of /events database queries in milliseconds")
            .build();

        let event_stream_db_query_duration = meter
            .f64_histogram("event_stream_db_query_duration_ms")
            .with_description("Duration of /events-stream database queries in milliseconds")
            .build();

        let event_stream_broadcast_lagged_count = meter
            .u64_counter("event_stream_broadcast_lagged_count")
            .with_description("Number of times event stream broadcast channel lagged")
            .build();

        let event_stream_active_connections = meter
            .i64_up_down_counter("event_stream_active_connections")
            .with_description("Number of active event stream connections")
            .build();

        let event_stream_connection_duration = meter
            .f64_histogram("event_stream_connection_duration_seconds")
            .with_description("Duration of event stream connections in seconds")
            .build();

        Self {
            registry: Arc::new(registry),
            _provider: Arc::new(provider),
            events_db_query_duration,
            event_stream_db_query_duration,
            event_stream_broadcast_lagged_count,
            event_stream_active_connections,
            event_stream_connection_duration,
        }
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

    // Broadcast channel health
    pub fn record_broadcast_lagged(&self) {
        self.event_stream_broadcast_lagged_count.add(1, &[]);
    }

    // Connection tracking
    pub fn increment_active_connections(&self) {
        self.event_stream_active_connections.add(1, &[]);
    }

    pub fn decrement_active_connections(&self) {
        self.event_stream_active_connections.add(-1, &[]);
    }

    // Connection lifecycle
    pub fn record_connection_closed(&self, duration_secs: u64) {
        self.event_stream_connection_duration
            .record(duration_secs as f64, &[]);
    }

    /// Render Prometheus metrics in text format
    pub fn render(&self) -> String {
        let metric_families = self.registry.gather();
        let encoder = TextEncoder::new();
        let mut buffer = Vec::new();
        match encoder.encode(&metric_families, &mut buffer) {
            Ok(_) => String::from_utf8(buffer).unwrap_or_else(|e| {
                tracing::error!("Failed to convert metrics to UTF-8: {:?}", e);
                String::from("# Error encoding metrics\n")
            }),
            Err(e) => {
                tracing::error!("Failed to encode metrics: {:?}", e);
                String::from("# Error encoding metrics\n")
            }
        }
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Initialize OpenTelemetry with Prometheus exporter
/// Returns the Prometheus Registry, MeterProvider, and Meter for creating instruments
fn init_metrics() -> (Registry, SdkMeterProvider, Meter) {
    let registry = Registry::new();

    let exporter = opentelemetry_prometheus::exporter()
        .with_registry(registry.clone())
        .build()
        .expect("Failed to build Prometheus exporter");

    let provider = SdkMeterProvider::builder().with_reader(exporter).build();

    let meter = provider.meter("pubky_homeserver");

    (registry, provider, meter)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_recording() {
        let metrics = Metrics::new();

        // Record various metrics
        metrics.record_events_db_query(100);
        metrics.record_event_stream_db_query(200);
        metrics.increment_active_connections();
        metrics.record_broadcast_lagged();
        metrics.record_connection_closed(30);

        let output = metrics.render();

        // Verify output is valid Prometheus format
        assert!(!output.is_empty());
        assert!(output.starts_with("#") || output.contains("# HELP"));
        // Verify all metric names appear in output
        assert!(
            output.contains("events_db_query_duration_ms"),
            "Missing events_db_query_duration_ms in: {}",
            output
        );
        assert!(
            output.contains("event_stream_db_query_duration_ms"),
            "Missing event_stream_db_query_duration_ms in: {}",
            output
        );
        assert!(
            output.contains("event_stream_active_connections"),
            "Missing event_stream_active_connections in: {}",
            output
        );
        assert!(
            output.contains("event_stream_broadcast_lagged_count"),
            "Missing event_stream_broadcast_lagged_count in: {}",
            output
        );
        assert!(
            output.contains("event_stream_connection_duration_seconds"),
            "Missing event_stream_connection_duration_seconds in: {}",
            output
        );
    }
}
