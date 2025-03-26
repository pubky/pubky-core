use std::time::Duration;

use crate::{DomainPort, FullConfig};

/// Convinient wrapper to build a pkarr client from
/// the data dir config.
/// Usage: `let pkarr_client = HsPkarrBuilder::from(full_config).build()?;`.
pub(crate) struct HsPkarrBuilder {
    bootstrap_nodes: Option<Vec<DomainPort>>,
    relays: Option<Vec<url::Url>>,
    request_timeout: Option<Duration>,
}

impl HsPkarrBuilder {
    pub fn new() -> Self {
        Self {
            bootstrap_nodes: None,
            relays: None,
            request_timeout: None,
        }
    }

    pub fn bootstrap_nodes(&mut self, bootstrap_nodes: Vec<DomainPort>) -> &mut Self {
        self.bootstrap_nodes = Some(bootstrap_nodes);
        self
    }

    pub fn relays(&mut self, relays: Vec<url::Url>) -> &mut Self {
        self.relays = Some(relays);
        self
    }

    pub fn request_timeout(&mut self, request_timeout: Duration) -> &mut Self {
        self.request_timeout = Some(request_timeout);
        self
    }

    /// Get the pkarr client builder.
    pub fn build_builder(self) -> pkarr::ClientBuilder {
        let mut builder = pkarr::ClientBuilder::default();
        if let Some(bootstrap_nodes) = &self.bootstrap_nodes {
            let nodes = bootstrap_nodes.iter().map(|node| node.to_string()).collect::<Vec<String>>();
            builder.bootstrap(&nodes);
        }
        if let Some(relays) = &self.relays {
            builder.relays(relays);
        }
        if let Some(request_timeout) = self.request_timeout {
            builder.request_timeout(request_timeout);
        }
        builder
    }

    /// Build the pkarr client.
    pub fn build(self) -> Result<pkarr::Client, pkarr::errors::BuildError> {
        let builder = self.build_builder();
        Ok(builder.build()?)
    }
}

impl From<FullConfig> for HsPkarrBuilder {
    fn from(config: FullConfig) -> Self {
        let mut builder = HsPkarrBuilder::new();
        if let Some(bootstrap_nodes) = config.toml.pkdns.dht_bootstrap_nodes {
            builder.bootstrap_nodes(bootstrap_nodes);
        }
        if let Some(relays) = config.toml.pkdns.dht_relay_nodes {
            builder.relays(relays);
        }
        if let Some(request_timeout) = config.toml.pkdns.dht_request_timeout {
            builder.request_timeout(request_timeout);
        }
        builder
    }
}