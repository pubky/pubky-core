use std::{num::NonZeroUsize, time::Duration};

use super::pkarr_republisher::{
    MultiRepublisher, MultiRepublisherError, RepublishSummary, RepublisherSettings,
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
    Pkarr(#[from] MultiRepublisherError),
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
        let summary = match self.republish_impl().await {
            Ok(summary) if summary.is_empty() => return,
            Ok(summary) => summary,
            Err(e) => {
                tracing::error!("Error republishing user keys: {e:?}");
                return;
            }
        };
        let elapsed = start.elapsed();
        Self::log_republish_summary(&summary, elapsed);
    }

    async fn republish_impl(&self) -> Result<RepublishSummary, UserKeysRepublisherError> {
        let keys = self.get_all_user_keys().await?;
        if keys.is_empty() {
            tracing::debug!("No user keys to republish.");
            return Ok(RepublishSummary::default());
        }
        let settings = RepublisherSettings::default();
        let republisher = MultiRepublisher::new(settings, self.pkarr_builder.clone());
        // TODO: Only publish if user points to this home server.
        let pkarr_keys = keys.into_iter().map(Into::into).collect();
        let max_concurrent_workers =
            NonZeroUsize::new(12).expect("worker count should be non-zero");
        Ok(republisher.run(pkarr_keys, max_concurrent_workers).await?)
    }

    async fn get_all_user_keys(&self) -> Result<Vec<PublicKey>, sqlx::Error> {
        let users = UserRepository::get_all(&mut self.db.pool().into()).await?;
        Ok(users.into_iter().map(|user| user.public_key).collect())
    }

    fn log_republish_summary(summary: &RepublishSummary, elapsed: Duration) {
        let total_count = summary.len();
        let elapsed_secs = elapsed.as_secs_f32();
        let success_count = summary.success_count();
        let missing_count = summary.missing_count();
        let failed_count = summary.publishing_failed_count();

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
        let summary = worker.republish_impl().await.unwrap();
        assert_eq!(summary.len(), 10);
        assert_eq!(summary.success_count(), 0);
        assert_eq!(summary.missing_count(), 10);
        assert_eq!(summary.publishing_failed_count(), 0);
    }
}
