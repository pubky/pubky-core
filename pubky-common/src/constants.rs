//! Constants used across Pubky.

/// [Reserved param keys](https://www.rfc-editor.org/rfc/rfc9460#name-initial-contents) for HTTPS Resource Records
pub mod reserved_param_keys {
    /// HTTPS (RFC 9460) record's private param key, used to inform browsers
    /// about the HTTP port to use when the domain is localhost.
    pub const HTTP_PORT: u16 = 65280;
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
