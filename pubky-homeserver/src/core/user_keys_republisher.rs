use std::{collections::HashMap, time::Duration};

use pkarr::PublicKey;
use pkarr_republisher::{
    MultiRepublishResult, MultiRepublisher, RepublisherSettings, ResilientClientBuilderError,
};
use tokio::{
    task::JoinHandle,
    time::{interval, Instant},
};

use crate::{app_context::AppContext, persistence::{sql::{user::UserRepository, SqlDb}}};

#[derive(Debug, thiserror::Error)]
pub(crate) enum UserKeysRepublisherError {
    #[error(transparent)]
    DB(sqlx::Error),
    #[error(transparent)]
    Pkarr(ResilientClientBuilderError),
}

const MIN_REPUBLISH_INTERVAL: Duration = Duration::from_secs(30 * 60);

/// Publishes the pkarr keys of all users to the Mainline DHT.
pub(crate) struct UserKeysRepublisher {
    handle: Option<JoinHandle<()>>,
}

impl UserKeysRepublisher {
    /// Run the user keys republisher with an initial delay.
    pub fn start_delayed(context: &AppContext, initial_delay: Duration) -> Self {
        let db = context.sql_db.clone();
        let is_disabled = context.config_toml.pkdns.user_keys_republisher_interval == 0;
        if is_disabled {
            tracing::info!("User keys republisher is disabled.");
            return Self { handle: None };
        }
        let mut republish_interval =
            Duration::from_secs(context.config_toml.pkdns.user_keys_republisher_interval);
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
            initial_delay
        );

        if republish_interval < Duration::from_secs(60 * 60) {
            tracing::warn!(
                "User keys republisher interval is less than 60min. This is strongly discouraged "
            );
        }

        let mut pkarr_builder = context.pkarr_builder.clone();
        pkarr_builder.no_relays(); // Disable relays to avoid their rate limiting.
        let handle = tokio::spawn(async move {
            tokio::time::sleep(initial_delay).await;
            Self::run_loop(&db, republish_interval, pkarr_builder).await
        });
        Self {
            handle: Some(handle),
        }
    }

    // Get all user public keys from the database.
    async fn get_all_user_keys(db: &SqlDb) -> Result<Vec<PublicKey>, sqlx::Error> {
        let users = UserRepository::get_all(&mut db.pool().into()).await?;

        let keys: Vec<PublicKey> = users
            .into_iter()
            .map(|user| user.public_key)
            .collect();
        Ok(keys)
    }

    /// Republishes all user pkarr keys to the Mainline DHT once.
    ///
    /// # Errors
    ///
    /// - If the database cannot be read, an error is returned.
    /// - If the pkarr keys cannot be republished, an error is returned.
    async fn republish_keys_once(
        db: &SqlDb,
        pkarr_builder: pkarr::ClientBuilder,
    ) -> Result<MultiRepublishResult, UserKeysRepublisherError> {
        let keys = Self::get_all_user_keys(db)
            .await
            .map_err(UserKeysRepublisherError::DB)?;
        if keys.is_empty() {
            tracing::debug!("No user keys to republish.");
            return Ok(MultiRepublishResult::new(HashMap::new()));
        }
        let mut settings = RepublisherSettings::default();
        settings.republish_condition(|_| true);
        let republisher = MultiRepublisher::new_with_settings(settings, Some(pkarr_builder));
        // TODO: Only publish if user points to this home server.
        let results = republisher
            .run(keys, 12)
            .await
            .map_err(UserKeysRepublisherError::Pkarr)?;
        Ok(results)
    }

    /// Internal run loop that publishes all user pkarr keys to the Mainline DHT continuously.
    async fn run_loop(db: &SqlDb, republish_interval: Duration, pkarr_builder: pkarr::ClientBuilder) {
        let mut interval = interval(republish_interval);
        loop {
            interval.tick().await;
            let start = Instant::now();
            tracing::debug!("Republishing user keys...");
            let result = match Self::republish_keys_once(db, pkarr_builder.clone()).await {
                Ok(result) => result,
                Err(e) => {
                    tracing::error!("Error republishing user keys: {:?}", e);
                    continue;
                }
            };
            let elapsed = start.elapsed();
            if result.is_empty() {
                continue;
            }
            if result.missing().is_empty() {
                tracing::debug!(
                    "Republished {} user keys within {:.1}s. {} success, {} missing, {} failed.",
                    result.len(),
                    elapsed.as_secs_f32(),
                    result.success().len(),
                    result.missing().len(),
                    result.publishing_failed().len()
                );
            } else {
                tracing::warn!(
                    "Republished {} user keys within {:.1}s. {} success, {} missing, {} failed.",
                    result.len(),
                    elapsed.as_secs_f32(),
                    result.success().len(),
                    result.missing().len(),
                    result.publishing_failed().len()
                );
            }
        }
    }
}

impl Drop for UserKeysRepublisher {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::core::user_keys_republisher::UserKeysRepublisher;
    use crate::persistence::sql::user::UserRepository;
    use crate::persistence::sql::SqlDb;
    use pkarr::Keypair;

    async fn init_db_with_users(count: usize) -> SqlDb {
        let db = SqlDb::test().await;
        for _ in 0..count {
            let public_key = Keypair::random().public_key();
            UserRepository::create(&public_key, &mut db.pool().into()).await.unwrap();
        }
        db
    }

    /// Test that the republisher tries to republish all keys passed.
    #[tokio::test(flavor = "multi_thread")]
    async fn test_republish_keys_once() {
        let db = init_db_with_users(10).await;
        let pkarr_builder = pkarr::ClientBuilder::default();
        let result = UserKeysRepublisher::republish_keys_once(&db, pkarr_builder)
            .await
            .unwrap();
        assert_eq!(result.len(), 10);
        assert_eq!(result.success().len(), 0);
        assert_eq!(result.missing().len(), 10);
        assert_eq!(result.publishing_failed().len(), 0);
    }
}
