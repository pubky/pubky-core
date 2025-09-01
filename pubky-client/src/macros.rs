/// Cross-platform `debug!` logging macro.
///
/// On native (non-WASM) builds it forwards to [`tracing::debug!`].  
/// In WASM builds (e.g. browsers) it forwards to [`log::debug!`].  
/// In tests it prints to `stdout`.
///
/// Useful when writing code that runs on both native and WASM without
/// pulling platform-specific logging crates into your app.
///
/// # Examples
/// ```
/// use pubky::cross_debug;
/// # fn main() {
/// cross_debug!("listing {} entries", 42);
/// # }
/// ```
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
