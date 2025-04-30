use super::super::app_state::AppState;
use crate::shared::{HttpError, HttpResult, PubkyHost};
use axum::{
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode},
    response::IntoResponse,
};
use tower_cookies::Cookies;

/// Auth endpoint for nginx to authenticate requests and extract the user pubkey.
/// Using the `auth_request` directive, nginx will forward the request without the body
/// to this endpoint. The homeserver will then extract the session cookie and validate it.
/// If the session is valid, the homeserver will return the user pubkey in the `x-user-id` header.
///
/// Nginx will then use the `auth_request_set` directive to set the `$user_id` variable to the value
/// of the `x-user-id` header.
/// This way, nginx can use the user pubkey for rate limiting or other purposes.
///
/// Usage:
/// ```nginx
/// location /protected/ {
///     auth_request /nginx_auth_request;
///     # Set $user_id from the x-user-id header returned by the homeserver
///     auth_request_set $user_id $upstream_http_x_user_id;
///
///     # Use $user_id for rate limiting
///     limit_req_zone $user_id zone=user_limit:10m rate=1r/s;
///     limit_req zone=user_limit burst=5 nodelay;
/// }
/// ```
pub async fn nginx_auth_request(
    State(state): State<AppState>,
    pubky: PubkyHost,
    cookies: Cookies,
) -> HttpResult<impl IntoResponse> {
    // Extract and validate the session cookie
    let cookie_name = pubky.public_key().to_z32();
    let cookie_value = match cookies.get(&cookie_name) {
        Some(cookie) => cookie.value().to_string(),
        None => {
            return Err(HttpError::new(
                StatusCode::UNAUTHORIZED,
                Some("Session cookie not found"),
            ))
        }
    };
    let session = match state.db.get_session(&cookie_value)? {
        Some(session) => session,
        None => {
            return Err(HttpError::new(
                StatusCode::UNAUTHORIZED,
                Some("Session not found"),
            ))
        }
    };

    // Return the signed user pubkey as a response header
    let user_pubkey = session.pubky();
    let header_value = match HeaderValue::from_str(user_pubkey.to_z32().as_str()) {
        Ok(header_value) => header_value,
        Err(_) => {
            return Err(HttpError::new(
                StatusCode::UNAUTHORIZED,
                Some("Invalid user pubkey"),
            ))
        }
    };
    let mut headers = HeaderMap::new();
    headers.insert("x-user-id", header_value);
    Ok((StatusCode::OK, headers))
}

#[cfg(test)]
mod tests {
    use crate::{persistence::lmdb::LmDB, shared::PubkyHostLayer};

    use super::*;
    use axum::{routing::get, Router};
    use base32::{encode as base32_encode, Alphabet};
    use pkarr::{Keypair, PublicKey};
    use pubky_common::{capabilities::Capability, crypto::random_bytes, session::Session};
    use reqwest::{cookie::Jar, Client, Url};
    use std::{net::SocketAddr, sync::Arc};
    use tokio::net::TcpListener;
    use tower_cookies::CookieManagerLayer;

    // Helper to spawn the app on a random port
    async fn spawn_app() -> (SocketAddr, LmDB) {
        let db = LmDB::test();
        let state = AppState::new(db.clone());
        let app = Router::new()
            .route("/nginx_auth_request", get(nginx_auth_request).layer(PubkyHostLayer))
            .with_state(state)

            .layer(CookieManagerLayer::new());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (addr, db)
    }

    fn create_session(db: &LmDB) -> (PublicKey, String) {
        let user_pubkey = Keypair::random().public_key();
        let session_secret = base32_encode(Alphabet::Crockford, &random_bytes::<16>());
        let session = Session::new(
            &user_pubkey,
            &vec![Capability::try_from("/pub/pubky.app/:rw").unwrap()],
            None,
        )
        .serialize();

        let mut wtxn = db.env.write_txn().unwrap();
        db.tables
            .sessions
            .put(&mut wtxn, &session_secret, &session)
            .unwrap();
        wtxn.commit().unwrap();

        (user_pubkey, session_secret)
    }

    #[tokio::test]
    async fn returns_200_for_valid_session() {
        let (addr, db) = spawn_app().await;
        let url = format!("http://{}/nginx_auth_request", addr);
        let url_parsed = Url::parse(&url).unwrap();
        let jar = Arc::new(Jar::default());
        let (user_pubkey, session_secret) = create_session(&db);
        jar.add_cookie_str(
            &format!("{}={}", user_pubkey.to_z32(), session_secret),
            &url_parsed,
        );
        let client = Client::builder()
            .cookie_provider(jar.clone())
            .build()
            .unwrap();
        let res = client
            .get(url)
            .header("pubky-host", user_pubkey.to_z32().as_str())
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), reqwest::StatusCode::OK);
        let header = res.headers().get("x-user-id");
        assert!(header.is_some(), "x-user-id header should be present");
        assert_eq!(
            header.unwrap().to_str().unwrap(),
            user_pubkey.to_z32().as_str(),
            "x-user-id header should match the user pubkey"
        );
    }

    #[tokio::test]
    async fn returns_401_for_invalid_session_but_valid_cookie() {
        let (addr, _) = spawn_app().await;
        let url = format!("http://{}/nginx_auth_request", addr);
        let url_parsed = Url::parse(&url).unwrap();
        let jar = Arc::new(Jar::default());
        let user_pubkey = Keypair::random().public_key();
        jar.add_cookie_str(&format!("{}=123456", user_pubkey.to_z32()), &url_parsed);
        let client = Client::builder()
            .cookie_provider(jar.clone())
            .build()
            .unwrap();
        let res = client
            .get(url)
            .header("pubky-host", user_pubkey.to_z32().as_str())
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), reqwest::StatusCode::UNAUTHORIZED);
        assert!(res.headers().get("x-user-id").is_none());
    }

    #[tokio::test]
    async fn returns_401_for_missing_cookie() {
        let (addr, _) = spawn_app().await;
        let client = Client::new();
        let user_pubkey = Keypair::random().public_key();
        let res = client
            .get(&format!("http://{}/nginx_auth_request", addr))
            .header("pubky-host", user_pubkey.to_z32().as_str())
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), reqwest::StatusCode::UNAUTHORIZED);
    }
}
