//! Pkarr related task

use std::net::IpAddr;
use std::sync::Arc;

use anyhow::Result;
use pkarr::dns::Name;
use pkarr::errors::PublishError;
use pkarr::{dns::rdata::SVCB, Keypair, SignedPacket};

use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time::{interval, Duration};

use crate::Domain;


pub (crate) struct HomeserverKeyRepublisherConfig {
    pub(crate) keypair: Keypair,
    pub(crate) client: pkarr::Client,
    pub(crate) public_ip: IpAddr,
    pub(crate) pubky_tls_port: u16,
    pub(crate) icann_http_port: u16,
    pub(crate) domain: Option<Domain>,
}

impl HomeserverKeyRepublisherConfig {
    pub fn new(
        keypair: Keypair,
        public_ip: IpAddr,
        pubky_tls_port: u16,
        icann_http_port: u16,
        client: pkarr::Client,
    ) -> Self {
        Self {
            keypair,
            public_ip,
            pubky_tls_port,
            icann_http_port,
            domain: None,
            client,
        }
    }

    pub fn domain(&mut self, domain: Domain) -> &mut Self {
        self.domain = Some(domain);
        self
    }

}

/// Republishes the homeserver's pkarr packet to the DHT every hour.
#[derive(Debug, Clone)]
pub struct HomeserverKeyRepublisher {
    client: pkarr::Client,
    signed_packet: SignedPacket,
    republish_task: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl HomeserverKeyRepublisher {
    pub fn new(
        config: HomeserverKeyRepublisherConfig,
    ) -> Result<Self> {

        let signed_packet = create_signed_packet(&config)?;

        Ok(Self {
            client: config.client,
            signed_packet,
            republish_task: Arc::new(Mutex::new(None)),
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
    config: &HomeserverKeyRepublisherConfig
) -> Result<SignedPacket> {
    let root_name: Name = ".".try_into().expect(". is the root domain and always valid");

    let mut signed_packet_builder = SignedPacket::builder();

    // `SVCB(HTTPS)` record pointing to the pubky tls port and the public ip address
    let mut svcb = SVCB::new(0, root_name.clone());
    svcb.set_port(
        config.pubky_tls_port,
    );
    match &config.public_ip {
        IpAddr::V4(ip) => {
            svcb.set_ipv4hint([ip.to_bits()]);
        },
        IpAddr::V6(_) => {
            // TODO: Implement ipv6 support
            tracing::warn!("IPv6 is not supported yet. Ignoring ipv6 hint in homeserver's pkarr packet.");
        },
    };
    signed_packet_builder = signed_packet_builder.https(
        root_name.clone(),
        svcb,
        60 * 60,
    );

    // `SVCB` record pointing to the icann http port and the ICANN domain for legacy browsers support.
    // Low priority to not override the `SVCB(HTTPS)` record.
    if let Some(ref domain) = config.domain {
        let mut svcb = SVCB::new(10, root_name.clone());

        let http_port_be_bytes = config.icann_http_port.to_be_bytes();
        if domain.0 == "localhost" {
            svcb.set_param(
                pubky_common::constants::reserved_param_keys::HTTP_PORT,
                &http_port_be_bytes,
            )?;
        }
        svcb.target = domain.0.as_str().try_into()?;
        signed_packet_builder = signed_packet_builder.https(
            root_name.clone(),
            svcb,
            60 * 60,
        );
    }

    // `A` record to the public IP. This is used for regular browser connections.
    signed_packet_builder = signed_packet_builder.address(
        root_name.clone(),
        config.public_ip.clone(),
        60 * 60,
    );

    Ok(signed_packet_builder.build(&config.keypair)?)
}


#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use pkarr::PublicKey;

    use super::*;

    #[tokio::test]
    async fn pull_homeserver_packet_from_dht() {
        let client = pkarr::ClientBuilder::default().build().unwrap();
        let public_key_str = "8um71us3fyw6h8wbcxb5ar3rwusy1a6u49956ikzojg3gcwd1dty";
        let public_key = PublicKey::from_str(public_key_str).unwrap();
        let packet = client.resolve(&public_key).await.unwrap();
        println!("{:?}", packet);
    }
    
}
