use wasm_bindgen::prelude::*;

use crate::js_error::JsResult;

/// An enum representing the available verbosity levels of the logger.
#[wasm_bindgen]
pub enum Level {
    Error = "error",
    Warn = "warn",
    Info = "info",
    Debug = "debug",
    Trace = "trace",
}

impl Level {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
            Self::__Invalid => unreachable!("Invalid Level variant"),
        }
    }
}

impl Into<log::Level> for Level {
    fn into(self) -> log::Level {
        match self {
            Self::Error => log::Level::Error,
            Self::Warn => log::Level::Warn,
            Self::Info => log::Level::Info,
            Self::Debug => log::Level::Debug,
            Self::Trace => log::Level::Trace,
            Self::__Invalid => unreachable!("Invalid Level variant"),
        }
    }
}

/// Set the global logging verbosity for the WASM Pubky SDK. Routes Rust `log` output to the browser console.
///
/// Accepted values (case-sensitive): "error" | "warn" | "info" | "debug" | "trace".
/// Effects:
/// - Initializes the logger once; subsequent calls may throw if the logger is already set.
/// - Emits a single info log: `Log level set to: <level>`.
/// - Messages at or above `level` are forwarded to the appropriate `console.*` method.
///
/// @param {Level} level
///        Minimum log level to enable. One of: "error" | "warn" | "info" | "debug" | "trace".
///
/// @returns {void}
///
/// @throws {Error}
///         If `level` is invalid ("Invalid log level") or the logger cannot be initialized
///         (e.g., already initialized).
///
/// Usage:
///   Call once at application startup, before invoking other SDK APIs.
#[wasm_bindgen(js_name = "setLogLevel")]
pub fn set_log_level(level: Level) -> Result<(), JsValue> {
    let level_str = level.as_str();
    console_log::init_with_level(level.into()).map_err(|e| JsValue::from_str(&e.to_string()))?;
    log::info!("Log level set to: {level_str}");
    Ok(())
}

/// Resolve a `pubky://` or `pubky<pk>/…` identifier into the homeserver transport URL.
///
/// @param {string} identifier Either `pubky<pk>/...` (preferred) or `pubky://<pk>/...`.
/// @returns {string} HTTPS URL in the form `https://_pubky.<pk>/...`.
#[wasm_bindgen(js_name = "resolvePubky")]
pub fn resolve_pubky(identifier: &str) -> JsResult<String> {
    Ok(pubky::resolve_pubky(identifier)?.to_string())
}
