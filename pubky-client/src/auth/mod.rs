//! High-level pubkyauth flow: build a pubkyauth URL, wait for a response, turn it into an agent.

mod flow;

pub use flow::AuthFlow;

// Optional alias if you prefer the name externally.
pub type PubkyAuth = AuthFlow;
