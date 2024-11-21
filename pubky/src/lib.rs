#![doc = include_str!("../README.md")]
//!

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

/// A client for Pubky homeserver API, as well as generic HTTP requests to Pubky urls.
#[derive(Debug, Clone)]
#[wasm_bindgen]
pub struct Client {
    http: reqwest::Client,
    pub(crate) pkarr: pkarr::Client,
}
