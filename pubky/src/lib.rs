#![doc = include_str!("../README.md")]
//!

mod error;
mod shared;

#[cfg(not(target_arch = "wasm32"))]
mod native;

#[cfg(target_arch = "wasm32")]
mod wasm;

use std::{fmt::Debug, sync::Arc};

use wasm_bindgen::prelude::*;

pub use error::Error;

#[cfg(not(target_arch = "wasm32"))]
pub use crate::shared::list_builder::ListBuilder;

/// A client for Pubky homeserver API, as well as generic HTTP requests to Pubky urls.
#[derive(Clone)]
#[wasm_bindgen]
pub struct Client {
    http: reqwest::Client,
    pkarr: pkarr::Client,

    #[cfg(not(target_arch = "wasm32"))]
    cookie_store: Arc<native::CookieJar>,
    #[cfg(not(target_arch = "wasm32"))]
    icann_http: reqwest::Client,
}

impl Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pubky Client").finish()
    }
}
