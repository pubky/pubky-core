//! PKDNS (Pkarr) top-level actor: resolve & publish `_pubky` records.
//!
//! - **Read-only (no keys):** `Pkdns::new()`
//! - **Publish (with keys):** `Pkdns::new_with_keypair(..)` or `signer.pkdns()`
//!
//! Reads do not require a session or keys. Publishing requires a `Keypair`.

use std::time::Duration;

use pkarr::{
    Keypair, PublicKey, SignedPacket, Timestamp,
    dns::rdata::{RData, SVCB},
};

use crate::{
    PubkyHttpClient, PubkySigner, cross_log,
    errors::{AuthError, Error, PkarrError, Result},
};

/// Default staleness window for homeserver `_pubky` Pkarr records (1 hour).
///
/// Used by [`crate::Pkdns::publish_homeserver_if_stale`] to decide when a record
/// should be republished. Republish too often and you add DHT churn; too rarely
/// and lookups may not be able to find the user's homeserver.
///
/// You can override this per instance via [`crate::Pkdns::set_stale_after`] (mutable setter).
pub const DEFAULT_STALE_AFTER: Duration = Duration::from_secs(60 * 60);

/// PKDNS actor: resolve & publish `_pubky` PKARR records.
///
/// Construct it **without** a keypair for read-only queries:
/// ```no_run
/// # async fn example() -> pubky::Result<()> {
/// let pkdns = pubky::Pkdns::new()?;
/// if let Some(host) = pkdns.get_homeserver_of(&"o4dk…uyy".try_into().unwrap()).await {
///     println!("homeserver: {host}");
/// }
/// # Ok(()) }
/// ```
///
/// Or **with** a keypair for publishing and self lookups:
/// ```no_run
/// # async fn example(kp: pubky::Keypair) -> pubky::Result<()> {
/// let pkdns = pubky::Pkdns::new_with_keypair(kp)?;
/// // Self-lookup (requires keypair on this Pkdns)
/// let my_host = pkdns.get_homeserver().await?;
/// println!("my homeserver: {my_host:?}");
///
/// // Publish if stale
/// pkdns.publish_homeserver_if_stale(None).await?;
/// # Ok(()) }
/// ```
#[derive(Debug, Clone)]
pub struct Pkdns {
    client: PubkyHttpClient,
    keypair: Option<Keypair>,
    /// Maximum age before a user record should be republished.
    /// Defaults to 1 hour.
    stale_after: Duration,
}

impl PubkySigner {
    /// Get a PKDNS actor bound to this signer's client and keypair (publishing enabled).
    #[inline]
    #[must_use]
    pub fn pkdns(&self) -> Pkdns {
        crate::Pkdns::with_client_and_keypair(self.client.clone(), self.keypair.clone())
    }
}

impl Pkdns {
    /// Construct a read-only PKDNS actor.
    ///
    /// # Errors
    /// - Returns [`crate::errors::Error`] if the underlying [`PubkyHttpClient`] cannot be created.
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: PubkyHttpClient::new()?,
            keypair: None,
            stale_after: DEFAULT_STALE_AFTER,
        })
    }

    /// Construct a publishing-capable PKDNS actor.
    ///
    /// # Errors
    /// - Returns [`crate::errors::Error`] if the underlying [`PubkyHttpClient`] cannot be created.
    pub fn new_with_keypair(keypair: Keypair) -> Result<Self> {
        Ok(Self {
            client: PubkyHttpClient::new()?,
            keypair: Some(keypair),
            stale_after: DEFAULT_STALE_AFTER,
        })
    }

    /// Infallible constructor with client + keypair (publishing enabled).
    /// Used internally for `signer.pkdns()`
    const fn with_client_and_keypair(client: PubkyHttpClient, keypair: Keypair) -> Self {
        Self {
            client,
            keypair: Some(keypair),
            stale_after: DEFAULT_STALE_AFTER,
        }
    }

    /// Create a read-only PKDNS actor bound to a specific client.
    /// No keypair attached; publishing is disabled.
    pub(crate) const fn with_client(client: PubkyHttpClient) -> Self {
        Self {
            client,
            keypair: None,
            stale_after: DEFAULT_STALE_AFTER,
        }
    }

    /// Set how long an existing `_pubky` PKARR record is considered **fresh** (builder-style).
    ///
    /// If the current record’s age is **≤ this duration**, [`Self::publish_homeserver_if_stale`]
    /// is a no-op; otherwise the record is (re)published.
    ///
    /// Defaults to 1 hour [`DEFAULT_STALE_AFTER`].
    ///
    /// # Examples
    /// ```no_run
    /// # use std::time::Duration;
    /// # async fn ex(signer: pubky::PubkySigner) -> pubky::Result<()> {
    /// let pkdns = signer.pkdns()
    ///     .set_stale_after(Duration::from_secs(30 * 60)); // 30 minutes
    ///
    /// // Will re-publish same homeserver only if the existing record is older than 30 minutes.
    /// pkdns.publish_homeserver_if_stale(None).await?;
    /// # Ok(()) }
    /// ```
    #[must_use]
    pub const fn set_stale_after(mut self, d: Duration) -> Self {
        self.stale_after = d;
        self
    }

    // -------------------- Reads --------------------

    /// Resolve current homeserver host for a user public key via Pkarr (no keypair required).
    ///
    /// Returns the `_pubky` SVCB/HTTPS target (domain or pubkey-as-host),
    /// or `None` if the record is missing/unresolvable.
    pub async fn get_homeserver_of(&self, user_public_key: &PublicKey) -> Option<PublicKey> {
        cross_log!(
            info,
            "Resolving homeserver for public key {} via PKARR",
            user_public_key
        );
        let packet = self.client.pkarr().resolve(user_public_key).await?;
        let s = extract_host_from_packet(&packet)?;
        let result = PublicKey::try_from(s).ok();
        cross_log!(
            debug,
            "Homeserver resolution for {} yielded {:?}",
            user_public_key,
            result
        );
        result
    }

    /// Convenience: resolve the homeserver for **this** user (requires keypair on `Pkdns`).
    ///
    /// Returns:
    /// - `Ok(Some(host))` if resolvable,
    /// - `Ok(None)` if no record is found,
    /// - `Err(_)` only for transport errors.
    ///
    /// # Errors
    /// - Returns [`crate::errors::Error::Authentication`] if called without an attached keypair.
    /// - Propagates transport failures from PKARR resolution.
    pub async fn get_homeserver(&self) -> Result<Option<PublicKey>> {
        let kp = self.keypair.as_ref().ok_or_else(|| {
            Error::from(AuthError::Validation(
                "get_homeserver() requires a keypair; use Pkdns::new_with_keypair() or signer.pkdns()".into(),
            ))
        })?;
        Ok(self.get_homeserver_of(&kp.public_key()).await)
    }

    // -------------------- Publishing (requires keypair) --------------------

    /// Publish `_pubky` **forcing** a refresh.
    ///
    /// If `host_override` is `None`, reuses the host found in the existing record (if any).
    ///
    /// # Errors
    /// - [`crate::errors::Error::Authentication`] if called without a keypair or validation fails.
    /// - [`crate::errors::Error::Pkarr`] if PKARR/DHT resolution or publish fails.
    pub async fn publish_homeserver_force(&self, host_override: Option<&PublicKey>) -> Result<()> {
        self.publish_homeserver(host_override, PublishMode::Force)
            .await
    }

    /// Publish `_pubky` **only if stale/missing**.
    ///
    /// If `host_override` is `None`, reuses the host found in the existing record (if any).
    ///
    /// # Errors
    /// - [`crate::errors::Error::Authentication`] if called without a keypair or validation fails.
    /// - [`crate::errors::Error::Pkarr`] if PKARR/DHT resolution or publish fails.
    pub async fn publish_homeserver_if_stale(
        &self,
        host_override: Option<&PublicKey>,
    ) -> Result<()> {
        self.publish_homeserver(host_override, PublishMode::IfStale)
            .await
    }

    // ---- internals ----

    async fn publish_homeserver(
        &self,
        host_override: Option<&PublicKey>,
        mode: PublishMode,
    ) -> Result<()> {
        let kp = self.keypair_ref()?;
        let pubky = kp.public_key();

        // 1) Resolve the most recent record once.
        cross_log!(
            info,
            "Preparing to publish homeserver record for {} with mode {:?}",
            pubky,
            mode
        );
        let existing = self.client.pkarr().resolve_most_recent(&pubky).await;

        // 2) Decide host string to publish.
        let Some(host_str) = Self::select_host(&pubky, host_override, existing.as_ref()) else {
            return Ok(());
        };

        // 3) Age check (for IfStale).
        if self.should_skip_due_to_age(mode, existing.as_ref(), &pubky) {
            return Ok(());
        }

        // 4) Publish with small retry loop on retryable pkarr errors.
        self.publish_with_retries(kp, &pubky, &host_str, existing)
            .await
    }

    async fn publish_homeserver_inner(
        &self,
        keypair: &Keypair,
        host: &str,
        existing: Option<SignedPacket>,
    ) -> Result<()> {
        let signed_packet = Self::build_homeserver_packet(keypair, host, existing.as_ref())?;

        cross_log!(
            debug,
            "Publishing `_pubky` packet for {} targeting host {}",
            keypair.public_key(),
            host
        );

        self.client
            .pkarr()
            .publish(&signed_packet, existing.map(|s| s.timestamp()))
            .await
            .map_err(PkarrError::from)?;

        cross_log!(
            info,
            "Successfully published `_pubky` packet for {}",
            keypair.public_key()
        );
        Ok(())
    }

    fn keypair_ref(&self) -> Result<&Keypair> {
        self.keypair.as_ref().ok_or_else(|| {
            Error::from(AuthError::Validation(
                "publishing `_pubky` requires a keypair (use Pkdns::new_with_keypair or signer.pkdns())".into(),
            ))
        })
    }

    fn select_host(
        pubky: &PublicKey,
        host_override: Option<&PublicKey>,
        existing: Option<&SignedPacket>,
    ) -> Option<String> {
        determine_host(host_override, existing).map_or_else(
            || {
                cross_log!(
                    info,
                    "No existing host found for {}; skipping publish",
                    pubky
                );
                None
            },
            |h| {
                cross_log!(
                    info,
                    "Selected host {} for `_pubky` publish of {}",
                    h,
                    pubky
                );
                Some(h)
            },
        )
    }

    fn should_skip_due_to_age(
        &self,
        mode: PublishMode,
        existing: Option<&SignedPacket>,
        pubky: &PublicKey,
    ) -> bool {
        if !matches!(mode, PublishMode::IfStale) {
            return false;
        }
        let Some(record) = existing else {
            return false;
        };

        let elapsed = Timestamp::now() - record.timestamp();
        let age = Duration::from_micros(elapsed.as_u64());
        if age <= self.stale_after {
            cross_log!(
                info,
                "Skipping publish for {}: record age {:?} <= stale_after {:?}",
                pubky,
                age,
                self.stale_after
            );
            return true;
        }

        false
    }

    async fn publish_with_retries(
        &self,
        keypair: &Keypair,
        pubky: &PublicKey,
        host: &str,
        existing: Option<SignedPacket>,
    ) -> Result<()> {
        for attempt in 1..=3 {
            cross_log!(
                info,
                "Publishing homeserver for {} (attempt {attempt}) -> host {}",
                pubky,
                host
            );
            match self
                .publish_homeserver_inner(keypair, host, existing.clone())
                .await
            {
                Ok(()) => return Ok(()),
                Err(err) if Self::should_retry(&err, attempt) => {
                    cross_log!(
                        warn,
                        "Retryable PKARR error while publishing {}: {}; retrying",
                        pubky,
                        err
                    );
                }
                Err(err) => {
                    cross_log!(error, "Failed to publish homeserver for {}: {}", pubky, err);
                    return Err(err);
                }
            }
        }

        Ok(())
    }

    const fn should_retry(err: &Error, attempt: u32) -> bool {
        matches!(err, Error::Pkarr(pk) if pk.is_retryable() && attempt < 3)
    }

    fn build_homeserver_packet(
        keypair: &Keypair,
        host: &str,
        existing: Option<&SignedPacket>,
    ) -> Result<SignedPacket> {
        // Keep previous records that are *not* `_pubky.*`, then write `_pubky` HTTPS/SVCB.
        let mut builder = SignedPacket::builder();
        if let Some(packet) = existing {
            for record in packet.all_resource_records() {
                if !record.name.to_string().starts_with("_pubky") {
                    builder = builder.record(record.to_owned());
                }
            }
        }

        let svcb = SVCB::new(0, host.try_into().map_err(PkarrError::from)?);
        let pubky_name = "_pubky".try_into().map_err(PkarrError::from)?;

        Ok(builder
            .https(pubky_name, svcb, 60 * 60)
            .sign(keypair)
            .map_err(PkarrError::from)?)
    }
}

/// Internal publish strategy.
#[derive(Debug, Clone, Copy)]
enum PublishMode {
    Force,
    IfStale,
}

/// Pick a host to publish: explicit override or the one found in the DHT packet.
fn determine_host(
    override_host: Option<&PublicKey>,
    dht_packet: Option<&SignedPacket>,
) -> Option<String> {
    if let Some(host) = override_host {
        cross_log!(info, "Using override host {} for `_pubky` publish", host);
        return Some(host.to_string());
    }
    cross_log!(debug, "Deriving publish host from existing `_pubky` record");
    dht_packet.and_then(extract_host_from_packet)
}

/// Extract `_pubky` SVCB/HTTPS target from a signed Pkarr packet.
pub fn extract_host_from_packet(packet: &SignedPacket) -> Option<String> {
    packet
        .resource_records("_pubky")
        .find_map(|rr| match &rr.rdata {
            RData::SVCB(svcb) => Some(svcb.target.to_string()),
            RData::HTTPS(https) => Some(https.0.target.to_string()),
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkarr::dns::rdata::TXT;

    #[test]
    fn republish_preserves_non_pubky_records() {
        let keypair = Keypair::random();
        let original_host = Keypair::random().public_key().to_string();

        let mut dnslink_txt = TXT::new();
        dnslink_txt
            .add_string("dnslink=/ipfs/example")
            .expect("valid dnslink string");

        let existing_packet = SignedPacket::builder()
            .https(
                "_pubky".try_into().expect("_pubky name"),
                SVCB::new(
                    0,
                    original_host
                        .as_str()
                        .try_into()
                        .expect("host name conversion"),
                ),
                3600,
            )
            .txt(
                "_dnslink".try_into().expect("_dnslink name"),
                dnslink_txt,
                3600,
            )
            .sign(&keypair)
            .expect("signed existing packet");

        let new_host = Keypair::random().public_key().to_string();

        let republished =
            Pkdns::build_homeserver_packet(&keypair, &new_host, Some(&existing_packet))
                .expect("republished packet");

        assert_eq!(
            extract_host_from_packet(&republished),
            Some(new_host.clone())
        );

        let original_dnslink = existing_packet
            .all_resource_records()
            .find(|rr| rr.name.to_string().starts_with("_dnslink"))
            .map(|rr| rr.to_owned())
            .expect("original _dnslink record");

        let republished_dnslink = republished
            .all_resource_records()
            .find(|rr| rr.name.to_string().starts_with("_dnslink"))
            .map(|rr| rr.to_owned())
            .expect("republished _dnslink record");

        assert_eq!(republished_dnslink.ttl, original_dnslink.ttl);
        assert_eq!(republished_dnslink.rdata, original_dnslink.rdata);
    }
}
