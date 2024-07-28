#![allow(unused)]

mod error;
mod shared;

#[cfg(not(target_arch = "wasm32"))]
mod native;

#[cfg(target_arch = "wasm32")]
mod wasm;
use wasm_bindgen::prelude::*;

#[cfg(not(target_arch = "wasm32"))]
use ::pkarr::PkarrClientAsync;

pub use error::Error;

#[derive(Debug, Clone)]
#[wasm_bindgen]
pub struct PubkyClient {
    http: reqwest::Client,
    #[cfg(not(target_arch = "wasm32"))]
    pkarr: PkarrClientAsync,
}
