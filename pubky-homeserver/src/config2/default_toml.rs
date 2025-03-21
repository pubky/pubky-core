//!
//! Default TOML configuration for the homeserver.
//! 
//! This is used to create a default config file if one doesn't exist.
//! 
//! Why not use the Default trait? The `toml` crate doesn't support adding comments.
//! So we maintain this default manually.
//! 

pub const DEFAULT_CONFIG: &str = r#"
# The password for the admin endpoints
admin_password = "admin"

# The mode for the signup.
signup_mode = "token_required"

[http_api]
# The port number to run an HTTP (clear text) server on.
http_port = 6286
# The port number to run an HTTPs (Pkarr TLS) server on.
https_port = 6287

# An ICANN domain name is necessary to support legacy browsers
#
# Make sure to setup a domain name and point it the IP
# address of this machine where you are running this server.
#
# This domain should point to the `<public_ip>:<public_port>`.
# 
# ICANN TLS is not natively supported, so you should be running
# a reverse proxy and managing certificates yourself.
legacy_browser_domain = "example.com"

[pkdns]
# The public IP address of the homeserver to be advertised on the DHT.
public_ip = "127.0.0.1"

# The public port the homeserver is listening on to be advertised on the DHT.
# Defaults to the http_port but might be different if you are
# using a reverse proxy.
public_port = 6286

# List of bootstrap nodes for the DHT
dht_bootstrap_nodes = [
    "router.bittorrent.com:6881",
    "dht.transmissionbt.com:6881",
    "dht.libtorrent.org:25401",
    "relay.pkarr.org:6881"
]
"#;