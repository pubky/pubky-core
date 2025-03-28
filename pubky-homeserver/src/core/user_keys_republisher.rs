use std::{
    collections::HashMap,
    time::Duration,
};

use pkarr::PublicKey;
use pkarr_republisher::{
    MultiRepublishResult, MultiRepublisher, RepublisherSettings, ResilientClientBuilderError,
};
use tokio::{
    task::JoinHandle,
    time::{interval, Instant},
};

use crate::{app_context::AppContext, persistence::lmdb::LmDB};

#[derive(Debug, thiserror::Error)]
pub(crate) enum UserKeysRepublisherError {
    #[error(transparent)]
    DB(heed::Error),
    #[error(transparent)]
    Pkarr(ResilientClientBuilderError),
}

/// Publishes the pkarr keys of all users to the Mainline DHT.
pub(crate) struct UserKeysRepublisher {
    handle: Option<JoinHandle<()>>,
}

impl UserKeysRepublisher {
    /// Run the user keys republisher with an initial delay.
    pub fn run_delayed(context: &AppContext, initial_delay: Duration) -> Self {
        let db = context.db.clone();
        let is_disabled = context.config_toml.pkdns.user_keys_republisher_interval == 0;
        if is_disabled {
            tracing::info!("User keys republisher is disabled.");
            return Self {
                handle: None,
            };
        }
        let republish_interval = Duration::from_secs(context.config_toml.pkdns.user_keys_republisher_interval);
        tracing::info!(
            "Initialize user keys republisher with interval {:?}",
            republish_interval
        );
        let handle = tokio::spawn(async move {
            tokio::time::sleep(initial_delay).await;
            Self::run_loop(db, republish_interval).await
        });
        Self {
            handle: Some(handle),
        }
    }

    // Get all user public keys from the database.
    async fn get_all_user_keys(db: LmDB) -> Result<Vec<PublicKey>, heed::Error> {
        let rtxn = db.env.read_txn()?;
        let users = db.tables.users.iter(&rtxn)?;

        let keys: Vec<PublicKey> = users
            .map(|result| result.map(|val| val.0))
            .filter_map(Result::ok) // Errors: Db corruption or out of memory. For this use case, we just ignore it.
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
        db: LmDB,
    ) -> Result<MultiRepublishResult, UserKeysRepublisherError> {
        let keys = Self::get_all_user_keys(db)
            .await
            .map_err(UserKeysRepublisherError::DB)?;
        if keys.is_empty() {
            tracing::info!("No user keys to republish.");
            return Ok(MultiRepublishResult::new(HashMap::new()));
        }
        let mut settings = RepublisherSettings::default();
        settings.republish_condition(|_| true);
        let republisher = MultiRepublisher::new_with_settings(settings, None);
        // TODO: Only publish if user points to this home server.
        let results = republisher
            .run(keys, 12)
            .await
            .map_err(UserKeysRepublisherError::Pkarr)?;
        Ok(results)
    }

    /// Internal run loop that publishes all user pkarr keys to the Mainline DHT continuously.
    async fn run_loop(db: LmDB, republish_interval: Duration) {
        let mut interval = interval(republish_interval);
        loop {
            interval.tick().await;
            let start = Instant::now();
            tracing::info!("Republishing user keys...");
            let result = match Self::republish_keys_once(db.clone()).await {
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
                tracing::info!(
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
    use pkarr::Keypair;
    use crate::core::user_keys_republisher::UserKeysRepublisher;
    use crate::persistence::lmdb::tables::users::User;
    use crate::persistence::lmdb::LmDB;

    async fn init_db_with_users(count: usize) -> LmDB {
        let db = LmDB::test();
        let mut wtxn = db.env.write_txn().unwrap();
        for _ in 0..count {
            let user = User::new();
            let public_key = Keypair::random().public_key();
            db.tables.users.put(&mut wtxn, &public_key, &user).unwrap();
        }
        wtxn.commit().unwrap();
        db
    }

    /// Test that the republisher tries to republish all keys passed.
    #[tokio::test]
    async fn test_republish_keys_once() {
        let db = init_db_with_users(10).await;
        let result = UserKeysRepublisher::republish_keys_once(db).await.unwrap();
        assert_eq!(result.len(), 10);
        assert_eq!(result.success().len(), 0);
        assert_eq!(result.missing().len(), 10);
        assert_eq!(result.publishing_failed().len(), 0);
    }
}
