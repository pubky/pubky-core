#!/bin/sh
set -e

# Ensure /data directory exists
mkdir -p /data

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
  
  # Determine ICANN_DOMAIN: use env var, or use DEVICE_DOMAIN_NAME, or use default
  if [ -n "$ICANN_DOMAIN" ]; then
    DETECTED_ICANN_DOMAIN="$ICANN_DOMAIN"
  elif [ -n "$DEVICE_DOMAIN_NAME" ]; then
    # Use Umbrel's device domain name as a better default
    DETECTED_ICANN_DOMAIN="$DEVICE_DOMAIN_NAME"
  else
    DETECTED_ICANN_DOMAIN="localhost"
  fi
  
  # Warn if using defaults that won't work for external access
  if [ "$DETECTED_PUBLIC_IP" = "127.0.0.1" ] || [ "$DETECTED_ICANN_DOMAIN" = "localhost" ]; then
    echo "WARNING: Using default values for public_ip ($DETECTED_PUBLIC_IP) and icann_domain ($DETECTED_ICANN_DOMAIN)." >&2
    echo "WARNING: These values will not work for a real homeserver. Please set PUBLIC_IP and ICANN_DOMAIN environment variables." >&2
    echo "WARNING: For Umbrel users, ICANN_DOMAIN can be set to your device domain or a custom domain." >&2
  fi
  
  # Use envsubst to safely substitute environment variables in the config template
  # This prevents TOML syntax errors from special characters in passwords
  export POSTGRES_PASSWORD ADMIN_PASSWORD DETECTED_PUBLIC_IP DETECTED_ICANN_DOMAIN
  envsubst < /usr/local/share/config.toml.template > /data/config.toml
  # Ensure the config file is readable by homeserver user
  chmod 644 /data/config.toml || true
  chown homeserver:homeserver /data/config.toml || true
fi

# Optimize chown: only run if ownership change is needed
# Check if /data is owned by homeserver user
if [ "$(stat -c '%U:%G' /data 2>/dev/null || stat -f '%Su:%Sg' /data 2>/dev/null)" != "homeserver:homeserver" ]; then
  # Only chown if ownership is different
  chown -R homeserver:homeserver /data || true
fi

# Switch to homeserver user and run the homeserver with --data-dir /data
exec su-exec homeserver:homeserver homeserver --data-dir /data
