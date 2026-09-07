#![allow(deprecated, reason = "Regression covers legacy cookie auth API")]

use pubky::{AuthToken, CookieCredential, PubkyCookieAuthFlow, PubkyHttpClient, PublicKey};

// Compile-time regression: callers can split relay approval receipt from the
// homeserver session exchange, retaining the verified token across retries.
async fn cookie_auth_can_be_completed_in_two_stages(
    flow: PubkyCookieAuthFlow,
    client: &PubkyHttpClient,
) -> pubky::Result<CookieCredential> {
    let homeserver: Option<PublicKey> = flow.target_homeserver();
    let token: AuthToken = flow.await_token().await?;
    CookieCredential::from_auth_token(&token, client, homeserver).await
}

#[test]
fn split_cookie_auth_api_is_public() {
    let _ = cookie_auth_can_be_completed_in_two_stages;
}
