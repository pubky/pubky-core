pub mod auth;
pub mod list_builder;
pub mod pkarr;
pub mod public;

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
