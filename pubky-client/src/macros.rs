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
    ($response:expr) => {
        if !$response.status().is_success() {
            let status = $response.status();
            let message = $response.text().await.unwrap_or_else(|_| {
                status
                    .canonical_reason()
                    .unwrap_or("Unknown Error")
                    .to_string()
            });

            return Err(crate::errors::Error::HttpStatus { status, message });
        }
    };
}
