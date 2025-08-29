//! High-level pubkyauth flow: build a pubkyauth URL, wait for a response, turn it into an agent.

mod flow;

pub use flow::PubkyAuth;
