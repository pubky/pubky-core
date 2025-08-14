use std::{fmt::Display, str::FromStr};

use sea_query::{Expr, Iden, Query, SimpleExpr};
use sqlx::{postgres::PgRow, Executor, FromRow, Row};
use futures_util::stream::{self, StreamExt};

use crate::{persistence::sql::db_connection::DbConnection, shared::webdav::WebDavPath};

pub const EVENT_TABLE: &str = "events";

/// Repository that handles all the queries regarding the EventEntity.
pub struct EventRepository<'a> {
    pub db: &'a DbConnection,
}

impl<'a> EventRepository<'a> {

    /// Create a new repository. This is very lightweight.
    pub fn new(db: &'a DbConnection) -> Self {
        Self { db }
    }

    /// Create a new event.
    /// The executor can either be db.pool() or a transaction.
    pub async fn create<'c, E>(&self, user_id: i32, event_type: EventType, path: &WebDavPath, executor: E) -> Result<EventEntity, sqlx::Error>
    where E: Executor<'c, Database = sqlx::Postgres> {
        let statement =
        Query::insert().into_table(EVENT_TABLE)
            .columns([EventIden::Type, EventIden::User, EventIden::Path])
            .values(vec![
                SimpleExpr::Value(event_type.to_string().into()),
                SimpleExpr::Value(user_id.into()),
                SimpleExpr::Value(path.as_str().into()),
            ]).expect("Failed to build insert statement").returning_all().to_owned();

        let (query, values) = self.db.build_query(statement);

        let event: EventEntity = sqlx::query_as_with(&query, values).fetch_one(executor).await?;
        Ok(event)
    }

    /// Get a list of events by the cursor. The cursor is the id of the last event in the list.
    /// If you don't to use the cursor, set it to 0.
    /// The limit is the maximum number of events to return.
    /// The executor can either be db.pool() or a transaction.
    pub async fn get_by_cursor<'c, E>(&self, cursor: i64, limit: u64, executor: E) 
    -> Result<Vec<EventEntity>, sqlx::Error>
    where E: Executor<'c, Database = sqlx::Postgres> {
        let statement = Query::select().from(EVENT_TABLE)
        .columns([EventIden::Id, EventIden::Type, EventIden::User, EventIden::Path, EventIden::CreatedAt])
        .and_where(Expr::col(EventIden::Id).gte(cursor))
        .limit(limit)
        .to_owned();
        let (query, values) = self.db.build_query(statement);
        let events: Vec<EventEntity> = sqlx::query_as_with(&query, values).fetch_all(executor).await?;
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
    pub event_type: EventType,
    pub user_id: i32,
    pub path: WebDavPath,
    pub created_at: sqlx::types::chrono::NaiveDateTime,
}

impl FromRow<'_, PgRow> for EventEntity {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let id: i64 = row.try_get(EventIden::Id.to_string().as_str())?;
        let event_type: String = row.try_get(EventIden::Type.to_string().as_str())?;
        let event_type = EventType::from_str(&event_type).map_err(|e| sqlx::Error::Decode(e.into()))?;
        let user_id: i32 = row.try_get(EventIden::User.to_string().as_str())?;
        let path: String = row.try_get(EventIden::Path.to_string().as_str())?;
        let path = WebDavPath::new(&path).map_err(|e| sqlx::Error::Decode(e.into()))?;
        let created_at: sqlx::types::chrono::NaiveDateTime =
            row.try_get(EventIden::CreatedAt.to_string().as_str())?;
        Ok(EventEntity {
            id,
            event_type,
            user_id,
            path,
            created_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use pkarr::Keypair;

    use crate::persistence::sql::entities::user::UserRepository;

    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_create_list_event() {
        let db = DbConnection::test().await;
        let user_repo = UserRepository::new(&db);
        let event_repo = EventRepository::new(&db);
        let user_pubkey = Keypair::random().public_key();

        // Test create user
        let user = user_repo.create(&user_pubkey, db.pool()).await.unwrap();

        // Test create session
        for _ in 0..10 {
            let _ = event_repo.create(user.id, EventType::Put, &WebDavPath::new("/test").unwrap(), db.pool()).await.unwrap();
        }

        // Test get session
        let events = event_repo.get_by_cursor(5, 4, db.pool()).await.unwrap();
        assert_eq!(events.len(), 4);
        assert_eq!(events[0].id, 5);
        assert_eq!(events[0].user_id, user.id);
        assert_eq!(events[0].path, WebDavPath::new("/test").unwrap());
        assert_eq!(events[0].event_type, EventType::Put);
    }

}