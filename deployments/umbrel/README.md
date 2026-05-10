# Umbrel Deployment

This directory contains the Umbrel-specific Docker configuration for the Pubky Homeserver.

## Overview

This deployment configuration is designed specifically for running the Pubky Homeserver on Umbrel. It includes:

- Homeserver-specific Dockerfile with all required dependencies
- Entrypoint script for automatic configuration generation
- User management (non-root user with UID 1000:1000)
- Automatic config.toml generation with sensible defaults

## Files

- `Dockerfile` - Umbrel-specific Dockerfile that builds the homeserver binary
- `entrypoint.sh` - Entrypoint script that handles config generation and user switching

## Building

To build the Umbrel-specific image:

```bash
docker build -f deployments/umbrel/Dockerfile -t pubky-homeserver:umbrel .
```

## Required Environment Variables

The following environment variables are **required** for the homeserver to start:

- `POSTGRES_PASSWORD` - Password for the PostgreSQL database connection
- `ADMIN_PASSWORD` - Password for the admin API

### Optional Environment Variables

- `PUBLIC_IP` - Public IP address of the homeserver (auto-detected if not set)
- `ICANN_DOMAIN` - ICANN domain name for the homeserver (uses `DEVICE_DOMAIN_NAME` if available, otherwise defaults to `localhost`)
- `DEVICE_DOMAIN_NAME` - Umbrel's device domain name (e.g., `umbrel.local`) - used as fallback for `ICANN_DOMAIN`

## Configuration

The entrypoint script automatically generates `/data/config.toml` on first run if it doesn't exist. The configuration includes:

- Database connection using `POSTGRES_PASSWORD`
- Admin API with `ADMIN_PASSWORD`
- Automatic IP/domain detection
- File system storage
- All required services (HTTP, Pubky TLS, Admin, Metrics)

### IP and Domain Detection

The entrypoint script intelligently detects configuration values:

1. **PUBLIC_IP**: 
   - Uses `PUBLIC_IP` environment variable if set
   - Otherwise attempts to auto-detect via `hostname -i`
   - Falls back to `127.0.0.1` with a warning

2. **ICANN_DOMAIN**:
   - Uses `ICANN_DOMAIN` environment variable if set
   - Otherwise uses Umbrel's `DEVICE_DOMAIN_NAME` if available
   - Falls back to `localhost` with a warning

**Note:** If defaults (`127.0.0.1`/`localhost`) are used, the homeserver will not be accessible externally. Set `PUBLIC_IP` and `ICANN_DOMAIN` for production deployments.

### External Access and PKARR Configuration

The auto-detection feature attempts to determine the local IP, but this often resolves to the Docker internal IP rather than the public IP needed for PKARR and external services. For production deployments requiring external access, explicitly setting `PUBLIC_IP` and `ICANN_DOMAIN` is recommended.

1. **Set environment variables** in `docker-compose.yml`:
   ```yaml
   homeserver:
     environment:
       PUBLIC_IP: "your.public.ip.address"
       ICANN_DOMAIN: "your-domain.com"
   ```

2. **Infrastructure setup**:
   - Configure NAT traversal (router port forwarding for ports 6286-6289)
   - Set up firewall rules to allow incoming connections
   - Configure DNS to point your domain to your public IP
   - Ensure your network supports the required ports

## Ports

The following ports are exposed:

- `6286` - HTTP (ICANN) endpoint
- `6287` - Pubky TLS endpoint
- `6288` - Admin API
- `6289` - Metrics endpoint

## Security

- Runs as non-root user (UID 1000:1000, matching Umbrel's default)
- `POSTGRES_PASSWORD` should be sourced from Umbrel's secure `APP_PASSWORD` variable
- Each installation gets a unique, securely generated password via Umbrel's framework

## Differences from Root Dockerfile

The root `Dockerfile` is generic and defaults to `testnet`. This Umbrel-specific Dockerfile:

- Always builds the `homeserver` binary
- Includes entrypoint script for config generation
- Creates homeserver user (UID 1000:1000)
- Exposes all required ports
- Includes all runtime dependencies (su-exec, netcat, etc.)
