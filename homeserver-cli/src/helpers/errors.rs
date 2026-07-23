use thiserror::Error;
use url::Url;

#[derive(Error, Debug)]
#[error("{url} returned {status}")]
pub struct HttpStatusError {
    pub status: u16,
    pub url: Url,
}

#[derive(Error, Debug)]
pub enum ApiError {
    #[error("user not found")]
    UserNotFound,

    #[error("invalid admin token")]
    InvalidToken,

    #[error("wrong pubky format")]
    WrongPubkyFormat,

    #[allow(dead_code)]
    #[error("server returned {status} for {url}")]
    Unexpected { status: u16, url: String },
}
