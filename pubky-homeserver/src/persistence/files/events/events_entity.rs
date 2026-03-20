use chrono::NaiveDateTime;
use pubky_common::crypto::Hash;
use pubky_common::crypto::PublicKey;
use pubky_common::events::{EventCursor, EventType};
use sea_query::Iden;
use serde::{Deserialize, Serialize};
use sqlx::{postgres::PgRow, FromRow, Row};

use crate::{
    persistence::{files::events::events_repository::EventIden, sql::user::UserIden},
    shared::webdav::{EntryPath, WebDavPath},
};

#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub struct EventEntity {
    pub id: u64,
    pub user_id: i32,
    pub user_pubkey: PublicKey,
    pub event_type: EventType,
    pub path: EntryPath,
    pub created_at: NaiveDateTime,
}

impl EventEntity {
    pub fn cursor(&self) -> EventCursor {
        EventCursor::new(self.id)
    }
}

impl FromRow<'_, PgRow> for EventEntity {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let id: i64 = row.try_get(EventIden::Id.to_string().as_str())?;
        let id = id as u64;
        let user_id: i32 = row.try_get(EventIden::User.to_string().as_str())?;
        let user_public_key: String = row.try_get(UserIden::PublicKey.to_string().as_str())?;
        let user_pubkey =
            PublicKey::try_from_z32(&user_public_key).map_err(|e| sqlx::Error::Decode(e.into()))?;
        let event_type_str: String = row.try_get(EventIden::Type.to_string().as_str())?;
        let path: String = row.try_get(EventIden::Path.to_string().as_str())?;
        let path = WebDavPath::new(&path).map_err(|e| sqlx::Error::Decode(e.into()))?;
        let created_at: NaiveDateTime = row.try_get(EventIden::CreatedAt.to_string().as_str())?;

        let content_hash_bytes: Option<Vec<u8>> =
            row.try_get(EventIden::ContentHash.to_string().as_str())?;

        let content_hash = content_hash_bytes.and_then(|bytes| {
            let hash_bytes: [u8; 32] = bytes.try_into().ok()?;
            Some(Hash::from_bytes(hash_bytes))
        });

        let event_type = match event_type_str.as_str() {
            "PUT" => {
                let hash = content_hash.unwrap_or_else(|| {
                    // This should never happen after m20251014 migration runs.
                    tracing::error!(
                        "PUT event {} has NULL content_hash - this indicates a database issue. Using zero hash as fallback.",
                        id
                    );
                    Hash::from_bytes([0; 32])
                });
                EventType::Put { content_hash: hash }
            }
            "DEL" => EventType::Delete,
            other => {
                return Err(sqlx::Error::Decode(
                    format!("Invalid event type: {}", other).into(),
                ))
            }
        };

        Ok(EventEntity {
            id,
            event_type,
            user_id,
            user_pubkey: user_pubkey.clone(),
            path: EntryPath::new(user_pubkey, path),
            created_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pubky_common::crypto::Keypair;

    /// Test that EventEntity can be serialized to JSON and deserialized back.
    /// This is critical for the pg_notify payload to survive the roundtrip.
    #[test]
    fn test_event_entity_serde_roundtrip() {
        let keypair = Keypair::random();
        let pubkey = keypair.public_key();
        let path = EntryPath::new(pubkey.clone(), WebDavPath::new("/pub/test.txt").unwrap());

        let event = EventEntity {
            id: 12345,
            user_id: 42,
            user_pubkey: pubkey,
            event_type: EventType::Put {
                content_hash: Hash::from_bytes([1; 32]),
            },
            path,
            created_at: NaiveDateTime::parse_from_str("2024-01-15 10:30:00", "%Y-%m-%d %H:%M:%S")
                .unwrap(),
        };

        // Serialize to JSON (what pg_notify does)
        let json = serde_json::to_string(&event).expect("Failed to serialize EventEntity");

        // Deserialize back (what PgEventListener does)
        let deserialized: EventEntity =
            serde_json::from_str(&json).expect("Failed to deserialize EventEntity");

        assert_eq!(event, deserialized);
    }

    /// Test that DELETE events also roundtrip correctly.
    #[test]
    fn test_event_entity_serde_roundtrip_delete() {
        let keypair = Keypair::random();
        let pubkey = keypair.public_key();
        let path = EntryPath::new(pubkey.clone(), WebDavPath::new("/pub/deleted.txt").unwrap());

        let event = EventEntity {
            id: 99999,
            user_id: 1,
            user_pubkey: pubkey,
            event_type: EventType::Delete,
            path,
            created_at: NaiveDateTime::parse_from_str("2024-06-20 15:45:30", "%Y-%m-%d %H:%M:%S")
                .unwrap(),
        };

        let json = serde_json::to_string(&event).expect("Failed to serialize");
        let deserialized: EventEntity = serde_json::from_str(&json).expect("Failed to deserialize");

        assert_eq!(event, deserialized);
    }

    /// Test that the JSON payload size is reasonable (pg_notify has 8KB limit).
    #[test]
    fn test_event_entity_json_size_is_reasonable() {
        let keypair = Keypair::random();
        let pubkey = keypair.public_key();
        // Use a reasonably long path to test size
        let path = EntryPath::new(
            pubkey.clone(),
            WebDavPath::new("/pub/some/nested/directory/structure/file.json").unwrap(),
        );

        let event = EventEntity {
            id: u64::MAX, // Worst case for ID size
            user_id: i32::MAX,
            user_pubkey: pubkey,
            event_type: EventType::Put {
                content_hash: Hash::from_bytes([255; 32]),
            },
            path,
            created_at: NaiveDateTime::parse_from_str("2024-12-31 23:59:59", "%Y-%m-%d %H:%M:%S")
                .unwrap(),
        };

        let json = serde_json::to_string(&event).expect("Failed to serialize");

        // Should be well under 8KB (pg_notify limit)
        // Typical size is ~200-400 bytes
        assert!(
            json.len() < 4096,
            "JSON payload size {} exceeds warning threshold",
            json.len()
        );
        assert!(
            json.len() < 8192,
            "JSON payload size {} exceeds pg_notify limit",
            json.len()
        );
    }
}
