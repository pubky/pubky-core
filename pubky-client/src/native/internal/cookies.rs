//! Cookie store for the native client
//!
//! This is a wrapper around the cookie store that is used to store the session cookies for the native client
//!
//! Because we need to move the session cookies returned by the homeserver to the domain of the user, we need to
//! store them in a custom cookie store.
//! 
//! Maybe we can improve the Pubky design to avoid this hack in the future?

use std::sync::RwLock;

use pkarr::PublicKey;
use reqwest::{cookie::CookieStore, header::HeaderValue, Response};
use url::Url;

const JWT_COOKIE_NAME: &str = "auth_token";

#[derive(Default, Debug)]
pub struct CookieJar {
    pub (crate) store: RwLock<cookie_store::CookieStore>,
}

impl CookieJar {
    /// Stores the session cookies for the given pubky
    /// 
    pub(crate) fn store_session_after_signup(
        &self,
        response: &Response,
        pubky: &PublicKey,
    ) -> Result<(), anyhow::Error> {
        // Extract the session cookies from the response
        let set_cookie_headers = response.headers().iter().filter_map(|(header_name, header_value)| {
            if header_name != "set-cookie" {
                return None;
            };

            Some(header_value)
        });
        let parsed_cookies = set_cookie_headers.filter_map(|header_value| {
            let value_str = match std::str::from_utf8(header_value.as_bytes()) {
                Ok(value) => value,
                Err(_) => return None,
            };
            Some(cookie::Cookie::parse(value_str.to_string())
                        .ok()?
                        .into_owned())
        });
        let session_cookie_names = vec![pubky.to_string(), JWT_COOKIE_NAME.to_string()];
        let session_cookies = parsed_cookies.filter(|cookie| session_cookie_names.contains(&cookie.name().to_string()));

        // Store the session cookies in the cookie jar
        let mut jar = self
            .store
            .write()
            .map_err(|e| anyhow::anyhow!("Failed to lock inner_jar: {e}"))?;
        let domain = format!("_pubky.{pubky}");
        let url = Url::parse(&format!("https://{domain}/signup")).expect("url is always valid");
        for mut cookie in session_cookies {
            tracing::debug!(?cookie, "Storing coookie after signup");
            cookie.set_domain(domain.clone()); // Set them to the domain of the user, not the homeserver
            jar.insert_raw(&cookie, &url)?;
        };

        Ok(())
    }

    /// Deletes all the session cookies for the given pubky
    pub(crate) fn delete_session_after_signout(
        &self,
        pubky: &PublicKey,
    ) -> Result<(), anyhow::Error> {
        let domain = format!("_pubky.{pubky}");
        let cookie_names = vec![pubky.to_string(), JWT_COOKIE_NAME.to_string()];
        let mut jar = self
            .store
            .write()
            .map_err(|e| anyhow::anyhow!("Failed to lock inner_jar: {e}"))?;
        for cookie_name in cookie_names {
            jar.remove(&domain, "/", &cookie_name);
        };

        Ok(())
    }
}


/// Implement the CookieStore trait for the CookieJar
/// 
/// The official implementation is using `unwrap()` all over the place, so we need to do the same here.
/// https://github.com/seanmonstar/reqwest/blob/master/src/cookie.rs#L179
/// 
impl CookieStore for CookieJar {
    fn set_cookies(&self, cookie_headers: &mut dyn Iterator<Item = &HeaderValue>, url: &url::Url) {
        let iter = cookie_headers.filter_map(|val| {
            val.to_str()
                .ok()
                .and_then(|s| cookie::Cookie::parse(s.to_owned()).ok())
        });

        self.store
            .write()
            .expect("Failed to lock inner_jar")
            .store_response_cookies(iter, url);
    }

    fn cookies(&self, url: &url::Url) -> Option<HeaderValue> {
        let s = self
            .store
            .read()
            .expect("Failed to lock inner_jar")
            .get_request_values(url)
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; ");

        if s.is_empty() {
            return None;
        }

        HeaderValue::from_maybe_shared(bytes::Bytes::from(s)).ok()
    }
}
