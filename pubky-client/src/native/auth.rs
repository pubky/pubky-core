use anyhow::Result;
use pkarr::PublicKey;

use crate::{Client, NativeClient};

impl NativeClient {
    /// Signs out from a homeserver and clears the local session cookie.
    ///
    /// This method wraps the generic signout logic and adds the native-specific
    /// action of explicitly deleting the session cookie from the custom `CookieJar`.
    pub async fn signout_and_clear_session(&self, pubky: &PublicKey) -> Result<()> {
        // First, call the generic signout method to perform the HTTP DELETE request.
        Client::signout(self, pubky).await?;

        // After the request succeeds, explicitly delete the cookie from the native store.
        self.http.cookie_store.delete_session_after_signout(pubky);

        Ok(())
    }
}
