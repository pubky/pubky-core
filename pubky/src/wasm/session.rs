use pubky_common::session;

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct Session(pub(crate) session::Session);
