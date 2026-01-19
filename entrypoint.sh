#!/bin/sh
set -e

# Ensure /data directory exists
mkdir -p /data

# Generate config.toml if it doesn't exist
if [ ! -f /data/config.toml ]; then
  cat > /data/config.toml <<EOF
[general]
signup_mode = "token_required"
user_storage_quota_mb = 0
database_url = "postgres://pubky:${POSTGRES_PASSWORD}@postgres:5432/pubky_homeserver"

[drive]
pubky_listen_socket = "0.0.0.0:6287"
icann_listen_socket = "0.0.0.0:6286"

[storage]
type = "file_system"

[admin]
enabled = true
listen_socket = "0.0.0.0:6288"
admin_password = "${ADMIN_PASSWORD}"

[metrics]
enabled = true
listen_socket = "0.0.0.0:6289"

[pkdns]
public_ip = "127.0.0.1"
icann_domain = "localhost"
user_keys_republisher_interval = 14400
dht_relay_nodes = ["https://pkarr.pubky.app", "https://pkarr.pubky.org"]

[logging]
level = "info"
module_levels = ["pubky_homeserver=debug", "tower_http=debug"]
EOF
  # Ensure the config file is readable by homeserver user
  chmod 644 /data/config.toml || true
  chown homeserver:homeserver /data/config.toml || true
fi

# Ensure /data is owned by homeserver user (for other files it creates)
chown -R homeserver:homeserver /data || true

# Switch to homeserver user and run the homeserver with --data-dir /data
exec su-exec homeserver:homeserver homeserver --data-dir /data
