//! Main Crate Error

use pkarr::dns::SimpleDnsError;

// Alias Result to be the crate Result.
pub type Result<T, E = Error> = core::result::Result<T, E>;

#[derive(thiserror::Error, Debug)]
/// Pk common Error
pub enum Error {
    /// For starter, to remove as code matures.
    #[error("Generic error: {0}")]
    Generic(String),

    #[error("Could not resolve endpoint for {0}")]
    ResolveEndpoint(String),

    #[error("Could not convert the passed type into a Url")]
    InvalidUrl,

    // === Transparent ===
    #[error(transparent)]
    Dns(#[from] SimpleDnsError),

    #[error(transparent)]
    Pkarr(#[from] pkarr::Error),

    #[error(transparent)]
    Url(#[from] url::ParseError),

    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),

    #[error(transparent)]
    Session(#[from] pubky_common::session::Error),

    #[error(transparent)]
    Crypto(#[from] pubky_common::crypto::Error),

    #[error(transparent)]
    RecoveryFile(#[from] pubky_common::recovery_file::Error),

    #[error(transparent)]
    AuthToken(#[from] pubky_common::auth::Error),
}

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsValue;

#[cfg(target_arch = "wasm32")]
impl From<Error> for JsValue {
    fn from(error: Error) -> JsValue {
        let error_message = error.to_string();
        js_sys::Error::new(&error_message).into()
    }
}
