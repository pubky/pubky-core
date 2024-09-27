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

use crate::database::DB;

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

    pub fn delete(url: &str) -> Self {
        Self::Delete(url.to_string())
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

const MAX_LIST_LIMIT: u16 = 1000;
const DEFAULT_LIST_LIMIT: u16 = 100;

impl DB {
    pub fn list_events(
        &self,
        limit: Option<u16>,
        cursor: Option<&str>,
    ) -> anyhow::Result<Vec<String>> {
        let txn = self.env.read_txn()?;

        let limit = limit.unwrap_or(DEFAULT_LIST_LIMIT).min(MAX_LIST_LIMIT);

        let mut cursor = cursor.unwrap_or("0000000000000");

        // Cursor smaller than 13 character is invalid
        // TODO: should we send an error instead?
        if cursor.len() < 13 {
            cursor = "0000000000000"
        }

        let mut result: Vec<String> = vec![];
        let mut next_cursor = cursor.to_string();

        for _ in 0..limit {
            match self.tables.events.get_greater_than(&txn, &next_cursor)? {
                Some((timestamp, event_bytes)) => {
                    let event = Event::deserialize(event_bytes)?;

                    let line = format!("{} {}", event.operation(), event.url());
                    next_cursor = timestamp.to_string();

                    result.push(line);
                }
                None => break,
            };
        }

        if !result.is_empty() {
            result.push(format!("cursor: {next_cursor}"))
        }

        txn.commit()?;

        Ok(result)
    }
}
