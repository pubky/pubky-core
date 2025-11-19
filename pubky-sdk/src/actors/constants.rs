/// Default HTTP relay base when none is supplied.
///
/// The per-flow channel segment is appended automatically as:
/// `base + base64url(hash(client_secret))`.
///
/// A trailing slash on `base` is optional; we normalize paths.
///
/// # Examples
/// Override the relay while reusing an existing client from `Pubky`:
/// ```no_run
/// # use pubky::{Capabilities, Pubky, PubkyAuthFlow};
/// # async fn run() -> pubky::Result<()> {
/// let pubky = Pubky::new()?;
/// let caps = Capabilities::builder().read("pub/example.com/").finish();
/// let flow = PubkyAuthFlow::builder(&caps)
///     .client(pubky.client().clone())
///     .relay(url::Url::parse("http://localhost:8080/link/")?)
///     .start()?;
/// # Ok(()) }
/// ```
pub const DEFAULT_HTTP_RELAY: &str = "https://httprelay.pubky.app/link/";