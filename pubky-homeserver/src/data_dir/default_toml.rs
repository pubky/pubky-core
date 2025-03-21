//!
//! Default TOML configuration for the homeserver.
//! 
//! This is used to create a default config file if one doesn't exist.
//! 
//! Why not use the Default trait? The `toml` crate doesn't support adding comments.
//! So we maintain this default manually.
//! 

pub const DEFAULT_CONFIG: &str = r#"
# The mode for the signup. Options:
# "open" - anyone can signup.
# "token_required" - a signup token is required to signup.
signup_mode = "token_required"

[icann_drive_api]
# The port number to run an HTTP (clear text) server on.
# Used for http requests from regular browsers.
# May be put behind a reverse proxy with TLS enabled.
listen_socket = "127.0.0.1:6286"

# An ICANN domain name is necessary to support legacy browsers
#
# Make sure to setup a domain name and point it the IP
# address of this machine where you are running this server.
#
# This domain should point to the `<public_ip>:<public_port>`.
# 
# ICANN TLS is not natively supported, so you should be running
# a reverse proxy and managing certificates yourself.
domain = "example.com"

[pubky_drive_api]
# The port number to run an HTTPS (Pkarr TLS) server on.
# Pkarr TLS is a TLS implementation that is compatible with the Pkarr protocol.
# No need to provide a ICANN TLS certificate.
listen_socket = "127.0.0.1:6287"

[admin_api]
# The port number to run the admin HTTP (clear text) server on.
# Used for admin requests from the admin UI.
listen_socket = "127.0.0.1:6288"

# The password for the admin user to access the admin UI.
admin_password = "admin"

[pkdns]
# The public IP address and port of the homeserver pubky_drive_api to be advertised on the DHT.
# Must be set to be reachable from the outside.
public_socket = "127.0.0.1:6286"

# The interval at which user keys are republished to the DHT.
user_keys_republisher_interval = 14400  # 4 hours in seconds

# List of bootstrap nodes for the DHT.
# domain:port format.
dht_bootstrap_nodes = [
    "router.bittorrent.com:6881",
    "dht.transmissionbt.com:6881",
    "dht.libtorrent.org:25401",
    "relay.pkarr.org:6881"
]

# Relay node urls for the DHT.
# Improves the availability of pkarr packets.
dht_relay_nodes = [
    "https://relay.pkarr.org", 
    "https://pkarr.pubky.org"
]
"#;