use tracing::Subscriber;
use tracing_subscriber::layer::Context;
use tracing_subscriber::{registry, Layer};
use tracing_subscriber::fmt::format::DefaultFields;
use tracing_subscriber::fmt::format::{FormatFields, Writer};
use crate::LoggingToml;
use anyhow::{Result};
use tracing_core::Level;

/// Log layer for Homeserver
#[derive(Clone)]
pub struct SuiteTraceLayer {
    instance: String,
    min_level: Level,
}

impl Default for SuiteTraceLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl SuiteTraceLayer {
    /// Creates a new SuiteTraceLayer with default settings (INFO level and above)
    pub fn new() -> Self {
        Self {
            instance: "[suite]".to_string(),
            min_level: Level::INFO,
        }
    }

    /// Creates a new SuiteTraceLayer from Logging config
    pub fn from_config(config: &LoggingToml, instance: &str) -> Result<Self> {
        // Parse main log level directive (e.g., "info")
        let min_level: Level = config.level.to_owned().into();

        // Set of excluded targets (optional)
        //let exclude_targets = config.exclude_targets.iter().cloned().collect();

        Ok(SuiteTraceLayer {
            instance: instance.to_string(),
            min_level,
        })
    }
}

impl<S> Layer<S> for SuiteTraceLayer
where
    S: Subscriber + for<'a> registry::LookupSpan<'a>,
{
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();

        if let Some(module) = meta.module_path() {
            if module.starts_with("pubky_homeserver") {
                let mut buf = String::new();
                let mut writer = Writer::new(&mut buf);

                if *meta.level() > self.min_level {
                    return;
                }

                let _ = write!(
                    writer,
                    "{} {:<5} {}<{}>: ",
                    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
                    meta.level(),
                    meta.target(),
                    self.instance
                );

                // Format fields using DefaultFields formatter
                let fmt_fields = DefaultFields::new();
                let _ = fmt_fields.format_fields(writer, event);

                println!("{}", buf);
            }
        }
    }
    // You can also override `on_enter`, `on_exit`, `on_new_span`, etc.
}