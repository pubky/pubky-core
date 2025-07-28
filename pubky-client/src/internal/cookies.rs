use reqwest::{cookie::CookieStore, header::HeaderValue};
use std::{collections::HashMap, sync::RwLock};

use pkarr::PublicKey;

use crate::cross_debug;

#[derive(Default, Debug)]
pub struct CookieJar {
    /// A special store for Pubky session cookies, keyed by the Pkarr domain.
    /// This is necessary because a single Pkarr identity can be served from
    /// multiple, changing hosts, which a standard cookie store cannot handle.
    pubky_sessions: RwLock<HashMap<String, String>>,
    /// A standard cookie store for all other (e.g., ICANN) domains.
    normal_jar: RwLock<cookie_store::CookieStore>,
}

impl CookieJar {
    /// Explicitly deletes a Pubky session cookie after signing out.
    /// This method is called by `NativeClient::signout`, as signing out
    /// is an explicit action not tied to a specific HTTP response header.
    pub(crate) fn delete_session_after_signout(&self, pubky: &PublicKey) {
        let domain_key = format!("_pubky.{}", pubky);
        if self
            .pubky_sessions
            .write()
            .unwrap()
            .remove(&domain_key)
            .is_some()
        {
            cross_debug!("Deleted session cookie for {}", domain_key);
        }
    }
}

impl CookieStore for CookieJar {
    /// Sets cookies from response headers. This implementation includes special logic
    /// to intercept and store session cookies for Pkarr domains.
    fn set_cookies(&self, cookie_headers: &mut dyn Iterator<Item = &HeaderValue>, url: &url::Url) {
        let headers: Vec<_> = cookie_headers.collect();

        // Check if the request was made to a Pkarr domain.
        if let Some(host) = url.host_str() {
            if let Ok(pubky) = PublicKey::try_from(host) {
                let session_cookie_name = pubky.to_string();

                // Iterate through the headers to find our special session cookie.
                for header_value in &headers {
                    if let Ok(cookie_str) = header_value.to_str() {
                        if let Ok(cookie) = cookie::Cookie::parse(cookie_str.to_owned()) {
                            // If we find a cookie whose name matches the public key,
                            // store it in our special `pubky_sessions` map.
                            if cookie.name() == session_cookie_name {
                                let domain_key = format!("_pubky.{}", pubky);
                                cross_debug!("Storing special session cookie for {}", domain_key);
                                self.pubky_sessions
                                    .write()
                                    .unwrap()
                                    .insert(domain_key, cookie.value().to_string());
                            }
                        }
                    }
                }
            }
        }

        // Delegate all cookies to the standard store for normal processing.
        // It will handle standard domain matching, expiration, etc.
        let iter = headers.into_iter();
        self.normal_jar.write().unwrap().store_response_cookies(
            iter.filter_map(|val| {
                val.to_str()
                    .ok()
                    .and_then(|s| cookie::Cookie::parse(s.to_owned()).ok())
            }),
            url,
        );
    }

    /// Provides cookies for outgoing requests.
    fn cookies(&self, url: &url::Url) -> Option<HeaderValue> {
        // First, check if the request is going to a Pkarr domain.
        if let Some(host) = url.host_str() {
            if let Ok(public_key) = PublicKey::try_from(host) {
                // If so, check our special session store for a matching cookie.
                if let Some(secret) = self.pubky_sessions.read().unwrap().get(host) {
                    let cookie_value = format!("{}={}", public_key, secret);
                    return HeaderValue::from_str(&cookie_value).ok();
                }
            }
        }

        // If not a Pkarr domain or no special session found, delegate to the standard jar.
        let s = self
            .normal_jar
            .read()
            .unwrap()
            .get_request_values(url)
            .map(|(name, value)| format!("{}={}", name, value))
            .collect::<Vec<_>>()
            .join("; ");

        if s.is_empty() {
            None
        } else {
            HeaderValue::from_str(&s).ok()
        }
    }
}
