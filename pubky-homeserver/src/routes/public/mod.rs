use axum::http::StatusCode;
use pkarr::PublicKey;
use tower_cookies::Cookies;

use crate::{
    error::{Error, Result},
    server::AppState,
};

pub mod read;
pub mod write;

/// Authorize write (PUT or DELETE) for Public paths.
fn authorize(
    state: &mut AppState,
    cookies: Cookies,
    public_key: &PublicKey,
    path: &str,
) -> Result<()> {
    // TODO: can we move this logic to the extractor or a layer
    // to perform this validation?
    let session = state
        .db
        .get_session(cookies, public_key)?
        .ok_or(Error::with_status(StatusCode::UNAUTHORIZED))?;

    if session.pubky() == public_key
        && session.capabilities().iter().any(|cap| {
            path.starts_with(&cap.scope[1..])
                && cap
                    .actions
                    .contains(&pubky_common::capabilities::Action::Write)
        })
    {
        return Ok(());
    }

    Err(Error::with_status(StatusCode::FORBIDDEN))
}

fn verify(path: &str) -> Result<()> {
    if !path.starts_with("pub/") {
        return Err(Error::new(
            StatusCode::FORBIDDEN,
            "Writing to directories other than '/pub/' is forbidden".into(),
        ));
    }

    // TODO: should we forbid paths ending with `/`?

    Ok(())
}
