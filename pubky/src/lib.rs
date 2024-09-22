mod error;
mod shared;

#[cfg(not(target_arch = "wasm32"))]
mod native;

#[cfg(target_arch = "wasm32")]
mod wasm;

use wasm_bindgen::prelude::*;

pub use error::Error;

#[cfg(not(target_arch = "wasm32"))]
pub use crate::shared::list_builder::ListBuilder;

#[derive(Debug, Clone)]
#[wasm_bindgen]
pub struct PubkyClient {
    http: reqwest::Client,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) pkarr: pkarr::Client,
    #[cfg(target_arch = "wasm32")]
    pub(crate) pkarr_relays: Vec<String>,
}
