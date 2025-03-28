//! Background task to republish the homeserver's pkarr packet to the DHT.
//! 
//! This task is started by the [crate::HomeserverCore] and runs until the homeserver is stopped.
//! 
//! The task is responsible for:
//! - Republishing the homeserver's pkarr packet to the DHT every hour.
//! - Stopping the task when the homeserver is stopped.

use std::net::IpAddr;

use anyhow::Result;
use pkarr::dns::Name;
use pkarr::errors::PublishError;
use pkarr::{dns::rdata::SVCB, SignedPacket};

use tokio::task::JoinHandle;
use tokio::time::{interval, Duration};
use crate::app_context::AppContext;


/// Republishes the homeserver's pkarr packet to the DHT every hour.
pub struct HomeserverKeyRepublisher {
    join_handle: JoinHandle<()>,
}

impl HomeserverKeyRepublisher {
    pub async fn run(context: &AppContext) -> Result<Self> {    

        let signed_packet = create_signed_packet(context)?;
        let join_handle = Self::start_periodic_republish(context.pkarr_client.clone(), &signed_packet).await?;
        Ok(Self {
            join_handle,
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
    async fn start_periodic_republish(client: pkarr::Client, signed_packet: &SignedPacket) -> anyhow::Result<JoinHandle<()>> {
        // Publish once to make sure the packet is published to the DHT before this
        // function returns.
        // Throws an error if the packet is not published to the DHT.
        Self::publish_once(&client, signed_packet).await?;

        // Start the periodic republish task.
        let signed_packet = signed_packet.clone();
        let handle = tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(60 * 60)); // 1 hour in seconds
            interval.tick().await; // This ticks immediatly. Wait for first interval before starting the loop.
            loop {
                interval.tick().await;
                let _ = Self::publish_once(&client, &signed_packet).await;
            }
        });

        Ok(handle)
    }

    /// Stop the periodic republish task.
    pub fn stop(&self) {
        self.join_handle.abort();
    }
}

impl Drop for HomeserverKeyRepublisher {
    fn drop(&mut self) {
        self.stop();
    }
}


pub fn create_signed_packet(context: &AppContext) -> Result<SignedPacket> {
    let root_name: Name = ".".try_into().expect(". is the root domain and always valid");

    let mut signed_packet_builder = SignedPacket::builder();

    let public_ip = context.config_toml.pkdns.public_ip;
    let public_pubky_tls_port = context.config_toml.pkdns.public_pubky_tls_port.unwrap_or(context.config_toml.drive.pubky_listen_socket.port());
    let public_icann_http_port = context.config_toml.pkdns.public_icann_http_port.unwrap_or(context.config_toml.drive.icann_listen_socket.port());

    // `SVCB(HTTPS)` record pointing to the pubky tls port and the public ip address
    let mut svcb = SVCB::new(0, root_name.clone());
    svcb.set_port(
        public_pubky_tls_port,
    );
    match &public_ip {
        IpAddr::V4(ip) => {
            svcb.set_ipv4hint([ip.to_bits()])?;
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
    if let Some(domain) = &context.config_toml.drive.icann_domain {
        let mut svcb = SVCB::new(10, root_name.clone());

        let http_port_be_bytes = public_icann_http_port.to_be_bytes();
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
        public_ip.clone(),
        60 * 60,
    );

    Ok(signed_packet_builder.build(&context.keypair)?)
}
