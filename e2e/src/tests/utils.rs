use std::sync::Once;

static TRACING_INIT: Once = Once::new();

/// Initializes the tracing subscriber for tests.
pub fn init_tracing() {
    TRACING_INIT.call_once(|| {
        tracing_subscriber::fmt()
            .with_env_filter(std::env::var("TRACING").unwrap_or_else(|_| "info".to_string()))
            // Use with_test_writer to ensure logs are captured correctly by the test runner.
            .with_test_writer()
            .init();
    });
}
