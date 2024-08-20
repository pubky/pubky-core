//! Server events (Put and Delete entries)
//!
//! Useful as a realtime sync with Indexers until
//! we implement more self-authenticated merkle data.

use heed::{
    types::{Bytes, Str},
    Database,
};
use postcard::{from_bytes, to_allocvec};
use serde::{Deserialize, Serialize};

/// Event [Timestamp] base32 => Encoded event.
pub type EventsTable = Database<Str, Bytes>;

pub const EVENTS_TABLE: &str = "events";

#[derive(Clone, Serialize, Deserialize, Debug, Eq, PartialEq)]
pub enum Event {
    Put(String),
    Delete(String),
}

impl Event {
    pub fn put(url: &str) -> Self {
        Self::Put(url.to_string())
    }

    pub fn serialize(&self) -> Vec<u8> {
        to_allocvec(self).expect("Session::serialize")
    }

    pub fn deserialize(bytes: &[u8]) -> core::result::Result<Self, postcard::Error> {
        if bytes[0] > 1 {
            panic!("Unknown Event version");
        }

        from_bytes(bytes)
    }

    pub fn url(&self) -> &str {
        match self {
            Event::Put(url) => url,
            Event::Delete(url) => url,
        }
    }

    pub fn operation(&self) -> &str {
        match self {
            Event::Put(_) => "PUT",
            Event::Delete(_) => "DEL",
        }
    }
}
