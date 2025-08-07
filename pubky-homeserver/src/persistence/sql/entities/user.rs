use pkarr::PublicKey;
use sea_query::{Iden, Query, SimpleExpr};
use sqlx::{any::AnyRow, Executor, FromRow, Row};
use sea_query_binder::SqlxBinder;

use crate::persistence::sql::db_connection::DbConnection;

pub struct UserRepository;

impl UserRepository {

    pub async fn create_user<'c, E>(&self, public_key: PublicKey, executor: E) -> Result<(), sqlx::Error>
    where E: Executor<'c, Database = sqlx::Any> {
        let (query, values) = Query::insert().into_table(UserIden::Table)
            .columns([UserIden::Id])
            .values(vec![
                SimpleExpr::Value(public_key.to_string().into()),
            ]).unwrap()
            .build_any_sqlx(self.db.schema_builder());

        let user = sqlx::query_with(&query, values)
            .fetch_one(executor)
            .await?;

        Ok(())
    }
}

#[derive(Iden)]
enum UserIden {
    Table,
    Id,
    CreatedAt,
    Disabled,
    UsedBytes,
}

#[derive(Debug, PartialEq, Eq, Clone)]
struct User {
    pub id: PublicKey,
    pub created_at: sqlx::types::chrono::NaiveDateTime,
    pub disabled: bool,
    pub used_bytes: u64,
}

impl FromRow<'_, AnyRow> for User {
    fn from_row(row: &AnyRow) -> Result<Self, sqlx::Error> {
        let id_name = UserIden::Id.to_string();
        let raw_pubkey: String = row.try_get(id_name.as_str())?;
        let id = PublicKey::try_from(raw_pubkey.as_str()).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        let raw_disabled: i64 = row.try_get(UserIden::Disabled.to_string().as_str())?;
        let disabled = raw_disabled != 0;
        let raw_used_bytes: i64 = row.try_get(UserIden::UsedBytes.to_string().as_str())?;
        let used_bytes = raw_used_bytes as u64;
        let raw_created_at: String = row.try_get(UserIden::CreatedAt.to_string().as_str())?;
        let created_at = sqlx::types::chrono::NaiveDateTime::parse_from_str(&raw_created_at, "%Y-%m-%d %H:%M:%S%.f").map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        Ok(User {
            id,
            created_at,
            disabled,
            used_bytes,
        })
    }
}