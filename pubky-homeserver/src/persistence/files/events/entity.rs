use std::str::FromStr;

use pkarr::PublicKey;
use pubky_common::crypto::Hash;
use sea_query::Iden;
use sqlx::{postgres::PgRow, FromRow, Row};

use crate::{
    persistence::{
        files::events::{
            repository::{Cursor, EventIden},
            EventType,
        },
        sql::user::UserIden,
    },
    shared::webdav::{EntryPath, WebDavPath},
};

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct EventEntity {
    pub id: i64,
    pub user_id: i32,
    pub user_pubkey: PublicKey,
    pub event_type: EventType,
    pub path: EntryPath,
    pub created_at: sqlx::types::chrono::NaiveDateTime,
    pub content_hash: Option<Hash>,
}

impl EventEntity {
    pub fn cursor(&self) -> Cursor {
        Cursor::new(self.id)
    }
}

impl FromRow<'_, PgRow> for EventEntity {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let id: i64 = row.try_get(EventIden::Id.to_string().as_str())?;
        let user_id: i32 = row.try_get(EventIden::User.to_string().as_str())?;
        let user_public_key: String = row.try_get(UserIden::PublicKey.to_string().as_str())?;
        let user_pubkey =
            PublicKey::from_str(&user_public_key).map_err(|e| sqlx::Error::Decode(e.into()))?;
        let event_type: String = row.try_get(EventIden::Type.to_string().as_str())?;
        let event_type =
            EventType::from_str(&event_type).map_err(|e| sqlx::Error::Decode(e.into()))?;
        let user_public_key: String = row.try_get(UserIden::PublicKey.to_string().as_str())?;
        let user_public_key =
            PublicKey::from_str(&user_public_key).map_err(|e| sqlx::Error::Decode(e.into()))?;
        let path: String = row.try_get(EventIden::Path.to_string().as_str())?;
        let path = WebDavPath::new(&path).map_err(|e| sqlx::Error::Decode(e.into()))?;
        let created_at: sqlx::types::chrono::NaiveDateTime =
            row.try_get(EventIden::CreatedAt.to_string().as_str())?;

        // Read optional content_hash
        let content_hash: Option<Vec<u8>> =
            row.try_get(EventIden::ContentHash.to_string().as_str())?;
        let content_hash = content_hash.and_then(|bytes| {
            let hash_bytes: [u8; 32] = bytes.try_into().ok()?;
            Some(Hash::from_bytes(hash_bytes))
        });

        Ok(EventEntity {
            id,
            event_type,
            user_id,
            user_pubkey,
            path: EntryPath::new(user_public_key, path),
            created_at,
            content_hash,
        })
    }
}
