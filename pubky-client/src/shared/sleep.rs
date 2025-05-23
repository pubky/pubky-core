// sleep for native
#[cfg(not(wasm_browser))]
use tokio::time::sleep as inner_sleep;
// sleep for wasm
#[cfg(wasm_browser)]
use gloo_timers::future::sleep as inner_sleep;

use std::time::Duration;

/// Sleep for the given duration.
/// Works on both native and wasm.
pub async fn sleep(duration: Duration) {
    inner_sleep(duration).await;
}
