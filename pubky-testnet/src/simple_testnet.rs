use std::time::Duration;

use http_relay::HttpRelay;

use crate::FlexibleTestnet;

/// A simple testnet with
/// - A local DHT with bootstrapping nodes: `&["localhost:6881"]`.
/// - pkarr relay on port 15411.
/// - http relay on port 15412.
/// - A homeserver with address is hardcoded to `8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo`.
/// - An admin server for the homeserver.
pub struct SimpleTestnet {
    /// Inner flexible testnet.
    pub flexible_testnet: FlexibleTestnet,
    #[allow(dead_code)]
    temp_dirs: Vec<tempfile::TempDir>, // Keep temp dirs alive for the pkarr relay
}

impl SimpleTestnet {
    /// Run a new simple testnet.
    pub async fn run() -> anyhow::Result<Self> {
        let mut me = Self { 
            flexible_testnet: FlexibleTestnet::new().await?,
            temp_dirs: vec![],
        };

        me.run_fixed_pkarr_relays().await?;
        me.run_fixed_http_relay().await?;
        me.flexible_testnet.run_homeserver_suite().await?;

        Ok(me)
    }

    /// Create a new pubky client builder.
    pub fn pubky_client_builder(&self) -> pubky::ClientBuilder {
        self.flexible_testnet.pubky_client_builder()
    }

    /// Get the homeserver in the testnet.
    pub fn homeserver_suite(&self) -> &pubky_homeserver::HomeserverSuite {
        self.flexible_testnet.homeservers.first().expect("homeservers should be non-empty")
    }

    /// Get the http relay in the testnet.
    pub fn http_relay(&self) -> &HttpRelay {
        self.flexible_testnet.http_relays.first().expect("http relays should be non-empty")
    }

    /// Get the pkarr relay in the testnet.
    pub fn pkarr_relay(&self) -> &pkarr_relay::Relay {
        self.flexible_testnet.pkarr_relays.first().expect("pkarr relays should be non-empty")
    }

    /// Creates a fixed pkarr relay on port 15411 with a temporary storage directory.
    async fn run_fixed_pkarr_relays(&mut self) -> anyhow::Result<()> {
        let temp_dir = tempfile::tempdir()?; // Gets cleaned up automatically when it drops
        let mut builder = pkarr_relay::Relay::builder();
        builder
            .http_port(15411)
            .storage(temp_dir.path().to_path_buf())
            .disable_rate_limiter()
            .pkarr(|pkarr| {
                pkarr
                    .request_timeout(Duration::from_millis(100))
                    .bootstrap(&self.flexible_testnet.dht.bootstrap)
                    .dht(|builder| {
                        if !self.flexible_testnet.dht.bootstrap.first().unwrap().contains("6881") {
                            builder.server_mode().port(6881);
                        }

                        builder
                            .bootstrap(&self.flexible_testnet.dht.bootstrap)
                            .request_timeout(Duration::from_millis(200))
                    })
            });
        let relay = unsafe { builder.run() }.await?;
        self.flexible_testnet.pkarr_relays.push(relay);
        Ok(())
    }

    /// Creates a fixed http relay on port 15412.
    async fn run_fixed_http_relay(&mut self) -> anyhow::Result<()> {
        let relay = HttpRelay::builder()
        .http_port(15412) // Random available port
        .run().await?;
        self.flexible_testnet.http_relays.push(relay);
        Ok(())
    }
}