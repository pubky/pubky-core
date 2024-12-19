use heed::{
    types::{Bytes, Str},
    Database,
};
use pkarr::PublicKey;
use pubky_common::session::Session;
use tower_cookies::Cookies;

use crate::core::database::DB;

/// session secret => Session.
pub type SessionsTable = Database<Str, Bytes>;

pub const SESSIONS_TABLE: &str = "sessions";

impl DB {
    pub fn get_session(
        &mut self,
        cookies: Cookies,
        public_key: &PublicKey,
    ) -> anyhow::Result<Option<Session>> {
        if let Some(bytes) = self.get_session_bytes(cookies, public_key)? {
            return Ok(Some(Session::deserialize(&bytes)?));
        };

        Ok(None)
    }

    pub fn get_session_bytes(
        &mut self,
        cookies: Cookies,
        public_key: &PublicKey,
    ) -> anyhow::Result<Option<Vec<u8>>> {
        if let Some(cookie) =
            cookies.get(&public_key.to_string().chars().take(8).collect::<String>())
        {
            let rtxn = self.env.read_txn()?;

            let session = self
                .tables
                .sessions
                .get(&rtxn, cookie.value())?
                .map(|s| s.to_vec());

            rtxn.commit()?;

            return Ok(session);
        };

        Ok(None)
    }

    pub fn delete_session(
        &mut self,
        cookies: Cookies,
        public_key: &PublicKey,
    ) -> anyhow::Result<bool> {
        // TODO: Set expired cookie to delete the cookie on client side.

        if let Some(cookie) =
            cookies.get(&public_key.to_string().chars().take(8).collect::<String>())
        {
            let mut wtxn = self.env.write_txn()?;

            let deleted = self.tables.sessions.delete(&mut wtxn, cookie.value())?;

            wtxn.commit()?;

            return Ok(deleted);
        };

        Ok(false)
    }
}
