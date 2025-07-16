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

#[macro_export]
macro_rules! handle_http_error {
    ($res:expr) => {
        if let Err(status) = $res.error_for_status_ref() {
            return match $res.text().await {
                Ok(text) => Err(anyhow::anyhow!("{status}. Error message: {text}")),
                _ => Err(anyhow::anyhow!("{status}")),
            };
        }
    };
}
