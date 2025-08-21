use crate::{
    persistence::{lmdb::LmDB, sql::{user::{UserEntity, UserRepository}, SqlDb, UnifiedExecutor}},
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

/// Returns the user if it exists and is not disabled, otherwise returns an error.
/// - User doesn't exist: returns 404
/// - User is disabled: returns 403
pub async fn get_user_or_http_error<'a>(
    pubkey: &PublicKey,
    executor: &mut UnifiedExecutor<'a>,
    err_if_disabled: bool,
) -> HttpResult<UserEntity> {
    let user = match UserRepository::get(pubkey, executor).await {
        Ok(user) => user,
        Err(sqlx::Error::RowNotFound) => {
            tracing::warn!("User {} not found. Forbid access.", pubkey);
            return Err(HttpError::not_found());
        }
        Err(e) => return Err(e.into()),
    };

    if err_if_disabled && user.disabled {
        tracing::warn!("User {} is disabled. Forbid access.", pubkey);
        return Err(HttpError::forbidden_with_message("User is disabled"));
    }

    Ok(user)
}