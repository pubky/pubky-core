use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use pkarr::PublicKey;
use pkarr_republisher::{MultiRepublishResult, MultiRepublisher, RepublisherSettings, ResilientClientBuilderError};
use tokio::{
    sync::RwLock,
    task::JoinHandle,
    time::{interval, Instant},
};

use crate::core::database::DB;

#[derive(Debug, thiserror::Error)]
pub enum UserKeysRepublisherError {
    #[error(transparent)]
    DB(heed::Error),
    #[error(transparent)]
    Pkarr(ResilientClientBuilderError),
}

/// Publishes the pkarr keys of all users to the Mainline DHT.
#[derive(Debug, Clone)]
pub struct UserKeysRepublisher {
    db: DB,
    handle: Arc<RwLock<Option<JoinHandle<()>>>>,
    is_running: Arc<AtomicBool>,
    republish_interval: Duration,
}

impl UserKeysRepublisher {
    pub fn new(db: DB, republish_interval: Duration) -> Self {
        Self {
            db,
            handle: Arc::new(RwLock::new(None)),
            is_running: Arc::new(AtomicBool::new(false)),
            republish_interval,
        }
    }

    /// Run the user keys republisher.
    pub async fn run(&self) {
        tracing::info!("Initialize user keys republisher...");
        let mut lock = self.handle.write().await;
        if lock.is_some() {
            return;
        }
        let db = self.db.clone();
        let republish_interval = self.republish_interval;
        let handle: JoinHandle<()> =
            tokio::spawn(async move { Self::run_loop(db, republish_interval).await });

        *lock = Some(handle);
        self.is_running.store(true, Ordering::Relaxed);
    }

    // Get all user public keys from the database.
    async fn get_all_user_keys(db: DB) -> Result<Vec<PublicKey>, heed::Error> {
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
    async fn republish_keys_once(db: DB) -> Result<MultiRepublishResult, UserKeysRepublisherError> {
        let keys = Self::get_all_user_keys(db)
            .await
            .map_err(UserKeysRepublisherError::DB)?;
        if keys.is_empty() {
            tracing::info!("No user keys to republish.");
            return Ok(MultiRepublishResult::new(HashMap::new()));
        }
        let mut settings = RepublisherSettings::new();
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
    async fn run_loop(db: DB, republish_interval: Duration) {
        let mut interval = interval(republish_interval);
        loop {
            interval.tick().await;
            let start = Instant::now();
            tracing::info!("Republishing user keys...");
            let result = Self::republish_keys_once(db.clone()).await;
            let elapsed = start.elapsed();
            if let Err(e) = result {
                tracing::error!("Error republishing user keys: {:?}", e);
                continue;
            }
            let result = result.unwrap();
            if result.is_empty() {
                tracing::info!(
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

    /// Stop the user keys republisher.
    #[allow(dead_code)]
    pub async fn stop(&mut self) {
        let mut lock = self.handle.write().await;
        if lock.is_none() {
            // Republisher is not running.
            return;
        }
        let handle = lock.as_ref().unwrap();

        handle.abort();
        *lock = None;
        self.is_running.store(false, Ordering::Relaxed);
    }

    /// Stops the republisher synchronously.
    #[allow(dead_code)]
    pub fn stop_sync(&mut self) {
        let mut lock = self.handle.blocking_write();
        if lock.is_none() {
            // Republisher is not running.
            return;
        }

        let handle = lock.take().unwrap();

        handle.abort();
        *lock = None;
        self.is_running.store(false, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use pkarr::Keypair;
    use tokio::time::Instant;

    use crate::core::{
        database::{tables::users::User, DB},
        user_keys_republisher::UserKeysRepublisher,
    };

    async fn init_db_with_users(count: usize) -> DB {
        let db = DB::test();
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

    /// Test that the republisher stops instantly.
    #[tokio::test]
    async fn start_and_stop() {
        let mut republisher =
            UserKeysRepublisher::new(init_db_with_users(1000).await, Duration::from_secs(1));
        let start = Instant::now();
        republisher.run().await;
        assert!(republisher.handle.read().await.is_some());
        republisher.stop().await;
        let elapsed = start.elapsed();
        assert!(elapsed < Duration::from_secs(1));
        assert!(republisher.handle.read().await.is_none());
    }
}
