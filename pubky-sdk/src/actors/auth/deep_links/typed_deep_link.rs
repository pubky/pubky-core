#![allow(
    dead_code,
    reason = "Typed deep link parser is experimental until the deep link refactor chooses a direction"
)]

use std::{
    fmt::{self, Display},
    marker::PhantomData,
    str::FromStr,
};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use pubky_common::{auth::jws::ClientId, capabilities::Capabilities, crypto::PublicKey};
use url::Url;

use super::{DeepLinkParseError, schemes::DeepLinkScheme};

/// Intent marker for typed Pubky deep links.
pub trait DeepLinkIntent {
    /// URI host value used as the deep-link intent.
    const NAME: &'static str;
}

/// Typed parameter set for a Pubky deep-link intent.
pub trait DeepLinkParams: Sized {
    /// Parse this parameter set from a URL.
    fn parse(url: &Url) -> Result<Self, DeepLinkParseError>;

    /// Append this parameter set as URL query pairs.
    fn append_query_pairs(&self, url: &mut Url);
}

/// A typed Pubky deep link with a statically selected intent and parameter set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedDeepLink<I, P> {
    scheme: DeepLinkScheme,
    params: P,
    _intent: PhantomData<I>,
}

impl<I, P> TypedDeepLink<I, P> {
    /// Create a typed deep link from a scheme and typed params.
    pub fn new(scheme: DeepLinkScheme, params: P) -> Self {
        Self {
            scheme,
            params,
            _intent: PhantomData,
        }
    }
}

impl<I, P> TypedDeepLink<I, P>
where
    I: DeepLinkIntent,
    P: DeepLinkParams,
{
    /// Parse a typed deep link from a URL.
    pub fn parse_url(url: &Url) -> Result<Self, DeepLinkParseError> {
        let scheme = url.scheme().parse()?;
        if url.host_str().unwrap_or("") != I::NAME {
            return Err(DeepLinkParseError::InvalidIntent(I::NAME));
        }

        Ok(Self {
            scheme,
            params: P::parse(url)?,
            _intent: PhantomData,
        })
    }

    /// Return the validated deep-link scheme.
    pub fn scheme(&self) -> DeepLinkScheme {
        self.scheme
    }

    /// Return the statically selected deep-link intent.
    pub fn intent(&self) -> &'static str {
        I::NAME
    }

    /// Return the typed parameter set for this deep link.
    pub fn params(&self) -> &P {
        &self.params
    }

    /// Convert this typed deep link into a URL.
    pub fn to_url(&self) -> Url {
        let mut url = Url::parse(&format!("{}://{}", self.scheme.as_str(), I::NAME))
            .expect("invariant: deep-link scheme and intent form a valid URL");
        self.params.append_query_pairs(&mut url);
        url
    }
}

impl<I, P> FromStr for TypedDeepLink<I, P>
where
    I: DeepLinkIntent,
    P: DeepLinkParams,
{
    type Err = DeepLinkParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse_url(&Url::parse(value)?)
    }
}

impl<I, P> Display for TypedDeepLink<I, P>
where
    I: DeepLinkIntent,
    P: DeepLinkParams,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_url())
    }
}

impl<I, P> From<TypedDeepLink<I, P>> for Url
where
    I: DeepLinkIntent,
    P: DeepLinkParams,
{
    fn from(value: TypedDeepLink<I, P>) -> Self {
        value.to_url()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SigninIntent;

impl DeepLinkIntent for SigninIntent {
    const NAME: &'static str = "signin";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SignupIntent;

impl DeepLinkIntent for SignupIntent {
    const NAME: &'static str = "signup";
}

pub(super) type TypedSigninDeepLink = TypedDeepLink<SigninIntent, SigninParams>;
pub(super) type TypedSignupDeepLink = TypedDeepLink<SignupIntent, SignupParams>;
pub(super) type TypedSigninGrantDeepLink = TypedDeepLink<SigninIntent, SigninGrantParams>;
pub(super) type TypedSignupGrantDeepLink = TypedDeepLink<SignupIntent, SignupGrantParams>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SigninParams {
    pub(super) capabilities: Capabilities,
    pub(super) relay: Url,
    pub(super) secret: [u8; 32],
}

impl DeepLinkParams for SigninParams {
    fn parse(url: &Url) -> Result<Self, DeepLinkParseError> {
        Ok(Self {
            capabilities: parse_capabilities(url)?,
            relay: parse_relay(url)?,
            secret: parse_secret(url)?,
        })
    }

    fn append_query_pairs(&self, url: &mut Url) {
        append_signin_params(url, &self.capabilities, &self.relay, &self.secret);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SignupParams {
    pub(super) capabilities: Capabilities,
    pub(super) relay: Url,
    pub(super) secret: [u8; 32],
    pub(super) homeserver: PublicKey,
    pub(super) signup_token: Option<String>,
}

impl DeepLinkParams for SignupParams {
    fn parse(url: &Url) -> Result<Self, DeepLinkParseError> {
        Ok(Self {
            capabilities: parse_capabilities(url)?,
            relay: parse_relay(url)?,
            secret: parse_secret(url)?,
            homeserver: parse_homeserver(url)?,
            signup_token: optional_query(url, "st"),
        })
    }

    fn append_query_pairs(&self, url: &mut Url) {
        append_signup_params(
            url,
            &self.capabilities,
            &self.relay,
            &self.secret,
            &self.homeserver,
            self.signup_token.as_deref(),
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SigninGrantParams {
    pub(super) capabilities: Capabilities,
    pub(super) relay: Url,
    pub(super) secret: [u8; 32],
    pub(super) client_id: ClientId,
    pub(super) client_pk: PublicKey,
}

impl DeepLinkParams for SigninGrantParams {
    fn parse(url: &Url) -> Result<Self, DeepLinkParseError> {
        Ok(Self {
            capabilities: parse_capabilities(url)?,
            relay: parse_relay(url)?,
            secret: parse_secret(url)?,
            client_id: parse_client_id(url)?,
            client_pk: parse_client_pk(url)?,
        })
    }

    fn append_query_pairs(&self, url: &mut Url) {
        append_signin_params(url, &self.capabilities, &self.relay, &self.secret);
        append_grant_params(url, &self.client_id, &self.client_pk);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SignupGrantParams {
    pub(super) capabilities: Capabilities,
    pub(super) relay: Url,
    pub(super) secret: [u8; 32],
    pub(super) homeserver: PublicKey,
    pub(super) signup_token: Option<String>,
    pub(super) client_id: ClientId,
    pub(super) client_pk: PublicKey,
}

impl DeepLinkParams for SignupGrantParams {
    fn parse(url: &Url) -> Result<Self, DeepLinkParseError> {
        Ok(Self {
            capabilities: parse_capabilities(url)?,
            relay: parse_relay(url)?,
            secret: parse_secret(url)?,
            homeserver: parse_homeserver(url)?,
            signup_token: optional_query(url, "st"),
            client_id: parse_client_id(url)?,
            client_pk: parse_client_pk(url)?,
        })
    }

    fn append_query_pairs(&self, url: &mut Url) {
        append_signup_params(
            url,
            &self.capabilities,
            &self.relay,
            &self.secret,
            &self.homeserver,
            self.signup_token.as_deref(),
        );
        append_grant_params(url, &self.client_id, &self.client_pk);
    }
}

fn append_signin_params(
    url: &mut Url,
    capabilities: &Capabilities,
    relay: &Url,
    secret: &[u8; 32],
) {
    url.query_pairs_mut()
        .append_pair("caps", &capabilities.to_string())
        .append_pair("relay", relay.as_str())
        .append_pair("secret", &URL_SAFE_NO_PAD.encode(secret));
}

fn append_signup_params(
    url: &mut Url,
    capabilities: &Capabilities,
    relay: &Url,
    secret: &[u8; 32],
    homeserver: &PublicKey,
    signup_token: Option<&str>,
) {
    append_signin_params(url, capabilities, relay, secret);
    let mut query = url.query_pairs_mut();
    query.append_pair("hs", &homeserver.z32());
    if let Some(signup_token) = signup_token {
        query.append_pair("st", signup_token);
    }
}

fn append_grant_params(url: &mut Url, client_id: &ClientId, client_pk: &PublicKey) {
    url.query_pairs_mut()
        .append_pair("cid", &client_id.to_string())
        .append_pair("cpk", &client_pk.z32());
}

fn parse_capabilities(url: &Url) -> Result<Capabilities, DeepLinkParseError> {
    required_query(url, "caps")?
        .as_str()
        .try_into()
        .map_err(|e| DeepLinkParseError::InvalidQueryParameter("caps", Box::new(e)))
}

fn parse_relay(url: &Url) -> Result<Url, DeepLinkParseError> {
    Url::parse(&required_query(url, "relay")?)
        .map_err(|e| DeepLinkParseError::InvalidQueryParameter("relay", Box::new(e)))
}

pub(super) fn parse_secret(url: &Url) -> Result<[u8; 32], DeepLinkParseError> {
    let raw_secret = required_query(url, "secret")?;
    let secret = URL_SAFE_NO_PAD
        .decode(raw_secret.as_str())
        .map_err(|e| DeepLinkParseError::InvalidQueryParameter("secret", Box::new(e)))?;

    secret.try_into().map_err(|e: Vec<u8>| {
        let msg = format!("Expected 32 bytes, got {}", e.len());
        DeepLinkParseError::InvalidQueryParameter(
            "secret",
            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, msg)),
        )
    })
}

fn parse_homeserver(url: &Url) -> Result<PublicKey, DeepLinkParseError> {
    PublicKey::try_from_z32(&required_query(url, "hs")?)
        .map_err(|e| DeepLinkParseError::InvalidQueryParameter("hs", Box::new(e)))
}

fn parse_client_id(url: &Url) -> Result<ClientId, DeepLinkParseError> {
    ClientId::new(&required_query(url, "cid")?)
        .map_err(|e| DeepLinkParseError::InvalidQueryParameter("cid", Box::new(e)))
}

fn parse_client_pk(url: &Url) -> Result<PublicKey, DeepLinkParseError> {
    PublicKey::try_from_z32(&required_query(url, "cpk")?)
        .map_err(|e| DeepLinkParseError::InvalidQueryParameter("cpk", Box::new(e)))
}

fn required_query(url: &Url, key: &'static str) -> Result<String, DeepLinkParseError> {
    optional_query(url, key).ok_or(DeepLinkParseError::MissingQueryParameter(key))
}

fn optional_query(url: &Url, key: &str) -> Option<String> {
    url.query_pairs()
        .find(|(param_key, _)| param_key == key)
        .map(|(_, value)| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pubky_common::crypto::Keypair;

    const SECRET: &str = "kqnceEMgrNQM_xi06oQXjA3cJHX_RQmw1BY6JE1bse8";
    const SECRET_BYTES: [u8; 32] = [
        146, 169, 220, 120, 67, 32, 172, 212, 12, 255, 24, 180, 234, 132, 23, 140, 13, 220, 36,
        117, 255, 69, 9, 176, 212, 22, 58, 36, 77, 91, 177, 239,
    ];
    const CLIENT_ID: &str = "franky.pubky.app";
    const PUBLIC_KEY: &str = "5jsjx1o6fzu6aeeo697r3i5rx15zq41kikcye8wtwdqm4nb4tryo";

    #[test]
    fn parses_signin_deep_link() {
        let deep_link: TypedSigninDeepLink = format!(
            "pubkyauth://signin?caps=/pub/pubky.app/:rw&relay=https://relay.test/inbox&secret={SECRET}"
        )
        .parse()
        .unwrap();

        assert_eq!(deep_link.scheme(), DeepLinkScheme::PubkyAuth);
        assert_eq!(deep_link.intent(), "signin");
        assert_eq!(
            deep_link.params().capabilities.to_string(),
            "/pub/pubky.app/:rw"
        );
        assert_eq!(deep_link.params().relay.as_str(), "https://relay.test/inbox");
        assert_eq!(deep_link.params().secret, SECRET_BYTES);
    }

    #[test]
    fn parses_signup_deep_link() {
        let deep_link: TypedSignupDeepLink = format!(
            "pubkyauth://signup?caps=/pub/pubky.app/:rw&relay=https://relay.test/inbox&secret={SECRET}&hs={PUBLIC_KEY}&st=123"
        )
        .parse()
        .unwrap();

        assert_eq!(deep_link.params().homeserver.z32(), PUBLIC_KEY);
        assert_eq!(deep_link.params().signup_token, Some("123".to_string()));
    }

    #[test]
    fn parses_signin_grant_deep_link() {
        let client_pk = Keypair::random().public_key();
        let deep_link: TypedSigninGrantDeepLink = format!(
            "pubkyauth://signin?caps=/pub/pubky.app/:rw&relay=https://relay.test/inbox&secret={SECRET}&cid={CLIENT_ID}&cpk={}",
            client_pk.z32()
        )
        .parse()
        .unwrap();

        assert_eq!(deep_link.params().client_id.to_string(), CLIENT_ID);
        assert_eq!(deep_link.params().client_pk.z32(), client_pk.z32());
    }

    #[test]
    fn parses_signup_grant_deep_link() {
        let client_pk = Keypair::random().public_key();
        let deep_link: TypedSignupGrantDeepLink = format!(
            "pubkyauth://signup?caps=/pub/pubky.app/:rw&relay=https://relay.test/inbox&secret={SECRET}&hs={PUBLIC_KEY}&st=123&cid={CLIENT_ID}&cpk={}",
            client_pk.z32()
        )
        .parse()
        .unwrap();

        assert_eq!(deep_link.params().homeserver.z32(), PUBLIC_KEY);
        assert_eq!(deep_link.params().signup_token, Some("123".to_string()));
        assert_eq!(deep_link.params().client_id.to_string(), CLIENT_ID);
        assert_eq!(deep_link.params().client_pk.z32(), client_pk.z32());
    }

    #[test]
    fn parses_from_url() {
        let url = Url::parse(&format!(
            "pubkyauth://signin?caps=/:rw&relay=https://relay.test/inbox&secret={SECRET}"
        ))
        .unwrap();
        let deep_link = TypedSigninDeepLink::parse_url(&url).unwrap();

        assert_eq!(deep_link.params().secret, SECRET_BYTES);
    }

    #[test]
    fn converts_signin_deep_link_to_url() {
        let deep_link: TypedSigninDeepLink = format!(
            "pubkyauth://signin?caps=/pub/pubky.app/:rw&relay=https://relay.test/inbox&secret={SECRET}"
        )
        .parse()
        .unwrap();
        let url = deep_link.to_url();
        let parsed_again = TypedSigninDeepLink::parse_url(&url).unwrap();

        assert_eq!(parsed_again, deep_link);
    }

    #[test]
    fn converts_signup_grant_deep_link_to_url() {
        let client_pk = Keypair::random().public_key();
        let deep_link: TypedSignupGrantDeepLink = format!(
            "pubkyauth://signup?caps=/pub/pubky.app/:rw&relay=https://relay.test/inbox&secret={SECRET}&hs={PUBLIC_KEY}&st=123&cid={CLIENT_ID}&cpk={}",
            client_pk.z32()
        )
        .parse()
        .unwrap();
        let url = deep_link.to_url();
        let parsed_again = TypedSignupGrantDeepLink::parse_url(&url).unwrap();

        assert_eq!(parsed_again, deep_link);
    }

    #[test]
    fn rejects_wrong_intent() {
        let error = format!(
            "pubkyauth://signup?caps=/:rw&relay=https://relay.test/inbox&secret={SECRET}"
        )
        .parse::<TypedSigninDeepLink>()
        .unwrap_err();

        assert!(matches!(error, DeepLinkParseError::InvalidIntent("signin")));
    }

    #[test]
    fn rejects_missing_required_param() {
        let error = format!("pubkyauth://signin?relay=https://relay.test/inbox&secret={SECRET}")
            .parse::<TypedSigninDeepLink>()
            .unwrap_err();

        assert!(matches!(
            error,
            DeepLinkParseError::MissingQueryParameter("caps")
        ));
    }

    #[test]
    fn rejects_invalid_typed_param() {
        let error = "pubkyauth://signin?caps=/:rw&relay=not-a-url&secret=abc"
            .parse::<TypedSigninDeepLink>()
            .unwrap_err();

        assert!(matches!(
            error,
            DeepLinkParseError::InvalidQueryParameter("relay", _)
        ));
    }
}
