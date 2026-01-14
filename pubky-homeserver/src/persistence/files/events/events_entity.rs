use pubky_common::crypto::Hash;
use pubky_common::crypto::PublicKey;
use sea_query::Iden;
use sqlx::{postgres::PgRow, FromRow, Row};

use crate::{
    persistence::{
        files::events::{
            events_repository::{EventCursor, EventIden},
            EventType,
        },
        sql::user::UserIden,
    },
    shared::webdav::{EntryPath, WebDavPath},
};

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct EventEntity {
    pub id: u64,
    pub user_id: i32,
    pub user_pubkey: PublicKey,
    pub event_type: EventType,
    pub path: EntryPath,
    pub created_at: sqlx::types::chrono::NaiveDateTime,
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
        let user_public_key =
            PublicKey::try_from_z32(&user_public_key).map_err(|e| sqlx::Error::Decode(e.into()))?;
        let path: String = row.try_get(EventIden::Path.to_string().as_str())?;
        let path = WebDavPath::new(&path).map_err(|e| sqlx::Error::Decode(e.into()))?;
        let created_at: sqlx::types::chrono::NaiveDateTime =
            row.try_get(EventIden::CreatedAt.to_string().as_str())?;

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
            user_pubkey,
            path: EntryPath::new(user_public_key, path),
            created_at,
        })
    }
}
