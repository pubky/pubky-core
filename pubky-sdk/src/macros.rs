/// Cross-platform logging macro with explicit level selection.
///
/// On native (non-WASM) builds it forwards to [`tracing`] macros.
/// In WASM builds (e.g. browsers) it forwards to log crate macros.
/// During tests it prints to `stdout`, preserving the log level for context.
///
/// This allows shared instrumentation across targets without conditional
/// compilation around the logging backend.
///
/// # Examples
/// ```
/// use pubky::cross_log;
/// # fn main() {
/// cross_log!(info, "listing {} entries", 42);
/// cross_log!(warn, "slow response from {}", "relay");
/// # }
/// ```
#[macro_export]
macro_rules! cross_log {
    ($level:ident, $($arg:tt)*) => {
        #[cfg(all(not(test), target_arch = "wasm32"))]
        log::$level!($($arg)*);
        #[cfg(all(not(test), not(target_arch = "wasm32")))]
        tracing::$level!($($arg)*);
        #[cfg(test)]
        println!("[{}] {}", stringify!($level), format_args!($($arg)*));
    };
}
