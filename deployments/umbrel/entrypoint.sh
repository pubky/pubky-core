#!/bin/sh
set -e

# Ensure /data directory exists
mkdir -p /data

# Cloudflare Tunnel: read domain from dashboard-written file if present (overrides env)
if [ -f /etc/pubky-cloudflare/domain ] && [ -s /etc/pubky-cloudflare/domain ]; then
  CLOUDFLARE_DOMAIN=$(cat /etc/pubky-cloudflare/domain | tr -d '\n\r')
  export CLOUDFLARE_DOMAIN
fi

# Generate config.toml if it doesn't exist
if [ ! -f /data/config.toml ]; then
  # Validate required environment variables
  if [ -z "$POSTGRES_PASSWORD" ]; then
    echo "ERROR: POSTGRES_PASSWORD environment variable is required" >&2
    exit 1
  fi
  
  if [ -z "$ADMIN_PASSWORD" ]; then
    echo "ERROR: ADMIN_PASSWORD environment variable is required" >&2
    exit 1
  fi
  
  # Determine PUBLIC_IP: use env var, or try to detect, or use default
  if [ -n "$PUBLIC_IP" ]; then
    DETECTED_PUBLIC_IP="$PUBLIC_IP"
  else
    # Try to get the device's local IP (works in Docker networks)
    DETECTED_PUBLIC_IP=$(hostname -i 2>/dev/null | awk '{print $1}' | grep -E '^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$' | head -n1)
    if [ -z "$DETECTED_PUBLIC_IP" ]; then
      DETECTED_PUBLIC_IP="127.0.0.1"
    fi
  fi
  
  # Determine ICANN_DOMAIN: Cloudflare Tunnel takes precedence, then env, then Umbrel device name, then default
  DETECTED_PUBLIC_ICANN_HTTP_PORT=""
  if [ -n "$CLOUDFLARE_DOMAIN" ]; then
    DETECTED_ICANN_DOMAIN="$CLOUDFLARE_DOMAIN"
    DETECTED_PUBLIC_ICANN_HTTP_PORT="443"
  elif [ -n "$ICANN_DOMAIN" ]; then
    DETECTED_ICANN_DOMAIN="$ICANN_DOMAIN"
  elif [ -n "$DEVICE_DOMAIN_NAME" ]; then
    DETECTED_ICANN_DOMAIN="$DEVICE_DOMAIN_NAME"
  else
    DETECTED_ICANN_DOMAIN="localhost"
  fi

  # Warn if using defaults that won't work for external access (skip when using Cloudflare)
  if [ -z "$CLOUDFLARE_DOMAIN" ] && { [ "$DETECTED_PUBLIC_IP" = "127.0.0.1" ] || [ "$DETECTED_ICANN_DOMAIN" = "localhost" ]; }; then
    echo "WARNING: Using default values for public_ip ($DETECTED_PUBLIC_IP) and icann_domain ($DETECTED_ICANN_DOMAIN)." >&2
    echo "WARNING: Set PUBLIC_IP and ICANN_DOMAIN, or use Cloudflare Tunnel (CLOUDFLARE_DOMAIN + CLOUDFLARE_TUNNEL_TOKEN)." >&2
  fi

  export POSTGRES_PASSWORD ADMIN_PASSWORD DETECTED_PUBLIC_IP DETECTED_ICANN_DOMAIN
  envsubst < /usr/local/share/config.toml.template > /data/config.toml

  if [ -n "$DETECTED_PUBLIC_ICANN_HTTP_PORT" ]; then
    sed -i "/^icann_domain = /a public_icann_http_port = $DETECTED_PUBLIC_ICANN_HTTP_PORT" /data/config.toml
  fi

  chmod 644 /data/config.toml || true
  chown homeserver:homeserver /data/config.toml || true
fi

# If config already exists and CLOUDFLARE_DOMAIN is set (e.g. from dashboard), update [pkdns]
if [ -f /data/config.toml ] && [ -n "$CLOUDFLARE_DOMAIN" ]; then
  if grep -q '^icann_domain = ' /data/config.toml; then
    sed -i "s|^icann_domain = .*|icann_domain = \"$CLOUDFLARE_DOMAIN\"|" /data/config.toml
  fi
  if ! grep -q '^public_icann_http_port = ' /data/config.toml; then
    sed -i "/^icann_domain = /a public_icann_http_port = 443" /data/config.toml
  else
    sed -i 's/^public_icann_http_port = .*/public_icann_http_port = 443/' /data/config.toml
  fi
  chown homeserver:homeserver /data/config.toml 2>/dev/null || true
fi

# Optimize chown: only run if ownership change is needed
# Check if /data is owned by homeserver user
if [ "$(stat -c '%U:%G' /data 2>/dev/null || stat -f '%Su:%Sg' /data 2>/dev/null)" != "homeserver:homeserver" ]; then
  # Only chown if ownership is different
  chown -R homeserver:homeserver /data || true
fi

# Switch to homeserver user and run the homeserver with --data-dir /data
exec su-exec homeserver:homeserver homeserver --data-dir /data
