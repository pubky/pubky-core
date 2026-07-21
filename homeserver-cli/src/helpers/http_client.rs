use anyhow::{bail, Context, Result};
use reqwest::blocking::{Client, RequestBuilder, Response};
use serde::Serialize;
use std::time::Duration;
use url::Url;

pub enum Auth {
    #[allow(dead_code)] // used by unauthenticated commands (upcoming)
    None,
    AdminPassword(String),
}

pub struct HttpClient {
    http: Client,
    base_url: Url,
    auth: Auth,
}

impl HttpClient {
    pub fn new(base_url: Url, auth: Auth) -> Result<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(concat!(
                env!("CARGO_PKG_NAME"),
                "/",
                env!("CARGO_PKG_VERSION")
            ))
            .build()
            .context("failed to build HTTP client")?;
        Ok(Self {
            http,
            base_url,
            auth,
        })
    }

    pub fn get(&self, path: &str) -> Result<Response> {
        let url = self.url(path)?;
        self.send(self.http.get(url.clone()), &url)
    }

    pub fn post_json<B: Serialize>(&self, path: &str, body: &B) -> Result<Response> {
        let url = self.url(path)?;
        self.send(self.http.post(url.clone()).json(body), &url)
    }

    #[allow(dead_code)] // used by unauthenticated commands (upcoming)
    pub fn post(&self, path: &str) -> Result<Response> {
        let url = self.url(path)?;
        self.send(self.http.post(url.clone()), &url)
    }

    fn url(&self, path: &str) -> Result<Url> {
        self.base_url
            .join(path)
            .with_context(|| format!("invalid path: {path}"))
    }

    fn send(&self, request: RequestBuilder, url: &Url) -> Result<Response> {
        let response = self
            .apply_auth(request)
            .send()
            .with_context(|| format!("request to {url} failed"))?;

        if !response.status().is_success() {
            bail!("{} returned {}", url, response.status());
        }
        Ok(response)
    }

    fn apply_auth(&self, request: RequestBuilder) -> RequestBuilder {
        match &self.auth {
            Auth::None => request,
            Auth::AdminPassword(password) => request.header("X-Admin-Password", password),
        }
    }
}
