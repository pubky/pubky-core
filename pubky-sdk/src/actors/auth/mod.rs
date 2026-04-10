pub(crate) mod approval;
pub mod auth_flow;
pub mod deep_links;
pub mod relay;

#[allow(
    unused_imports,
    reason = "Preserve existing auth module paths after relay split"
)]
pub use relay::{auth_relay_listener, http_relay_inbox_channel, http_relay_link_channel};
