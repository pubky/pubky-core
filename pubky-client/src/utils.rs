// sleep for native
#[cfg(not(wasm_browser))]
use tokio::time::sleep as inner_sleep;
// sleep for wasm
#[cfg(wasm_browser)]
use gloo_timers::future::sleep as inner_sleep;

use std::time::Duration;

#[macro_export]
macro_rules! cross_debug {
    ($($arg:tt)*) => {
        #[cfg(all(not(test), target_arch = "wasm32"))]
        log::debug!($($arg)*);
        #[cfg(all(not(test), not(target_arch = "wasm32")))]
        tracing::debug!($($arg)*);
        #[cfg(test)]
        println!($($arg)*);
    };
}

/// Sleep for the given duration.
/// Works on both native and wasm.
pub async fn sleep(duration: Duration) {
    inner_sleep(duration).await;
}
