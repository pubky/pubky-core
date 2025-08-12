use async_trait::async_trait;
use pkarr::PublicKey;
use sea_query::{ColumnDef, Expr, ForeignKey, ForeignKeyAction, Iden, Table};
use sqlx::{postgres::PgRow, FromRow, Row, Transaction};

use crate::persistence::{
    lmdb::tables::users::USERS_TABLE,
    sql::{db_connection::DbConnection, entities::user::UserIden, migration::MigrationTrait},
};

const SIGNUP_CODE_TABLE: &str = "signup_codes";

pub struct M20250812CreateSignupCodeMigration;

#[async_trait]
impl MigrationTrait for M20250812CreateSignupCodeMigration {
    async fn up(
        &self,
        db: &DbConnection,
        tx: &mut Transaction<'static, sqlx::Postgres>,
    ) -> anyhow::Result<()> {
        let statement = Table::create()
            .table(SIGNUP_CODE_TABLE)
            .if_not_exists()
            .col(
                ColumnDef::new(SignupCodeIden::Id)
                    .string_len(14)
                    .not_null()
                    .primary_key(),
            )
            .col(
                ColumnDef::new(SignupCodeIden::CreatedAt)
                    .timestamp()
                    .not_null()
                    .default(Expr::current_timestamp()),
            )
            .col(ColumnDef::new(SignupCodeIden::UsedBy).string_len(52).null())
            .to_owned();
        let query = db.build_schema(statement);
        sqlx::query(query.as_str()).execute(&mut **tx).await?;

        let foreign_key = ForeignKey::create()
            .name("fk_signup_code_used_by")
            .from(SIGNUP_CODE_TABLE, SignupCodeIden::UsedBy)
            .to(USERS_TABLE, UserIden::Id)
            .on_delete(ForeignKeyAction::Cascade)
            .to_owned();

        let query = db.build_schema(foreign_key);
        sqlx::query(query.as_str()).execute(&mut **tx).await?;

        Ok(())
    }

    fn name(&self) -> &str {
        "m20250812_create_signup_code"
    }
}

#[derive(Iden)]
enum SignupCodeIden {
    Id,
    CreatedAt,
    UsedBy,
}

#[derive(Debug, PartialEq, Eq, Clone)]
struct SignupCodeEntity {
    pub id: String,
    pub created_at: sqlx::types::chrono::NaiveDateTime,
    pub used_by: Option<PublicKey>,
}

impl FromRow<'_, PgRow> for SignupCodeEntity {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let token: String = row.try_get(SignupCodeIden::Id.to_string().as_str())?;
        let created_at: sqlx::types::chrono::NaiveDateTime =
            row.try_get(SignupCodeIden::CreatedAt.to_string().as_str())?;
        let used_by_raw: Option<String> =
            row.try_get(SignupCodeIden::UsedBy.to_string().as_str())?;
        let used_by = used_by_raw
            .map(|s| PublicKey::try_from(s.as_str()).map_err(|e| sqlx::Error::Decode(Box::new(e))))
            .transpose()?;
        Ok(SignupCodeEntity {
            id: token,
            created_at,
            used_by,
        })
    }
}

#[cfg(test)]
mod tests {
    use pkarr::Keypair;
    use sea_query::{Query, SimpleExpr};

    use crate::persistence::sql::{migrations::M20250806CreateUserMigration, migrator::Migrator};

    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_create_user_migration() {
        let db = DbConnection::test_without_migrations().await;
        let migrator = Migrator::new(&db);
        migrator
            .run_migrations(vec![
                Box::new(M20250806CreateUserMigration),
                Box::new(M20250812CreateSignupCodeMigration),
            ])
            .await
            .expect("Should run successfully");

        // Create a user
        let pubkey = Keypair::random().public_key();
        let statement = Query::insert()
            .into_table(USERS_TABLE)
            .columns([UserIden::Id])
            .values(vec![SimpleExpr::Value(pubkey.to_string().into())])
            .unwrap()
            .to_owned();
        let (query, values) = db.build_query(statement);
        sqlx::query_with(query.as_str(), values)
            .execute(db.pool())
            .await
            .unwrap();

        // Create a signup code
        let statement = Query::insert()
            .into_table(SIGNUP_CODE_TABLE)
            .columns([SignupCodeIden::Id])
            .values(vec![SimpleExpr::Value("JZY0-D6MY-ZFNG".into())])
            .unwrap()
            .to_owned();
        let (query, values) = db.build_query(statement);
        sqlx::query_with(query.as_str(), values)
            .execute(db.pool())
            .await
            .unwrap();

        // Read signup code
        let statement = Query::select()
            .from(SIGNUP_CODE_TABLE)
            .columns([
                SignupCodeIden::Id,
                SignupCodeIden::CreatedAt,
                SignupCodeIden::UsedBy,
            ])
            .to_owned();
        let (query, _) = db.build_query(statement);
        let code: SignupCodeEntity = sqlx::query_as(query.as_str())
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(code.id, "JZY0-D6MY-ZFNG");
        assert_eq!(code.used_by, None);

        // Use signup code
        let statement = Query::update()
            .table(SIGNUP_CODE_TABLE)
            .values(vec![
                (
                    SignupCodeIden::UsedBy,
                    SimpleExpr::Value(pubkey.to_string().into()),
                ),
            ])
            .and_where(Expr::col(SignupCodeIden::Id).eq(code.id))
            .to_owned();
        let (query, values) = db.build_query(statement);
        sqlx::query_with(query.as_str(), values)
            .execute(db.pool())
            .await
            .unwrap();

        // Read signup code again
        let statement = Query::select()
            .from(SIGNUP_CODE_TABLE)
            .columns([
                SignupCodeIden::Id,
                SignupCodeIden::CreatedAt,
                SignupCodeIden::UsedBy,
            ])
            .to_owned();
        let (query, _) = db.build_query(statement);
        let code: SignupCodeEntity = sqlx::query_as(query.as_str())
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(code.id, "JZY0-D6MY-ZFNG");
        assert_eq!(code.used_by, Some(pubkey.clone()));

        // Check foreign key delete cascade
        let statement = Query::delete()
            .from_table(USERS_TABLE)
            .and_where(Expr::col(UserIden::Id).eq(pubkey.to_string()))
            .to_owned();
        let (query, values) = db.build_query(statement);
        sqlx::query_with(query.as_str(), values)
            .execute(db.pool())
            .await
            .unwrap();

        // Read signup code again. Should be deleted.
        let statement = Query::select()
            .from(SIGNUP_CODE_TABLE)
            .columns([
                SignupCodeIden::Id,
                SignupCodeIden::CreatedAt,
                SignupCodeIden::UsedBy,
            ])
            .to_owned();
        let (query, _) = db.build_query(statement);
        sqlx::query(query.as_str())
            .fetch_one(db.pool())
            .await
            .expect_err("Signup code should be deleted");
    }
}
