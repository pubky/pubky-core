use std::{collections::HashMap, sync::RwLock};

use pkarr::PublicKey;
use reqwest::{cookie::CookieStore, header::HeaderValue, Response};

#[derive(Default, Debug)]
pub struct CookieJar {
    pubky_sessions: RwLock<HashMap<String, String>>,
    normal_jar: RwLock<cookie_store::CookieStore>,
}

impl CookieJar {
    pub(crate) fn store_session_after_signup(&self, response: &Response, pubky: &PublicKey) {
        // let cookie_names = vec![ // Cookie names that we are interested in
        //     pubky.to_string(),
        //     "auth_token".to_string(),
        // ];

        // let session_cookies = response.headers().iter().filter_map(|(header_name, header_value)| {
        //     // Skip if the header name is not set-cookie
        //     if header_name != "set-cookie" {
        //         return None;
        //     };

        //     // Parse the cookie value
        //     let value_str = match std::str::from_utf8(header_value.as_bytes()) {
        //         Ok(value) => value,
        //         Err(_) => return None,
        //     };
        //     let cookie =cookie::Cookie::parse(value_str.to_string()).ok()?;

        //     // Only look at cookies with names we are interested in
        //     if !cookie_names.contains(&cookie.name().to_string()) {
        //         return None;
        //     };
        //     Some(cookie)
        // });

        // for cookie in session_cookies {
        //     let domain = format!("_pubky.{pubky}");
        //     tracing::debug!(?cookie, "Storing coookie after signup");
            
        // }


        for (header_name, header_value) in response.headers() {
            let cookie_name = &pubky.to_string();

            if header_name == "set-cookie"
                && header_value.as_ref().starts_with(cookie_name.as_bytes())
            {
                if let Ok(Ok(cookie)) =
                    std::str::from_utf8(header_value.as_bytes()).map(cookie::Cookie::parse)
                {
                    if cookie.name() == cookie_name {
                        let domain = format!("_pubky.{pubky}");
                        tracing::debug!(?cookie, "Storing coookie after signup");

                        self.pubky_sessions
                            .write()
                            .unwrap()
                            .insert(domain, cookie.value().to_string());
                    }
                };
            }
        }
    }

    pub(crate) fn delete_session_after_signout(&self, pubky: &PublicKey) {
        self.pubky_sessions
            .write()
            .unwrap()
            .remove(&format!("_pubky.{pubky}"));
    }
}

impl CookieStore for CookieJar {
    fn set_cookies(&self, cookie_headers: &mut dyn Iterator<Item = &HeaderValue>, url: &url::Url) {
        let iter = cookie_headers.filter_map(|val| {
            val.to_str()
                .ok()
                .and_then(|s| cookie::Cookie::parse(s.to_owned()).ok())
        });

        self.normal_jar
            .write()
            .unwrap()
            .store_response_cookies(iter, url);
    }

    fn cookies(&self, url: &url::Url) -> Option<HeaderValue> {
        let s = self
            .normal_jar
            .read()
            .unwrap()
            .get_request_values(url)
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; ");

        if s.is_empty() {
            let host = url.host_str().unwrap_or("");

            if let Ok(public_key) = PublicKey::try_from(host) {
                let cookie_name = public_key.to_string();

                return self.pubky_sessions.read().unwrap().get(host).map(|secret| {
                    HeaderValue::try_from(format!("{cookie_name}={secret}")).unwrap()
                });
            }
        }

        HeaderValue::from_maybe_shared(bytes::Bytes::from(s)).ok()
    }
}
