use crate::{
    persistence::lmdb::LmDB,
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
        Ok(Some(user)) => {
            if err_if_disabled && user.disabled {
                tracing::warn!("User {} is disabled. Forbid access.", pubkey);
                Err(HttpError::forbidden_with_message("User is disabled"))
            } else {
                Ok(())
            }
        }
        Ok(None) => {
            tracing::warn!("User {} not found. Forbid access.", pubkey);
            Err(HttpError::not_found())
        }
        Err(e) => Err(e.into()),
    }
}
