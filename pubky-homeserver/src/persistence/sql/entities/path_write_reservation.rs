use sea_query::{Expr, Iden, PostgresQueryBuilder, Query, SimpleExpr};
use sea_query_binder::SqlxBinder;
use sqlx::Row;

use crate::{
    persistence::sql::{user::UserRepository, SqlDb, UnifiedExecutor},
    shared::webdav::EntryPath,
};

pub const PATH_WRITE_RESERVATION_TABLE: &str = "path_write_reservations";
// Stale cleanup is a fallback for process crashes and runtime shutdown. Normal
// close, abort, and writer drop paths release reservations promptly.
const STALE_RESERVATION_INTERVAL: &str = "24 hours";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathWriteReservation {
    pub id: i64,
    pub user_id: i32,
    pub path: String,
}

pub struct PathWriteReservationRepository;

impl PathWriteReservationRepository {
    /// Reserve an app-facing write path. Returns `Ok(None)` when an active
    /// reservation or committed entry would create a file/folder collision.
    pub async fn reserve(
        db: &SqlDb,
        path: &EntryPath,
    ) -> Result<Option<PathWriteReservation>, sqlx::Error> {
        let mut tx = db.pool().begin().await?;
        let mut executor = UnifiedExecutor::from_tx(&mut tx);
        let user = UserRepository::get_for_update(path.pubkey(), &mut executor).await?;

        Self::delete_stale_for_user(user.id, &mut executor).await?;

        if Self::has_active_write_collision(user.id, path.path().as_str(), &mut executor).await? {
            drop(executor);
            tx.rollback().await?;
            return Ok(None);
        }

        let reservation = Self::insert(user.id, path.path().as_str(), &mut executor).await?;
        drop(executor);
        tx.commit().await?;
        Ok(Some(reservation))
    }

    pub async fn release(db: &SqlDb, reservation_id: i64) -> Result<(), sqlx::Error> {
        let statement = Query::delete()
            .from_table(PATH_WRITE_RESERVATION_TABLE)
            .and_where(Expr::col(PathWriteReservationIden::Id).eq(reservation_id))
            .to_owned();
        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);
        sqlx::query_with(&query, values).execute(db.pool()).await?;
        Ok(())
    }

    async fn insert<'a>(
        user_id: i32,
        path: &str,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<PathWriteReservation, sqlx::Error> {
        let statement = Query::insert()
            .into_table(PATH_WRITE_RESERVATION_TABLE)
            .columns([
                PathWriteReservationIden::User,
                PathWriteReservationIden::Path,
            ])
            .values(vec![
                SimpleExpr::Value(user_id.into()),
                SimpleExpr::Value(path.into()),
            ])
            .expect("Failed to build insert statement")
            .returning_all()
            .to_owned();
        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);
        let row = sqlx::query_with(&query, values)
            .fetch_one(executor.get_con().await?)
            .await?;

        Ok(PathWriteReservation {
            id: row.try_get(PathWriteReservationIden::Id.to_string().as_str())?,
            user_id: row.try_get(PathWriteReservationIden::User.to_string().as_str())?,
            path: row.try_get(PathWriteReservationIden::Path.to_string().as_str())?,
        })
    }

    async fn delete_stale_for_user<'a>(
        user_id: i32,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            DELETE FROM path_write_reservations
            WHERE "user" = $1
              AND created_at < NOW() - $2::interval
            "#,
        )
        .bind(user_id)
        .bind(STALE_RESERVATION_INTERVAL)
        .execute(executor.get_con().await?)
        .await?;
        Ok(())
    }

    async fn has_active_write_collision<'a>(
        user_id: i32,
        path: &str,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<bool, sqlx::Error> {
        let descendant_prefix = if path.ends_with('/') {
            path.to_string()
        } else {
            format!("{path}/")
        };
        let ancestor_paths = Self::ancestor_file_paths(path);

        let con = executor.get_con().await?;
        sqlx::query_scalar(
            r#"
            SELECT EXISTS (
                SELECT 1
                FROM entries
                WHERE "user" = $1
                  AND (
                    (
                      path <> $2
                      AND substr(path, 1, length($3)) = $3
                    )
                    OR path = ANY($4::text[])
                  )
                UNION ALL
                SELECT 1
                FROM path_write_reservations
                WHERE "user" = $1
                  AND (
                    path = $2
                    OR (
                      path <> $2
                      AND substr(path, 1, length($3)) = $3
                    )
                    OR path = ANY($4::text[])
                  )
            )
            "#,
        )
        .bind(user_id)
        .bind(path)
        .bind(descendant_prefix)
        .bind(ancestor_paths)
        .fetch_one(con)
        .await
    }

    fn ancestor_file_paths(path: &str) -> Vec<String> {
        let path = path.trim_end_matches('/');
        if path.is_empty() || path == "/" {
            return Vec::new();
        }

        path.char_indices()
            .filter_map(|(idx, ch)| {
                if ch == '/' && idx > 0 {
                    Some(path[..idx].to_string())
                } else {
                    None
                }
            })
            .collect()
    }

    #[cfg(test)]
    pub async fn create_for_test(
        user_id: i32,
        path: &str,
        executor: &mut UnifiedExecutor<'_>,
    ) -> Result<PathWriteReservation, sqlx::Error> {
        Self::insert(user_id, path, executor).await
    }
}

#[derive(Iden)]
enum PathWriteReservationIden {
    Id,
    User,
    Path,
}

#[cfg(test)]
mod tests {
    use pubky_common::crypto::Keypair;

    use super::*;
    use crate::{
        persistence::sql::{entry::EntryRepository, user::UserRepository, SqlDb},
        shared::webdav::WebDavPath,
    };

    async fn create_user(db: &SqlDb) -> (pubky_common::crypto::PublicKey, i32) {
        let pubkey = Keypair::random().public_key();
        let user = UserRepository::create(&pubkey, &mut db.pool().into())
            .await
            .unwrap();
        (pubkey, user.id)
    }

    async fn create_entry_for_path(db: &SqlDb, user_id: i32, path: &str) {
        EntryRepository::create(
            user_id,
            &WebDavPath::new(path).unwrap(),
            &pubky_common::crypto::Hash::from_bytes([0; 32]),
            100,
            "text/plain",
            &mut db.pool().into(),
        )
        .await
        .unwrap();
    }

    async fn create_reservation_for_path(db: &SqlDb, user_id: i32, path: &str) -> i64 {
        let reservation =
            PathWriteReservationRepository::create_for_test(user_id, path, &mut db.pool().into())
                .await
                .unwrap();
        reservation.id
    }

    fn entry_path(pubkey: pubky_common::crypto::PublicKey, path: &str) -> EntryPath {
        EntryPath::new(pubkey, WebDavPath::new(path).unwrap())
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_reservation_rejects_committed_descendant() {
        let db = SqlDb::test().await;
        let (pubkey, user_id) = create_user(&db).await;
        create_entry_for_path(&db, user_id, "/test/sub1/1.txt").await;

        let reservation =
            PathWriteReservationRepository::reserve(&db, &entry_path(pubkey, "/test/sub1"))
                .await
                .unwrap();

        assert!(reservation.is_none());
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_reservation_rejects_committed_ancestor() {
        let db = SqlDb::test().await;
        let (pubkey, user_id) = create_user(&db).await;
        create_entry_for_path(&db, user_id, "/test/sub1").await;

        let reservation =
            PathWriteReservationRepository::reserve(&db, &entry_path(pubkey, "/test/sub1/1.txt"))
                .await
                .unwrap();

        assert!(reservation.is_none());
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_reservation_allows_exact_committed_overwrite() {
        let db = SqlDb::test().await;
        let (pubkey, user_id) = create_user(&db).await;
        create_entry_for_path(&db, user_id, "/test/sub1").await;

        let reservation =
            PathWriteReservationRepository::reserve(&db, &entry_path(pubkey, "/test/sub1"))
                .await
                .unwrap();

        assert!(reservation.is_some());
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_reservation_rejects_active_exact_same_path() {
        let db = SqlDb::test().await;
        let (pubkey, user_id) = create_user(&db).await;
        create_reservation_for_path(&db, user_id, "/test/sub1").await;

        let reservation =
            PathWriteReservationRepository::reserve(&db, &entry_path(pubkey, "/test/sub1"))
                .await
                .unwrap();

        assert!(reservation.is_none());
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_reservation_rejects_active_descendant() {
        let db = SqlDb::test().await;
        let (pubkey, user_id) = create_user(&db).await;
        create_reservation_for_path(&db, user_id, "/test/sub1/1.txt").await;

        let reservation =
            PathWriteReservationRepository::reserve(&db, &entry_path(pubkey, "/test/sub1"))
                .await
                .unwrap();

        assert!(reservation.is_none());
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_reservation_rejects_active_ancestor() {
        let db = SqlDb::test().await;
        let (pubkey, user_id) = create_user(&db).await;
        create_reservation_for_path(&db, user_id, "/test/sub1").await;

        let reservation =
            PathWriteReservationRepository::reserve(&db, &entry_path(pubkey, "/test/sub1/1.txt"))
                .await
                .unwrap();

        assert!(reservation.is_none());
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_reservation_does_not_match_siblings() {
        let db = SqlDb::test().await;
        let (pubkey, user_id) = create_user(&db).await;
        create_entry_for_path(&db, user_id, "/test/sub11/file.txt").await;
        create_reservation_for_path(&db, user_id, "/test/sub12/file.txt").await;

        let reservation =
            PathWriteReservationRepository::reserve(&db, &entry_path(pubkey, "/test/sub1"))
                .await
                .unwrap();

        assert!(reservation.is_some());
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_reservation_is_scoped_to_user() {
        let db = SqlDb::test().await;
        let (_user_a_pubkey, user_a_id) = create_user(&db).await;
        let (user_b_pubkey, _user_b_id) = create_user(&db).await;
        create_entry_for_path(&db, user_a_id, "/test/sub1").await;
        create_reservation_for_path(&db, user_a_id, "/test/sub2").await;

        let reservation = PathWriteReservationRepository::reserve(
            &db,
            &entry_path(user_b_pubkey, "/test/sub1/1.txt"),
        )
        .await
        .unwrap();

        assert!(reservation.is_some());
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_reservation_ignores_and_cleans_stale_reservations() {
        let db = SqlDb::test().await;
        let (pubkey, user_id) = create_user(&db).await;
        let stale_id = create_reservation_for_path(&db, user_id, "/test/sub1").await;

        sqlx::query(
            r#"
            UPDATE path_write_reservations
            SET created_at = NOW() - INTERVAL '25 hours'
            WHERE id = $1
            "#,
        )
        .bind(stale_id)
        .execute(db.pool())
        .await
        .unwrap();

        let reservation =
            PathWriteReservationRepository::reserve(&db, &entry_path(pubkey, "/test/sub1"))
                .await
                .unwrap();

        assert!(reservation.is_some());
        let stale_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM path_write_reservations WHERE id = $1)",
        )
        .bind(stale_id)
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert!(!stale_exists);
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_released_reservation_no_longer_blocks() {
        let db = SqlDb::test().await;
        let (pubkey, _user_id) = create_user(&db).await;
        let reservation =
            PathWriteReservationRepository::reserve(&db, &entry_path(pubkey.clone(), "/test/sub1"))
                .await
                .unwrap()
                .unwrap();

        PathWriteReservationRepository::release(&db, reservation.id)
            .await
            .unwrap();
        let reservation =
            PathWriteReservationRepository::reserve(&db, &entry_path(pubkey, "/test/sub1"))
                .await
                .unwrap();

        assert!(reservation.is_some());
    }
}
