use std::{collections::HashMap, time::Duration};

use super::pkarr_republisher::{
    MultiRepublishResult, MultiRepublisher, RepublisherSettings, ResilientClientBuilderError,
};
use pubky_common::crypto::PublicKey;
use tokio::{
    task::JoinHandle,
    time::{interval, Instant},
};

use crate::persistence::sql::{user::UserRepository, SqlDb};

const MIN_REPUBLISH_INTERVAL: Duration = Duration::from_secs(30 * 60);

#[derive(Debug, thiserror::Error)]
pub(crate) enum UserKeysRepublisherError {
    #[error(transparent)]
    DB(#[from] sqlx::Error),
    #[error(transparent)]
    Pkarr(#[from] ResilientClientBuilderError),
}

/// Publishes the pkarr keys of all users to the Mainline DHT.
pub(crate) struct UserKeysRepublisherJob {
    handle: JoinHandle<()>,
}

impl UserKeysRepublisherJob {
    const INITIAL_DELAY_BEFORE_REPUBLISH: Duration = Duration::from_secs(60);

    /// Run the user keys republisher with an initial delay.
    pub fn start(
        db: SqlDb,
        pkarr_builder: pkarr::ClientBuilder,
        mut republish_interval: Duration,
    ) -> Option<Self> {
        if republish_interval.is_zero() {
            tracing::info!("User keys republisher is disabled.");
            return None;
        }
        if republish_interval < MIN_REPUBLISH_INTERVAL {
            tracing::warn!(
                "The configured user keys republisher interval is less than {}s. To avoid spamming the Mainline DHT, the value is set to {}s.",
                MIN_REPUBLISH_INTERVAL.as_secs(),
                MIN_REPUBLISH_INTERVAL.as_secs()
            );
            republish_interval = MIN_REPUBLISH_INTERVAL;
        }
        tracing::info!(
            "Initialize user keys republisher with an interval of {:?} and an initial delay of {:?}",
            republish_interval,
            Self::INITIAL_DELAY_BEFORE_REPUBLISH
        );

        if republish_interval < Duration::from_secs(60 * 60) {
            tracing::warn!(
                "User keys republisher interval is less than 60min. This is strongly discouraged "
            );
        }

        let republisher = UserKeysRepublisher { db, pkarr_builder };
        let handle = tokio::spawn(async move {
            tokio::time::sleep(Self::INITIAL_DELAY_BEFORE_REPUBLISH).await;
            let mut interval = interval(republish_interval);
            loop {
                interval.tick().await;
                republisher.republish().await;
            }
        });
        Some(Self { handle })
    }
}

impl Drop for UserKeysRepublisherJob {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

struct UserKeysRepublisher {
    db: SqlDb,
    pkarr_builder: pkarr::ClientBuilder,
}

impl UserKeysRepublisher {
    async fn republish(&self) {
        let start = Instant::now();
        tracing::debug!("Republishing user keys...");
        let result = match self.republish_impl().await {
            Ok(result) if result.is_empty() => return,
            Ok(result) => result,
            Err(e) => {
                tracing::error!("Error republishing user keys: {e:?}");
                return;
            }
        };
        let elapsed = start.elapsed();
        Self::log_republish_result(&result, elapsed);
    }

    async fn republish_impl(&self) -> Result<MultiRepublishResult, UserKeysRepublisherError> {
        let keys = self.get_all_user_keys().await?;
        if keys.is_empty() {
            tracing::debug!("No user keys to republish.");
            return Ok(MultiRepublishResult::new(HashMap::new()));
        }
        let settings = RepublisherSettings::default();
        let republisher = MultiRepublisher::new_with_settings(settings, self.pkarr_builder.clone());
        // TODO: Only publish if user points to this home server.
        let pkarr_keys = keys.into_iter().map(Into::into).collect();
        Ok(republisher.run(pkarr_keys, 12).await?)
    }

    async fn get_all_user_keys(&self) -> Result<Vec<PublicKey>, sqlx::Error> {
        let users = UserRepository::get_all(&mut self.db.pool().into()).await?;
        Ok(users.into_iter().map(|user| user.public_key).collect())
    }

    fn log_republish_result(result: &MultiRepublishResult, elapsed: Duration) {
        let total_count = result.len();
        let elapsed_secs = elapsed.as_secs_f32();
        let success_count = result.success_count();
        let missing_count = result.missing_count();
        let failed_count = result.publishing_failed_count();

        if missing_count == 0 {
            tracing::debug!(
                "Republished {total_count} user keys within {elapsed_secs:.1}s. {success_count} success, {missing_count} missing, {failed_count} failed.",
            );
        } else {
            tracing::warn!(
                "Republished {total_count} user keys within {elapsed_secs:.1}s. {success_count} success, {missing_count} missing, {failed_count} failed.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::persistence::sql::user::UserRepository;
    use crate::persistence::sql::SqlDb;
    use crate::republishers::user_keys_republisher::UserKeysRepublisher;
    use pubky_common::crypto::Keypair;

    async fn init_db_with_users(count: usize) -> SqlDb {
        let db = SqlDb::test().await;
        for _ in 0..count {
            let public_key = Keypair::random().public_key();
            UserRepository::create(&public_key, &mut db.pool().into())
                .await
                .unwrap();
        }
        db
    }

    /// Test that the republisher tries to republish all keys passed.
    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_republish_keys_once() {
        let db = init_db_with_users(10).await;
        let pkarr_builder = pkarr::ClientBuilder::default();
        let worker = UserKeysRepublisher { db, pkarr_builder };
        let result = worker.republish_impl().await.unwrap();
        assert_eq!(result.len(), 10);
        assert_eq!(result.success_count(), 0);
        assert_eq!(result.missing_count(), 10);
        assert_eq!(result.publishing_failed_count(), 0);
    }
}
