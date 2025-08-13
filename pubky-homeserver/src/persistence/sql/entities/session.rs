// use pkarr::PublicKey;
// use pubky_common::{capabilities::Capability, crypto::random_bytes};
// use sea_query::{Expr, Iden, Query, SimpleExpr};
// use sqlx::{postgres::PgRow, Executor, FromRow, Row};

// use crate::persistence::sql::{db_connection::DbConnection, entities::user::{UserIden, UserRepository}};

// pub const SESSION_TABLE: &str = "sessions";

// /// Repository that handles all the queries regarding the UserEntity.
// pub struct SessionRepository<'a> {
//     pub db: &'a DbConnection,
// }

// impl<'a> SessionRepository<'a> {

//     /// Create a new repository. This is very lightweight.
//     pub fn new(db: &'a DbConnection) -> Self {
//         Self { db }
//     }

//     /// Create a new user.
//     /// The executor can either be db.pool() or a transaction.
//     pub async fn create<'c, E>(&self, user: &PublicKey, capabilities: &[Capability], executor: E) -> Result<SessionEntity, sqlx::Error>
//     where E: Executor<'c, Database = sqlx::Postgres> {
//         let user_repo = UserRepository::new(self.db);
//         let user = user_repo.get(user, executor).await?;
//         let session_secret = base32::encode(base32::Alphabet::Crockford, &random_bytes::<16>());
//         let statement =
//         Query::insert().into_table(SESSION_TABLE)
//             .columns([SessionIden::Secret, SessionIden::Version, SessionIden::User, SessionIden::Capabilities])
//             .values(vec![
//                 SimpleExpr::Value(session_secret.into()),
//                 SimpleExpr::Value(1.into()),
//                 SimpleExpr::Value(user.id.into()),
//                 SimpleExpr::Value(capabilities.iter().map(|c| c.to_string()).collect::<Vec<String>>().into()),
//             ]).unwrap().returning_all().to_owned();

//         let (query, values) = self.db.build_query(statement);

//         // let user: SessionEntity = SessionEntity executor.fetch_one(sqlx::query_as_with(&query, values)).await?;
//         let user: SessionEntity = sqlx::query_as_with(&query, values).fetch_one(executor).await?;
        

//         Ok(user)
//     }

//     /// Get a user by their public key.
//     /// The executor can either be db.pool() or a transaction.
//     pub async fn get<'c, E>(&self, public_key: &PublicKey, executor: E) -> Result<SessionEntity, sqlx::Error>
//     where E: Executor<'c, Database = sqlx::Postgres> {
//         let statement = Query::select().from(SESSION_TABLE)
//         .columns([UserIden::Id, UserIden::PublicKey, UserIden::CreatedAt, UserIden::Disabled, UserIden::UsedBytes])
//         .and_where(Expr::col(UserIden::PublicKey).eq(public_key.to_string()))
//         .to_owned();
//         let (query, values) = self.db.build_query(statement);
//         let user: SessionEntity = sqlx::query_as_with(&query, values).fetch_one(executor).await?;
//         Ok(user)
//     }

//     pub async fn update<'c, E>(&self, user: &SessionEntity, executor: E) -> Result<SessionEntity, sqlx::Error>
//     where E: Executor<'c, Database = sqlx::Postgres> {
//         let statement = Query::update()
//             .table(SESSION_TABLE)
//             .values(vec![
//                 (UserIden::Disabled, SimpleExpr::Value((user.disabled).into())),
//                 (UserIden::UsedBytes, SimpleExpr::Value((user.used_bytes as i64).into())),
//             ])
//             .and_where(Expr::col(UserIden::Id).eq(user.id))
//             .returning_all()
//             .to_owned();
        
//         let (query, values) = self.db.build_query(statement);
//         let updated_user: SessionEntity = sqlx::query_as_with(&query, values).fetch_one(executor).await?;
//         Ok(updated_user)
//     }

//     /// Delete a user by their public key.
//     /// The executor can either be db.pool() or a transaction.
//     pub async fn delete<'c, E>(&self, user_id: u32, executor: E) -> Result<(), sqlx::Error>
//     where E: Executor<'c, Database = sqlx::Postgres> {
//         let statement = Query::delete()
//             .from_table(SESSION_TABLE)
//             .and_where(Expr::col(UserIden::Id).eq(user_id))
//             .to_owned();
        
//         let (query, values) = self.db.build_query(statement);
//         sqlx::query_with(&query, values).execute(executor).await?;
//         Ok(())
//     }
// }


// #[derive(Iden)]
// enum SessionIden {
//     Id,
//     Secret,
//     Version,
//     User,
//     Capabilities,
//     CreatedAt,
// }

// #[derive(Debug, PartialEq, Eq, Clone)]
// struct SessionEntity {
//     pub id: i32,
//     pub secret: String,
//     pub version: u16,
//     pub user: PublicKey,
//     pub capabilities: Vec<String>,
//     pub created_at: sqlx::types::chrono::NaiveDateTime,
// }

// impl FromRow<'_, PgRow> for SessionEntity {
//     fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
//         let id: i32 = row.try_get(SessionIden::Id.to_string().as_str())?;
//         let secret: String = row.try_get(SessionIden::Secret.to_string().as_str())?;
//         let version: i16 = row.try_get(SessionIden::Version.to_string().as_str())?;
//         let user_raw: String = row.try_get(UserIden::PublicKey.to_string().as_str())?;
//         let user = PublicKey::try_from(user_raw.as_str())
//             .map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
//         let capabilities: Vec<String> = row.try_get(SessionIden::Capabilities.to_string().as_str())?;
//         let created_at: sqlx::types::chrono::NaiveDateTime =
//             row.try_get(SessionIden::CreatedAt.to_string().as_str())?;
//         Ok(SessionEntity {
//             id,
//             secret,
//             version: version as u16,
//             user,
//             capabilities,
//             created_at,
//         })
//     }
// }

// #[cfg(test)]
// mod tests {
//     use pkarr::Keypair;

//     use super::*;

//     #[tokio::test(flavor = "multi_thread")]
//     async fn test_create_get_user() {
//         let db = DbConnection::test().await;
//         let user_repo = SessionRepository::new(&db);
//         let user_pubkey = Keypair::random().public_key();

//         // Test create user
//         let created_user = user_repo.create(&user_pubkey, db.pool()).await.unwrap();
//         assert_eq!(created_user.public_key, user_pubkey);
//         assert_eq!(created_user.disabled, false);
//         assert_eq!(created_user.used_bytes, 0);
//         assert_eq!(created_user.id, 1);

//         // Test get user
//         let user = user_repo.get(&user_pubkey, db.pool()).await.unwrap();
//         assert_eq!(user.public_key, user_pubkey);
//         assert_eq!(user.disabled, false);
//         assert_eq!(user.used_bytes, 0);
//         assert_eq!(user.id, created_user.id);
//     }

//     #[tokio::test(flavor = "multi_thread")]
//     async fn test_create_user_twice() {
//         let db = DbConnection::test().await;
//         let user_repo = SessionRepository::new(&db);
//         let user_pubkey = Keypair::random().public_key();

//         // Test create user
//         let user = user_repo.create(&user_pubkey, db.pool()).await.unwrap();
//         assert_eq!(user.public_key, user_pubkey);
//         assert_eq!(user.disabled, false);
//         assert_eq!(user.used_bytes, 0);

//         user_repo.create(&user_pubkey, db.pool()).await.expect_err("Should fail to create user twice");
//     }

//     #[tokio::test(flavor = "multi_thread")]
//     async fn test_update_user() {
//         let db = DbConnection::test().await;
//         let user_repo = SessionRepository::new(&db);
//         let user_pubkey = Keypair::random().public_key();
//         let mut user = user_repo.create(&user_pubkey, db.pool()).await.unwrap();
        
//         user.used_bytes = 10;
//         user.disabled = true;

//         user_repo.update(&user, db.pool()).await.unwrap();
//         let updated_user = user_repo.get(&user_pubkey, db.pool()).await.unwrap();
//         assert_eq!(updated_user.id, user.id);
//         assert_eq!(updated_user.disabled, true);
//         assert_eq!(updated_user.used_bytes, 10);
//     }

//     #[tokio::test(flavor = "multi_thread")]
//     async fn test_delete_user() {
//         let db = DbConnection::test().await;
//         let user_repo = SessionRepository::new(&db);
//         let user_pubkey = Keypair::random().public_key();

//         // Create a user first
//         let user = user_repo.create(&user_pubkey, db.pool()).await.unwrap();
//         assert_eq!(user.public_key, user_pubkey);

//         // Verify the user exists
//         let retrieved_user = user_repo.get(&user_pubkey, db.pool()).await.unwrap();
//         assert_eq!(retrieved_user.public_key, user_pubkey);

//         // Delete the user
//         user_repo.delete(user.id, db.pool()).await.unwrap();

//         // Verify the user no longer exists
//         let result = user_repo.get(&user_pubkey, db.pool()).await;
//         assert!(result.is_err());
//     }
// }