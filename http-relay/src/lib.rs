//! A Rust implementation of _some_ of [Http relay spec](https://httprelay.io/).
//!

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![cfg_attr(any(), deny(clippy::unwrap_used))]

mod http_relay;
mod waiting_list;

pub use http_relay::*;
