//! Constants used across Pubky.

use std::net::IpAddr;

/// [Reserved param keys](https://www.rfc-editor.org/rfc/rfc9460#name-initial-contents) for HTTPS Resource Records
pub mod reserved_param_keys {
    /// HTTPS (RFC 9460) record's private param key, used to inform browsers
    /// about the HTTP port to use on domains that cannot obtain TLS certificates
    /// (localhost, bare IP addresses).
    pub const HTTP_PORT: u16 = 65280;
}

/// Returns `true` when the domain cannot obtain a TLS certificate and must
/// use plain HTTP instead — i.e. `localhost` or a bare IP address.
///
/// These hosts need an explicit [`reserved_param_keys::HTTP_PORT`] in their
/// SVCB record so clients know which port to connect to over HTTP.
///
/// TODO: This should live in pkarr crate with all other SVCB/HTTPS record logic.
pub fn requires_http_port(domain: &str) -> bool {
    domain == "localhost" || domain.parse::<IpAddr>().is_ok()
}

/// Local test network's hardcoded port numbers for local development.
pub mod testnet_ports {
    /// The local test network's hardcorded DHT bootstrapping node's port number.
    pub const BOOTSTRAP: u16 = 6881;
    /// The local test network's hardcorded Pkarr Relay port number.
    pub const PKARR_RELAY: u16 = 15411;
    /// The local test network's hardcorded HTTP Relay port number.
    pub const HTTP_RELAY: u16 = 15412;
}
