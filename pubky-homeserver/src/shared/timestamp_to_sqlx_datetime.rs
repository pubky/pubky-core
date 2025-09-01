use pubky_common::timestamp::Timestamp;
use sqlx::types::chrono::{DateTime, Utc};

/// Convert a pubky timestamp to a sqlx datetime.
pub fn timestamp_to_sqlx_datetime(timestamp: &Timestamp) -> DateTime<Utc> {
    let micros = timestamp.as_u64();
    let secs = micros / 1_000_000;
    let nanos = (micros % 1_000_000) * 1000;
    DateTime::from_timestamp(secs as i64, nanos as u32).expect("Failed to convert timestamp to sqlx datetime")
}