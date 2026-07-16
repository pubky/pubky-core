# Deploy a Pubky Homeserver

How to make a Pubky homeserver reachable from the internet. This guide covers DNS, HTTPS via a reverse proxy, and Pubky TLS. It assumes you have already [installed the homeserver](./INSTALL.md) and have it running.

Commands and package names assume a Debian-based system (Ubuntu, Debian, etc.), adapt as needed for other distributions.

## Contents

- [Overview](#overview)
- [Open Firewall Ports](#open-firewall-ports)
- [Configure the Homeserver](#configure-the-homeserver)
- [Set Up DNS](#set-up-dns)
- [Set Up Caddy as a Reverse Proxy](#set-up-caddy-as-a-reverse-proxy)
- [Verify the Deployment](#verify-the-deployment)
  - [Check ICANN HTTPS](#check-icann-https)
  - [Check Pubky TLS](#check-pubky-tls)
  - [Check PKARR Record](#check-pkarr-record)
  - [Test a Signup](#test-a-signup)
- [Production Notes](#production-notes)
- [Troubleshooting](#troubleshooting)

## Overview

The homeserver exposes two sockets. Both need to be reachable from the internet:

| Socket | Default Port | Protocol | What we'll do |
| --- | --- | --- | --- |
| Pubky TLS | 6287 | PKARR-based TLS | Expose directly — no certificate needed. |
| ICANN HTTP | 6286 | Plain HTTP | Place behind a reverse proxy to add HTTPS for browsers. |

In this guide we'll:

1. **Open the necessary ports** so traffic can reach the homeserver.
2. **Point a domain name** at the server's public IP.
3. **Set up a reverse proxy** (Caddy) to handle HTTPS for the ICANN endpoint - this is where the domain will point.

## Open Firewall Ports

The server needs three ports open for inbound traffic:

| Port | Purpose |
| --- | --- |
| 80 | HTTP — needed by [Caddy](#set-up-caddy-as-a-reverse-proxy) for automatic TLS certificate provisioning |
| 443 | HTTPS — serves your domain with a TLS certificate (terminated by Caddy) |
| 6287 | Pubky TLS — direct Pubky protocol connections (not proxied) |

How you open these depends on your setup: it may be a cloud provider's security group, a router's port forwarding rules, or a host firewall.

> **Note:** Do **not** expose port 6286 (ICANN HTTP), 6288 (admin API), or 6289 (metrics) to the public internet. Caddy proxies to port 6286 internally. The admin and metrics APIs should remain on localhost, or behind additional authentication if exposed.

## Configure the Homeserver

Edit `~/.pubky/config.toml` with the following settings:

```toml
[drive]
# Listen on all interfaces so Pubky TLS is reachable from the internet
pubky_listen_socket = "0.0.0.0:6287"

# Keep on localhost — Caddy will proxy to this
icann_listen_socket = "127.0.0.1:6286"

[pkdns]
# The public-facing IP of this machine. Published to the DHT so that
# Pubky TLS clients can connect directly to the homeserver.
public_ip = "your_ip"

# Your domain name for HTTPS browser access (via the reverse proxy).
icann_domain = "your_domain.com"
```

Replace `your_ip` with the public IP of the machine running the homeserver. You can find it with `curl -4 ifconfig.me`.

## Set Up DNS

Configure your domain with an A record pointing to your server's public IP.

| Record Type | Name | Value |
| --- | --- | --- |
| A | your_domain.com | your_ip |

Wait for DNS propagation, then verify:

```bash
dig +short your_domain.com
```

This should return your server's public IP.

## Set Up Caddy as a Reverse Proxy

The homeserver's ICANN HTTP endpoint speaks plain HTTP. To serve it securely over HTTPS with a valid TLS certificate, you need a reverse proxy in front of it. [Caddy](https://caddyserver.com/) is a good choice because it automatically provisions and renews TLS certificates from Let's Encrypt with zero configuration.

> **Prerequisites:** [DNS propagation](#set-up-dns) must be complete before this step, and [port 80 must be open](#open-firewall-ports). Caddy uses port 80 to complete the ACME challenge when obtaining a TLS certificate, if either DNS or port 80 are not ready then certificate provisioning will fail.

Install Caddy (Debian/Ubuntu):

```bash
sudo apt install -y debian-keyring debian-archive-keyring apt-transport-https curl
curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' | sudo gpg --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' | sudo tee /etc/apt/sources.list.d/caddy-stable.list
sudo apt update && sudo apt install -y caddy
```

For other install methods, see the [Caddy install docs](https://caddyserver.com/docs/install).

Edit the Caddyfile:

```bash
sudo nano /etc/caddy/Caddyfile
```

Replace its contents with:

```
your_domain.com {
    reverse_proxy 127.0.0.1:6286
}
```

Reload Caddy:

```bash
sudo systemctl reload caddy
```

Check the logs for a successful certificate:

```bash
journalctl -u caddy --no-pager | grep "certificate obtained"
```

If this returns nothing, the certificate has not been issued yet. Check the full Caddy logs for errors:

```bash
journalctl -u caddy --no-pager | tail -50
```

> **Tip:** The most common cause of ACME certificate errors is port 80 not being reachable from the internet. If you see an ACME error in the logs, double-check your cloud firewall rules (e.g. GCP, AWS security groups) and any host-level firewall. If `dig` already returns your IP then DNS is not the issue.

Restart the homeserver for the configuration changes to take effect. If you haven't already set it up as a background service, see [systemd Service](./INSTALL.md#systemd-service) in the install guide.

## Verify the Deployment

### Check ICANN HTTPS

From your local machine, verify that HTTP redirects to HTTPS and the homeserver responds:

```bash
# Should return 308 Permanent Redirect
curl -I http://your_domain.com

# Should return 200
curl -I https://your_domain.com
```

### Check PKARR Record

Verify that your homeserver's PKARR record is published correctly by looking up its public key on [pkdns.net](https://pkdns.net/). The record should contain your server's public IP and ICANN domain.

### Check Pubky TLS

From a separate machine with the [pkarr](https://github.com/pubky/pkarr) repository cloned:

```bash
cargo run --features=reqwest-builder --example http-get https://<homeserver-public-key>
```

Replace `<homeserver-public-key>` with your homeserver's public key. You can find it via the admin API:

```bash
curl -s "http://127.0.0.1:6288/info" -H "X-Admin-Password: admin" | grep public_key
```

A successful response prints `Pubky Homeserver`.


### Test a Signup

Generate a signup token via the admin API:

```bash
curl -X GET "http://127.0.0.1:6288/generate_signup_token" \
  -H "X-Admin-Password: admin"
```

Then test signing up a user (requires `pubky-keygen` and the `signup` example from this repository):

```bash
pubky-keygen > recovery.key
cargo run -p pubky-core-examples --bin signup <homeserver-public-key> recovery.key <TOKEN>
```

## Production Notes

- Back up the homeserver's state regularly:
  - The keypair at `~/.pubky/secret` — this is the homeserver's identity. If lost, the server cannot be recovered.
  - User data — by default stored in `~/.pubky/data/files` (depends on `storage.type`).
  - The PostgreSQL database.
- Change the default admin password in `[admin].admin_password`.
- Do not expose port 6288 (admin API) or 6289 (metrics) to the public internet.

## Troubleshooting

### Caddy fails to obtain a certificate

Ensure ports 80 and 443 are open and reachable from the internet. Caddy uses HTTP-01 challenges on port 80 by default. Check your cloud firewall and any host-level firewall (`ufw`, `iptables`).

### Pubky TLS connections time out

Verify that port 6287 is open in your firewall and that the homeserver is listening on `0.0.0.0:6287`, not `127.0.0.1:6287`. Check with:

```bash
ss -tlnp | grep 6287
```

### PKARR record shows wrong IP

Check `pkdns.public_ip` in `~/.pubky/config.toml`. It must be the server's public IP, not a private or localhost address. After updating, restart the homeserver and allow a few minutes for the DHT record to propagate.

