use wasm_bindgen::prelude::*;

pub mod api;
pub mod constructor;
mod js_result;
mod wasm_http_client;
pub mod wrappers;

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
