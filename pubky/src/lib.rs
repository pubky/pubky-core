#![allow(unused)]

macro_rules! if_not_wasm {
    ($($item:item)*) => {$(
        #[cfg(not(target_arch = "wasm32"))]
        $item
    )*}
}

macro_rules! if_wasm {
    ($($item:item)*) => {$(
        #[cfg(target_arch = "wasm32")]
        $item
    )*}
}

mod error;
pub use error::Error;

if_not_wasm! {
    mod client;
    mod client_async;

    pub use client::PubkyClient;
}

if_wasm! {
    mod wasm;

    pub use wasm::{PubkyClient, Keypair};
}
