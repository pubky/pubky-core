use std::sync::Arc;
use std::time::Duration;

use pkarr::{
    Keypair, PublicKey, SignedPacket, Timestamp,
    dns::rdata::{RData, SVCB},
};

use crate::{
    PubkyClient, PubkySigner,
    errors::{AuthError, Error, PkarrError, Result},
    global::global_client,
};

/// PKDNS actor: resolve & publish `_pubky` PKARR records.
///
/// Construct it **without** a keypair for read-only queries:
/// ```no_run
/// # async fn example() -> pubky::Result<()> {
/// let pkdns = pubky::Pkdns::new()?;
/// if let Some(host) = pkdns.get_homeserver(&"o4dk…uyy".try_into().unwrap()).await {
///     println!("homeserver: {host}");
/// }
/// # Ok(()) }
/// ```
///
/// Or **with** a keypair for publishing:
/// ```no_run
/// # async fn example(client: std::sync::Arc<pubky::PubkyClient>, kp: pubky::Keypair) -> pubky::Result<()> {
/// let pkdns = pubky::Pkdns::with_client_and_keypair(client, kp);
/// pkdns.publish_homeserver_if_stale(None).await?;
/// # Ok(()) }
/// ```
#[derive(Debug, Clone)]
pub struct Pkdns {
    client: Arc<PubkyClient>,
    keypair: Option<Keypair>,
}

impl PubkySigner {
    /// Get a PKDNS actor bound to this signer's client and keypair (publishing enabled).
    #[inline]
    pub fn pkdns(&self) -> crate::Pkdns {
        crate::Pkdns::with_client_and_keypair(self.client.clone(), self.keypair.clone())
    }
}

impl Pkdns {
    /// Read-only PKDNS actor using the global shared client.
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: global_client()?,
            keypair: None,
        })
    }

    /// Read-only PKDNS actor on a specific client.
    pub fn with_client(client: Arc<PubkyClient>) -> Self {
        Self {
            client,
            keypair: None,
        }
    }

    /// Publishing-capable PKDNS actor: provide a client and a keypair.
    pub fn with_client_and_keypair(client: Arc<PubkyClient>, keypair: Keypair) -> Self {
        Self {
            client,
            keypair: Some(keypair),
        }
    }

    // -------------------- Reads (no keypair needed) --------------------

    /// Resolve current homeserver host for a `pubky` via Pkarr.
    ///
    /// Returns the `_pubky` SVCB/HTTPS target (domain or pubkey-as-host),
    /// or `None` if the record is missing/unresolvable.
    pub async fn get_homeserver(&self, pubky: &PublicKey) -> Option<String> {
        let packet = self.client.pkarr().resolve_most_recent(pubky).await?;
        extract_host_from_packet(&packet)
    }

    // -------------------- Publishing (requires keypair) --------------------

    /// Publish `_pubky` **forcing** a refresh.
    ///
    /// If `host_override` is `None`, reuses the host found in the existing record (if any).
    pub async fn publish_homeserver_force(&self, host_override: Option<&PublicKey>) -> Result<()> {
        self.publish_homeserver(host_override, PublishMode::Force)
            .await
    }

    /// Publish `_pubky` **only if stale/missing**.
    ///
    /// If `host_override` is `None`, reuses the host found in the existing record (if any).
    pub async fn publish_homeserver_if_stale(
        &self,
        host_override: Option<&PublicKey>,
    ) -> Result<()> {
        self.publish_homeserver(host_override, PublishMode::IfOlderThan)
            .await
    }

    // ---- internals ----

    async fn publish_homeserver(
        &self,
        host_override: Option<&PublicKey>,
        mode: PublishMode,
    ) -> Result<()> {
        let kp = self.keypair.as_ref().ok_or_else(|| {
            Error::from(AuthError::Validation(
                "publishing `_pubky` requires a keypair (use Pkdns::with_client_and_keypair or signer.pkdns())".into(),
            ))
        })?;
        let pubky = kp.public_key();

        // 1) Resolve the most recent record once.
        let existing = self.client.pkarr().resolve_most_recent(&pubky).await;

        // 2) Decide host string to publish.
        let host_str = match determine_host(host_override, existing.as_ref()) {
            Some(h) => h,
            None => return Ok(()), // nothing to do
        };

        // 3) Age check (for IfOlderThan).
        if matches!(mode, PublishMode::IfOlderThan) {
            if let Some(ref record) = existing {
                let elapsed = Timestamp::now() - record.timestamp();
                let age = Duration::from_micros(elapsed.as_u64());
                if age <= self.client.max_record_age() {
                    return Ok(());
                }
            }
        }

        // 4) Publish with small retry loop on retryable pkarr errors.
        for attempt in 1..=3 {
            match self
                .publish_homeserver_inner(kp, &host_str, existing.clone())
                .await
            {
                Ok(()) => return Ok(()),
                Err(e) => {
                    if let Error::Pkarr(pk) = &e {
                        if pk.is_retryable() && attempt < 3 {
                            continue;
                        }
                    }
                    return Err(e);
                }
            }
        }

        Ok(())
    }

    async fn publish_homeserver_inner(
        &self,
        keypair: &Keypair,
        host: &str,
        existing: Option<SignedPacket>,
    ) -> Result<()> {
        // Keep previous records that are *not* `_pubky.*`, then write `_pubky` HTTPS/SVCB.
        let mut builder = SignedPacket::builder();
        if let Some(ref packet) = existing {
            for answer in packet.resource_records("_pubky") {
                if !answer.name.to_string().starts_with("_pubky") {
                    builder = builder.record(answer.to_owned());
                }
            }
        }

        let svcb = SVCB::new(0, host.try_into().map_err(PkarrError::from)?);
        let signed_packet = builder
            .https("_pubky".try_into().unwrap(), svcb, 60 * 60)
            .sign(keypair)
            .map_err(PkarrError::from)?;

        self.client
            .pkarr()
            .publish(&signed_packet, existing.map(|s| s.timestamp()))
            .await
            .map_err(PkarrError::from)?;

        Ok(())
    }
}

/// Internal publish strategy.
#[derive(Debug, Clone, Copy)]
enum PublishMode {
    Force,
    IfOlderThan,
}

/// Pick a host to publish: explicit override or the one found in the DHT packet.
fn determine_host(
    override_host: Option<&PublicKey>,
    dht_packet: Option<&SignedPacket>,
) -> Option<String> {
    if let Some(host) = override_host {
        return Some(host.to_string());
    }
    dht_packet.and_then(extract_host_from_packet)
}

/// Extract `_pubky` SVCB/HTTPS target from a signed Pkarr packet.
pub(crate) fn extract_host_from_packet(packet: &SignedPacket) -> Option<String> {
    packet
        .resource_records("_pubky")
        .find_map(|rr| match &rr.rdata {
            RData::SVCB(svcb) => Some(svcb.target.to_string()),
            RData::HTTPS(https) => Some(https.0.target.to_string()),
            _ => None,
        })
}
