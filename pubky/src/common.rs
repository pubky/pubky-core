#[cfg(wasm_browser)]
use js_sys::Date;

/// Returns the current timestamp in secs since the UNIX epoch.
#[cfg(wasm_browser)]
pub(crate) fn timestamp() -> u64 {
    // Use JS Date.now() which returns ms since Unix epoch
    let ms = Date::now() as u64;
    // Convert to secs
    ms / 1000
}

#[cfg(not(wasm_browser))]
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(not(wasm_browser))]
pub(crate) fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
