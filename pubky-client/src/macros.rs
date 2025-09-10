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
