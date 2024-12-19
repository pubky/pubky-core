#![doc = include_str!("../README.md")]
//!

mod shared;

#[cfg(not(target_arch = "wasm32"))]
mod native;

#[cfg(target_arch = "wasm32")]
mod wasm;

use std::fmt::Debug;

use wasm_bindgen::prelude::*;

#[cfg(not(target_arch = "wasm32"))]
pub use crate::shared::list_builder::ListBuilder;

/// A client for Pubky homeserver API, as well as generic HTTP requests to Pubky urls.
#[derive(Clone)]
#[wasm_bindgen]
pub struct Client {
    http: reqwest::Client,
    pkarr: pkarr::Client,

    #[cfg(not(target_arch = "wasm32"))]
    cookie_store: std::sync::Arc<native::CookieJar>,
    #[cfg(not(target_arch = "wasm32"))]
    icann_http: reqwest::Client,

    #[cfg(target_arch = "wasm32")]
    testnet: bool,
}

impl Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pubky Client").finish()
    }
}
