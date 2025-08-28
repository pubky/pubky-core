use std::{fmt::Display, str::FromStr};

use pkarr::{PublicKey};
use sea_query::{Expr, Iden, PostgresQueryBuilder, Query, SimpleExpr};
use sea_query_binder::SqlxBinder;
use sqlx::{postgres::PgRow, FromRow, Row};

use crate::{persistence::sql::{entities::user::{UserIden, USER_TABLE}, UnifiedExecutor}, shared::webdav::{EntryPath, WebDavPath}};

pub const EVENT_TABLE: &str = "events";

/// Repository that handles all the queries regarding the EventEntity.
pub struct EventRepository;

impl EventRepository {

    /// Create a new event.
    /// The executor can either be db.pool() or a transaction.
    pub async fn create<'a>(user_id: i32, event_type: EventType, path: &WebDavPath, executor: &mut UnifiedExecutor<'a>) -> Result<i64, sqlx::Error> {
        let statement =
        Query::insert().into_table(EVENT_TABLE)
            .columns([EventIden::Type, EventIden::User, EventIden::Path])
            .values(vec![
                SimpleExpr::Value(event_type.to_string().into()),
                SimpleExpr::Value(user_id.into()),
                SimpleExpr::Value(path.as_str().into()),
            ]).expect("Failed to build insert statement").returning_col(EventIden::Id).to_owned();

        let (query, values) = statement.build_sqlx(PostgresQueryBuilder::default());

        let con = executor.get_con().await?;
        let ret_row: PgRow = sqlx::query_with(&query, values).fetch_one(con).await?;
        let event_id: i64 = ret_row.try_get(EventIden::Id.to_string().as_str())?;
        Ok(event_id)
    }

    /// Get a list of events by the cursor. The cursor is the id of the last event in the list.
    /// If you don't to use the cursor, set it to 0.
    /// The limit is the maximum number of events to return.
    /// The executor can either be db.pool() or a transaction.
    pub async fn get_by_cursor<'a>(cursor: i64, limit: u64, executor: &mut UnifiedExecutor<'a>) -> Result<Vec<EventEntity>, sqlx::Error> {
        let statement = Query::select()
        .columns([(EVENT_TABLE, EventIden::Id), (EVENT_TABLE, EventIden::User), (EVENT_TABLE, EventIden::Type), (EVENT_TABLE, EventIden::User), (EVENT_TABLE, EventIden::Path), (EVENT_TABLE, EventIden::CreatedAt)])
        .column((USER_TABLE, UserIden::PublicKey))
        .from(EVENT_TABLE)
        .left_join(USER_TABLE, Expr::col((EVENT_TABLE, EventIden::User)).eq(Expr::col((USER_TABLE, UserIden::Id))))
        .and_where(Expr::col((EVENT_TABLE, EventIden::Id)).gt(cursor))
        .limit(limit)
        .to_owned();
        let (query, values) = statement.build_sqlx(PostgresQueryBuilder::default());
        let con = executor.get_con().await?;
        let events: Vec<EventEntity> = sqlx::query_as_with(&query, values).fetch_all(con).await?;
        Ok(events)
    }
}


#[derive(Iden)]
enum EventIden {
    Id,
    Type,
    User,
    Path,
    CreatedAt,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum EventType {
    Put,
    Delete,
}

impl Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            EventType::Put => "PUT",
            EventType::Delete => "DEL",
        };
        write!(f, "{}", s)
    }
}

impl FromStr for EventType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "PUT" => Ok(EventType::Put),
            "DEL" => Ok(EventType::Delete),
            _ => Err(format!("Failed to parse invalid event type: {}", s)),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct EventEntity {
    pub id: i64,
    pub user_id: i32,
    pub event_type: EventType,
    pub path: EntryPath,
    pub created_at: sqlx::types::chrono::NaiveDateTime,
}

impl FromRow<'_, PgRow> for EventEntity {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let id: i64 = row.try_get(EventIden::Id.to_string().as_str())?;
        let user_id: i32 = row.try_get(EventIden::User.to_string().as_str())?;
        let event_type: String = row.try_get(EventIden::Type.to_string().as_str())?;
        let event_type = EventType::from_str(&event_type).map_err(|e| sqlx::Error::Decode(e.into()))?;
        let user_public_key: String = row.try_get(UserIden::PublicKey.to_string().as_str())?;
        let user_public_key = PublicKey::from_str(&user_public_key).map_err(|e| sqlx::Error::Decode(e.into()))?;
        let path: String = row.try_get(EventIden::Path.to_string().as_str())?;
        let path = WebDavPath::new(&path).map_err(|e| sqlx::Error::Decode(e.into()))?;
        let created_at: sqlx::types::chrono::NaiveDateTime =
            row.try_get(EventIden::CreatedAt.to_string().as_str())?;
        Ok(EventEntity {
            id,
            event_type,
            user_id,
            path: EntryPath::new(user_public_key, path),
            created_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use pkarr::Keypair;

    use crate::persistence::sql::{entities::user::UserRepository, SqlDb};

    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_create_list_event() {
        let db = SqlDb::test().await;
        let user_pubkey = Keypair::random().public_key();

        // Test create user
        let user = UserRepository::create(&user_pubkey, &mut db.pool().into()).await.unwrap();

        // Test create session
        for _ in 0..10 {
            let _ = EventRepository::create(user.id, EventType::Put, &WebDavPath::new("/test").unwrap(), &mut db.pool().into()).await.unwrap();
        }

        // Test get session
        let events = EventRepository::get_by_cursor(5, 4, &mut db.pool().into()).await.unwrap();
        assert_eq!(events.len(), 4);
        assert_eq!(events[0].id, 6);
        assert_eq!(events[0].user_id, user.id);
        assert_eq!(events[0].path, EntryPath::new(user_pubkey, WebDavPath::new("/test").unwrap()));
        assert_eq!(events[0].event_type, EventType::Put);
    }

}