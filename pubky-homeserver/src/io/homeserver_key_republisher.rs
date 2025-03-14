//! Pkarr related task

use anyhow::Result;
use pkarr::errors::PublishError;
use pkarr::{dns::rdata::SVCB, Keypair, SignedPacket};

use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time::{interval, Duration};

use super::IoConfig;

/// Republishes the homeserver's pkarr packet to the DHT every hour.
#[derive(Debug)]
pub struct HomeserverKeyRepublisher {
    client: pkarr::Client,
    signed_packet: SignedPacket,
    republish_task: Mutex<Option<JoinHandle<()>>>,
}

impl HomeserverKeyRepublisher {
    pub fn new(
        keypair: &Keypair,
        config: &IoConfig,
        https_port: u16,
        http_port: u16,
    ) -> Result<Self> {
        let mut builder = pkarr::Client::builder();

        // TODO: Relays have rate limits. pkarr currently panics if accessed too quickly.
        builder.no_relays();

        if let Some(bootstrap) = &config.bootstrap {
            builder.bootstrap(bootstrap);
        }

        if let Some(request_timeout) = config.dht_request_timeout {
            builder.request_timeout(request_timeout);
        }

        let client = builder.build()?;

        let signed_packet = create_signed_packet(keypair, config, https_port, http_port)?;

        Ok(Self {
            client,
            signed_packet,
            republish_task: Mutex::new(None),
        })
    }

    async fn publish_once(
        client: &pkarr::Client,
        signed_packet: &SignedPacket,
    ) -> Result<(), PublishError> {
        let res = client.publish(signed_packet, None).await;
        if let Err(e) = &res {
            tracing::warn!(
                "Failed to publish the homeserver's pkarr packet to the DHT: {}",
                e
            );
        } else {
            tracing::info!("Published the homeserver's pkarr packet to the DHT.");
        }
        res
    }

    /// Start the periodic republish task which will republish the server packet to the DHT every hour.
    ///
    /// # Errors
    /// - Throws an error if the initial publish fails.
    /// - Throws an error if the periodic republish task is already running.
    pub async fn start_periodic_republish(&self) -> anyhow::Result<()> {
        let mut task_guard = self.republish_task.lock().await;

        if task_guard.is_some() {
            return Err(anyhow::anyhow!(
                "Periodic republish task is already running"
            ));
        }

        // Publish once to make sure the packet is published to the DHT before this
        // function returns.
        // Throws an error if the packet is not published to the DHT.
        Self::publish_once(&self.client, &self.signed_packet).await?;

        // Start the periodic republish task.
        let client = self.client.clone();
        let signed_packet = self.signed_packet.clone();
        let handle = tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(60 * 60)); // 1 hour in seconds
            interval.tick().await; // This ticks immediatly. Wait for first interval before starting the loop.
            loop {
                interval.tick().await;
                let _ = Self::publish_once(&client, &signed_packet).await;
            }
        });

        *task_guard = Some(handle);
        Ok(())
    }

    /// Stop the periodic republish task.
    pub async fn stop_periodic_republish(&self) {
        let mut task_guard = self.republish_task.lock().await;

        if let Some(handle) = task_guard.take() {
            handle.abort();
        }
    }
}

pub fn create_signed_packet(
    keypair: &Keypair,
    config: &IoConfig,
    https_port: u16,
    http_port: u16,
) -> Result<SignedPacket> {
    // TODO: Try to resolve first before publishing.

    let mut signed_packet_builder = SignedPacket::builder();

    let mut svcb = SVCB::new(0, ".".try_into()?);

    // Set the public Ip or localhost
    signed_packet_builder = signed_packet_builder.address(
        ".".try_into()
            .expect(". is valid domain and therefore always succeeds"),
        config
            .public_addr
            .map(|addr| addr.ip())
            .unwrap_or("127.0.0.1".parse().expect("localhost is valid ip")),
        60 * 60,
    );

    // Set the public port or the local https_port
    svcb.set_port(
        config
            .public_addr
            .map(|addr| addr.port())
            .unwrap_or(https_port),
    );

    signed_packet_builder = signed_packet_builder.https(
        ".".try_into()
            .expect(". is valid domain and therefore always succeeds"),
        svcb,
        60 * 60,
    );

    // Set low priority https record for legacy browsers support
    if let Some(ref domain) = config.domain {
        let mut svcb = SVCB::new(10, ".".try_into()?);

        let http_port_be_bytes = http_port.to_be_bytes();
        if domain == "localhost" {
            svcb.set_param(
                pubky_common::constants::reserved_param_keys::HTTP_PORT,
                &http_port_be_bytes,
            )?;
        }

        svcb.target = domain.as_str().try_into()?;

        signed_packet_builder = signed_packet_builder.https(
            ".".try_into()
                .expect(". is valid domain and therefore always succeeds"),
            svcb,
            60 * 60,
        );
    }

    Ok(signed_packet_builder.build(keypair)?)
}
