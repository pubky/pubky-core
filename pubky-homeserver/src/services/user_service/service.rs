//! Service layer for user operations.

use pubky_common::crypto::PublicKey;

use crate::persistence::sql::user::{UserEntity, UserRepository};
use crate::persistence::sql::{uexecutor, SqlDb, UnifiedExecutor};
use crate::shared::user_quota::{UserQuota, UserQuotaPatch};
use crate::shared::{HttpError, HttpResult};

use super::quota_cache::{CachedEntry, QuotaCache};

/// A rough estimate of the size of the file metadata stored alongside every file.
/// Added to quota accounting so that zero-byte files still count against the quota.
pub const FILE_METADATA_SIZE: u64 = 256;

/// Coordinates user lookups, creation, and storage quota enforcement.
///
/// Wraps the database, quota cache, and system-wide defaults so that
/// callers don't need direct repository access or knowledge of config values.
#[derive(Clone, Debug)]
pub struct UserService {
    /// Database connection pool.
    sql_db: SqlDb,
    /// In-memory TTL cache for resolved per-user quotas.
    quota_cache: QuotaCache,
    /// System-wide default storage quota in MB (`None` = unlimited).
    default_storage_quota_mb: Option<u64>,
}

impl UserService {
    /// Create a new service with its own quota cache.
    pub fn new(sql_db: SqlDb, default_storage_quota_mb: Option<u64>) -> Self {
        let quota_cache = QuotaCache::new();
        Self {
            sql_db,
            quota_cache,
            default_storage_quota_mb,
        }
    }

    /// Access the underlying connection pool.
    pub fn pool(&self) -> &sqlx::PgPool {
        self.sql_db.pool()
    }

    /// Fetch a user with a `FOR UPDATE` row lock within an existing transaction.
    pub async fn get_for_update<'a>(
        &self,
        pubkey: &PublicKey,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<UserEntity, sqlx::Error> {
        UserRepository::get_for_update(pubkey, executor).await
    }

    /// Persist an updated user entity within an existing transaction.
    pub async fn update<'a>(
        &self,
        user: &UserEntity,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<UserEntity, sqlx::Error> {
        UserRepository::update(user, executor).await
    }

    /// Check whether adding `bytes_delta` to the user's current usage would
    /// exceed their effective storage quota.
    pub fn would_exceed_storage_quota(&self, user: &UserEntity, bytes_delta: i64) -> bool {
        let max_bytes = user
            .quota()
            .storage_quota_mb
            .resolve_with_default(self.default_storage_quota_mb)
            .map(|mb| mb.saturating_mul(1024 * 1024));
        Self::exceeds_limit(user.used_bytes, bytes_delta, max_bytes)
    }

    /// Pure arithmetic: does `current + delta` exceed `max`?
    /// `None` max means unlimited (never exceeds).
    fn exceeds_limit(current_bytes: u64, bytes_delta: i64, max_bytes: Option<u64>) -> bool {
        let Some(max) = max_bytes else {
            return false;
        };
        let new_total = current_bytes as i128 + bytes_delta as i128;
        new_total > 0 && new_total > max as i128
    }

    /// Look up a user by public key, returning HTTP-appropriate errors.
    /// - User not found → 404
    /// - User disabled (when `err_if_disabled` is true) → 403
    pub async fn get_or_http_error(
        &self,
        pubkey: &PublicKey,
        err_if_disabled: bool,
    ) -> HttpResult<UserEntity> {
        let user = match UserRepository::get(pubkey, &mut self.sql_db.pool().into()).await {
            Ok(user) => user,
            Err(sqlx::Error::RowNotFound) => {
                tracing::warn!("User {} not found. Forbid access.", pubkey);
                return Err(HttpError::not_found());
            }
            Err(e) => return Err(e.into()),
        };

        if err_if_disabled && user.disabled {
            tracing::warn!("User {} is disabled. Forbid access.", pubkey);
            return Err(HttpError::forbidden_with_message("User is disabled"));
        }

        Ok(user)
    }

    /// Resolve the effective quota for a user, checking the cache first and
    /// falling back to the database on a miss.
    ///
    /// Returns `Ok(Some(config))` for known users, `Ok(None)` for unknown users,
    /// or `Err` if the DB query fails.
    pub async fn resolve_quota(
        &self,
        pubkey: &PublicKey,
    ) -> Result<Option<UserQuota>, sqlx::Error> {
        if let Some(cached) = self.quota_cache.get(pubkey) {
            return Ok(cached);
        }

        // Cache miss or expired — remove stale entry and query DB.
        self.quota_cache.remove(pubkey);
        self.quota_cache.make_room();

        match UserRepository::get(pubkey, &mut self.sql_db.pool().into()).await {
            Ok(user) => {
                let resolved = user.quota();
                self.quota_cache
                    .insert(pubkey.clone(), CachedEntry::found(resolved.clone()));
                Ok(Some(resolved))
            }
            Err(sqlx::Error::RowNotFound) => {
                self.quota_cache
                    .insert(pubkey.clone(), CachedEntry::not_found());
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }

    /// Create a user with explicit quota, commit the transaction, and populate
    /// the cache so downstream layers (rate limiter, etc.) see the user immediately.
    pub async fn create_user(
        &self,
        public_key: &PublicKey,
        quota: &UserQuota,
        tx: sqlx::Transaction<'static, sqlx::Postgres>,
    ) -> HttpResult<UserEntity> {
        let mut tx = tx;
        let user = UserRepository::create(public_key, uexecutor!(tx)).await?;
        let user = UserRepository::set_quota(user.id, quota, uexecutor!(tx)).await?;
        tx.commit().await?;

        // Populate cache so the rate limiter sees the new user immediately
        // (evicts any negative cache entry from pre-signup lookups).
        self.quota_cache
            .insert(public_key.clone(), CachedEntry::found(user.quota()));

        Ok(user)
    }

    /// Apply a partial quota update and evict the cached entry so downstream
    /// layers (rate limiter, etc.) re-resolve from the database.
    pub async fn patch_quota(
        &self,
        pubkey: &PublicKey,
        patch: &UserQuotaPatch,
    ) -> HttpResult<UserEntity> {
        let mut tx = self.sql_db.pool().begin().await?;

        let user = match UserRepository::get_for_update(pubkey, uexecutor!(tx)).await {
            Ok(user) => user,
            Err(sqlx::Error::RowNotFound) => return Err(HttpError::not_found()),
            Err(e) => return Err(e.into()),
        };

        let mut config = user.quota();
        config.merge(patch);

        // Validate the merged config (e.g. burst requires a corresponding rate Value).
        config.validate().map_err(|e| {
            HttpError::new_with_message(axum::http::StatusCode::UNPROCESSABLE_ENTITY, e)
        })?;

        let updated = UserRepository::set_quota(user.id, &config, uexecutor!(tx)).await?;
        tx.commit().await?;

        self.quota_cache.remove(pubkey);
        Ok(updated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_limit_never_exceeds() {
        assert!(!UserService::exceeds_limit(u64::MAX, i64::MAX, None));
    }

    #[test]
    fn exactly_at_limit_does_not_exceed() {
        // 500 current + 500 delta == 1000 limit → not exceeded
        assert!(!UserService::exceeds_limit(500, 500, Some(1000)));
    }

    #[test]
    fn one_byte_over_limit_exceeds() {
        assert!(UserService::exceeds_limit(500, 501, Some(1000)));
    }

    #[test]
    fn negative_delta_shrinks_usage() {
        // 1000 current - 500 delta = 500 → within 1000 limit
        assert!(!UserService::exceeds_limit(1000, -500, Some(1000)));
    }

    #[test]
    fn negative_delta_below_zero_does_not_exceed() {
        // 100 current - 200 delta = -100 → negative total, not exceeded
        assert!(!UserService::exceeds_limit(100, -200, Some(50)));
    }

    #[test]
    fn zero_limit_any_positive_delta_exceeds() {
        assert!(UserService::exceeds_limit(0, 1, Some(0)));
    }

    #[test]
    fn zero_limit_zero_delta_does_not_exceed() {
        assert!(!UserService::exceeds_limit(0, 0, Some(0)));
    }

    #[test]
    fn large_current_with_large_negative_delta() {
        // Ensures i128 promotion handles this without overflow
        assert!(!UserService::exceeds_limit(
            u64::MAX,
            i64::MIN,
            Some(u64::MAX)
        ));
    }

    #[test]
    fn large_current_near_limit() {
        let max = u64::MAX;
        // Exactly at limit
        assert!(!UserService::exceeds_limit(max, 0, Some(max)));
        // One over (via delta=1)
        assert!(UserService::exceeds_limit(max, 1, Some(max)));
    }
}
