use crate::{
    persistence::lmdb::{tables::users::UserQueryError, LmDB},
    shared::{HttpError, HttpResult},
};
use pkarr::PublicKey;

/// Returns an error if the user doesn't exist or is disabled.
pub fn err_if_user_is_invalid(
    pubkey: &PublicKey,
    db: &LmDB,
    err_if_disabled: bool,
) -> HttpResult<()> {
    match db.get_user(pubkey, &db.env.read_txn()?) {
        Ok(user) => {
            if err_if_disabled && user.disabled {
                return Err(HttpError::forbidden());
            }
        }
        Err(UserQueryError::UserNotFound) => {
            return Err(HttpError::not_found());
        }
        Err(UserQueryError::DatabaseError(e)) => {
            return Err(e.into());
        }
    };

    Ok(())
}
