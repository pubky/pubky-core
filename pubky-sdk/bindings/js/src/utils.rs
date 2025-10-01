use wasm_bindgen::prelude::*;

/// Set the global logging verbosity for the WASM Pubky SDK. Routes Rust `log` output to the browser console.
///
/// Accepted values (case-insensitive): "error" | "warn" | "info" | "debug" | "trace".
/// Effects:
/// - Initializes the logger once; subsequent calls may throw if the logger is already set.
/// - Emits a single info log: `Log level set to: <level>`.
/// - Messages at or above `level` are forwarded to the appropriate `console.*` method.
///
/// @param {string} level
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
pub fn set_log_level(level: &str) -> Result<(), JsValue> {
    let level = match level.to_lowercase().as_str() {
        "error" => log::Level::Error,
        "warn" => log::Level::Warn,
        "info" => log::Level::Info,
        "debug" => log::Level::Debug,
        "trace" => log::Level::Trace,
        _ => return Err(JsValue::from_str("Invalid log level")),
    };

    console_log::init_with_level(level).map_err(|e| JsValue::from_str(&e.to_string()))?;
    log::info!("Log level set to: {}", level);
    Ok(())
}
