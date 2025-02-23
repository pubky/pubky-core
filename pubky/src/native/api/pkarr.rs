use crate::{native::internal::pkarr::PublishStrategy, Client};
use anyhow::Result;
use pkarr::Keypair;

impl Client {
    /// Republish the user's Pkarr record pointing to their homeserver if
    /// no record can be resolved or if the existing record is older than 4 days.
    /// This method is intended for clients and key managers (e.g., pubky-ring)
    /// to keep the records of active users fresh and available in the DHT and
    /// relays. It is intended to be used only after failed signin due to homeserver
    /// resolution failure. This method is lighter than performing a re-signup into
    /// the last known homeserver, but does not return a session token, so a signin
    /// must be done after republishing. On a failed signin due to homeserver resolution
    /// failure, `pubky-ring` should always republish the last known homeserver.
    ///
    /// # Arguments
    ///
    /// * `keypair` - The keypair associated with the record.
    /// * `host` - The homeserver to publish the record for.
    ///
    /// # Errors
    ///
    /// Returns an error if the publication fails.
    pub async fn republish_homeserver(&self, keypair: &Keypair, host: &str) -> Result<()> {
        self.update_homeserver_record(keypair, Some(host), PublishStrategy::IfOlderThan)
            .await
    }
}
