use crate::{app_context::AppContext, core::HomeserverCore};
use axum::{http::header, Router};
use axum_test::TestServer;
use pkarr::{Keypair, PublicKey};
use pubky_common::{auth::AuthToken, capabilities::Capability};

pub async fn create_test_signup(
    server: &axum_test::TestServer,
    keypair: &Keypair,
) -> anyhow::Result<String> {
    let auth_token = AuthToken::sign(keypair, vec![Capability::root()]);
    let body_bytes: axum::body::Bytes = auth_token.serialize().into();
    let response = server
        .post("/signup")
        .add_header("host", keypair.public_key().to_string())
        .bytes(body_bytes)
        .expect_success()
        .await;

    let header_value = response
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|h| h.to_str().ok())
        .expect("should return a set-cookie header")
        .to_string();

    Ok(header_value)
}

pub async fn create_test_env() -> anyhow::Result<(AppContext, Router, TestServer, PublicKey, String)>
{
    let context = AppContext::test();
    let router = HomeserverCore::create_router(&context);
    let server = axum_test::TestServer::new(router.clone()).unwrap();

    let keypair = Keypair::random();
    let public_key = keypair.public_key();
    let cookie = create_test_signup(&server, &keypair)
        .await
        .unwrap()
        .to_string();

    Ok((context, router, server, public_key, cookie))
}
