#[cfg(not(target_arch = "wasm32"))]
pub mod cookies;
#[cfg(not(target_arch = "wasm32"))]
pub mod http_native;
#[cfg(target_arch = "wasm32")]
pub mod http_wasm;
