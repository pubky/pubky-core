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

if_not_wasm! {
    mod client;

    use client::PubkyClient;
}

if_wasm! {
    mod wasm;

    pub use wasm::keys::Keypair;
    pub use wasm::PubkyClient;
}

mod error;
mod shared;

pub use error::Error;
