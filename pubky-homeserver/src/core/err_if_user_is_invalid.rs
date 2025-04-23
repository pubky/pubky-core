use pkarr::PublicKey;
use reqwest::StatusCode;

use crate::persistence::lmdb::{tables::users::UserQueryError, LmDB};

use super::Error;

/// Returns an error if the user doesn't exist or is disabled.
pub fn err_if_user_is_invalid(pubkey: &PublicKey, db: &LmDB) -> super::error::Result<()> {
    match db.get_user(pubkey, &mut db.env.read_txn()?) {
        Ok(user) => {
            if user.disabled {
                return Err(Error::with_status(StatusCode::FORBIDDEN));
            }
        }
        Err(UserQueryError::UserNotFound) => {
            return Err(Error::with_status(StatusCode::NOT_FOUND));
        }
        Err(UserQueryError::DatabaseError(e)) => {
            return Err(e.into());
        }
    };

    Ok(())
}
