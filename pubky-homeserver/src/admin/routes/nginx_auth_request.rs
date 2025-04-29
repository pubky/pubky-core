use super::super::app_state::AppState;
use crate::shared::{HttpError, HttpResult, PubkyHost};
use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
};



/// Delete a single entry from the database.
pub async fn nginx_auth_request(
    State(mut state): State<AppState>,
    pubky: PubkyHost,
) -> HttpResult<impl IntoResponse> {
    Ok((StatusCode::NO_CONTENT, ()))
}

// #[cfg(test)]
// mod tests {
//     use crate::persistence::lmdb::LmDB;

//     use super::*;
//     use axum::{Router, routing::get};
//     use pkarr::{Keypair, PublicKey};
//     use pubky_common::{capabilities::Capability, crypto::random_bytes, session::Session};
//     use tokio::net::TcpListener;
//     use std::{net::SocketAddr, sync::Arc};
//     use base32::{encode as base32_encode, Alphabet};
//     use reqwest::{Client, cookie::Jar, Url};
//     use axum_extra::extract::cookie::CookieJar;

//     // Helper to spawn the app on a random port
//     async fn spawn_app() -> (SocketAddr, LmDB) {
//         let db = LmDB::test();
//         let state = AppState::new(db.clone());
//         let app = Router::new()
//             .route("/nginx_auth_request", get(nginx_auth_request)).with_state(state);
//         let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
//         let addr = listener.local_addr().unwrap();
//         tokio::spawn(async move {
//             axum::serve(listener, app).await.unwrap();
//         });
//         (addr, db)
//     }

//     fn create_session(db: &LmDB) -> (PublicKey, String) {
//         let user_pubkey = Keypair::random().public_key();
//         let session_secret = base32_encode(Alphabet::Crockford, &random_bytes::<16>());
//         let session = Session::new(
//             &user_pubkey,
//             &vec![Capability::try_from("/pub/pubky.app/:rw").unwrap()],
//             None,
//         )
//         .serialize();
        
//         let mut wtxn = db.env.write_txn().unwrap();
//         db
//             .tables
//             .sessions
//             .put(&mut wtxn, &session_secret, &session)
//             .unwrap();
//         wtxn.commit().unwrap();

//         (user_pubkey, session_secret)
//     }

//     #[tokio::test]
//     async fn returns_200_for_valid_session() {
//         let (addr, db)   = spawn_app().await;
//         let url = format!("http://{}/extract_pubky_from_session_cookie", addr);
//         let url_parsed = Url::parse(&url).unwrap();
//         let jar = Arc::new(Jar::default());
//         // This value should be a valid session cookie for your AuthToken logic
//         let (user_pubkey, session_secret) = create_session(&db);
//         jar.add_cookie_str(&format!("session={}", session_secret), &url_parsed);
//         let client = Client::builder().cookie_provider(jar.clone()).build().unwrap();
//         let res = client
//             .get(url)
//             .send()
//             .await
//             .unwrap();
//         assert_eq!(res.status(), reqwest::StatusCode::UNAUTHORIZED);
//         assert!(res.headers().get("x-user-id").is_none());
//     }

//     #[tokio::test]
//     async fn returns_401_for_invalid_session_but_valid_cookie() {
//         let (addr, db) = spawn_app().await;   
//         let url = format!("http://{}/extract_pubky_from_session_cookie", addr);
//         let url_parsed = Url::parse(&url).unwrap();
//         let jar = Arc::new(Jar::default());
//         // This value should be a valid session cookie for your AuthToken logic
//         let (_, session_secret) = create_session(&db);   
//         jar.add_cookie_str(&format!("session={}", session_secret), &url_parsed);
//         let client = Client::builder().cookie_provider(jar.clone()).build().unwrap();
//         let res = client
//             .get(url)
//             .send()
//             .await
//             .unwrap();
//         assert_eq!(res.status(), reqwest::StatusCode::UNAUTHORIZED);
//         assert!(res.headers().get("x-user-id").is_none());
//     }

//     #[tokio::test]
//     async fn returns_401_for_missing_cookie() {
//         let (addr, _) = spawn_app().await;
//         let client = Client::new();
//         let res = client
//             .get(&format!("http://{}/extract_pubky_from_session_cookie", addr))
//             .send()
//             .await
//             .unwrap();
//         assert_eq!(res.status(), reqwest::StatusCode::UNAUTHORIZED);
//     }
// }

