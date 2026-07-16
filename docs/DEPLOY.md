# Deploy a Pubky Homeserver

How to make a Pubky homeserver reachable from the internet. This guide assumes you have already [installed the homeserver](./INSTALL.md) and have it running.

Commands and package names assume a Debian-based system (Ubuntu, Debian, etc.), adapt as needed for other distributions.

> **Note:** This guide assumes your homeserver is running on a machine with a static public IP and that you have a domain name you control. It covers homeserver-specific setup only, not general server hardening. If you're new to running public-facing servers, look into basic Linux server security before proceeding.

## Contents

- [Open Ports](#open-ports)
- [Configure the Homeserver](#configure-the-homeserver)
- [Set Up DNS](#set-up-dns)
- [Set Up Caddy as a Reverse Proxy](#set-up-caddy-as-a-reverse-proxy)
- [Verify the Deployment](#verify-the-deployment)
  - [Check ICANN HTTPS](#check-icann-https)
  - [Find Your Public Key](#find-your-public-key)
  - [Check PKARR Record](#check-pkarr-record)
  - [Check Pubky TLS](#check-pubky-tls)
  - [Test a Signup](#test-a-signup)
- [Production Notes](#production-notes)
- [Troubleshooting](#troubleshooting)

## Open Ports

The homeserver exposes two endpoints: Pubky TLS on port 6287 and a plain HTTP endpoint on port 6286. We'll place [Caddy](https://caddyserver.com/) in front of the HTTP endpoint to serve it over HTTPS on port 443, it will manage the TLS certificates for us.

Open these three ports for inbound traffic:

| Port | Purpose |
| --- | --- |
| 80 | HTTP — [Caddy](#set-up-caddy-as-a-reverse-proxy) needs this for automatic TLS certificate provisioning. |
| 443 | HTTPS — serves your domain via [Caddy](#set-up-caddy-as-a-reverse-proxy) with a TLS certificate. |
| 6287 | Pubky TLS — direct Pubky protocol connections (no certificate needed). |

How you open these depends on your setup: a cloud provider's security group, a router's port-forwarding rules, or a host firewall.

Do **not** expose these ports:

| Port | Purpose |
| --- | --- |
| 6286 | ICANN HTTP — Caddy proxies to this internally. |
| 6288 | Admin API — sensitive operations, keep behind authentication if exposed. |
| 6289 | Metrics — internal monitoring only. |

## Configure the Homeserver

Edit `~/.pubky/config.toml` with the following settings:

```toml
[drive]
# Listen on all interfaces so Pubky TLS is reachable from the internet
pubky_listen_socket = "0.0.0.0:6287"

# icann_listen_socket defaults to 127.0.0.1:6286 — no change needed.

[pkdns]
# The public-facing IP of this machine. Published to the DHT so that
# Pubky TLS clients can connect directly to the homeserver.
public_ip = "your_ip"

# Your domain name for HTTPS browser access (via the reverse proxy).
icann_domain = "your_domain.com"
```

Replace `your_ip` with the public IP of the machine running the homeserver. You can find it with `curl -4 ifconfig.me`.

> **Important:** Ensure your server has a **static (reserved) public IP**. On most cloud providers the default external IP is ephemeral. If your ip changes then both your DNS `A` record and the PKARR record (which embeds `public_ip`) will silently point at a dead address.

Restart the homeserver after editing `config.toml` for the changes to take effect. If you have set it up as a background service, see [systemd Service](./INSTALL.md#systemd-service) in the install guide.

## Set Up DNS

Configure your domain with an A record pointing to your server's public IP. If your server also has a public IPv6 address, add an AAAA record too.

| Record Type | Name | Value |
| --- | --- | --- |
| A | your_domain.com | your IPv4 address |
| AAAA | your_domain.com | your IPv6 address (optional) |

Wait for DNS propagation, then verify:

```bash
dig +short your_domain.com        # A record
dig +short AAAA your_domain.com   # AAAA record (if configured)
```

These should return your server's public IP(s).

## Set Up Caddy as a Reverse Proxy

The homeserver's ICANN HTTP endpoint speaks plain HTTP. To serve it securely over HTTPS with a valid TLS certificate, you need a reverse proxy in front of it. We'll use [Caddy](https://caddyserver.com/) as its quick to setup and automatically provisions and renews TLS certificates from Let's Encrypt with zero configuration.

> **Prerequisites:** [DNS propagation](#set-up-dns) must be complete before this step, and [port 80 must be open](#open-ports). Caddy uses port 80 to complete the ACME challenge when obtaining a TLS certificate, if either DNS or port 80 are not ready then certificate provisioning will fail.

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

Check the Caddy logs — look for "certificate obtained" to confirm success, or any ACME errors:

```bash
journalctl -u caddy --no-pager | tail -50
```

> **Tip:** The most common cause of ACME certificate errors is port 80 not being reachable from the internet. If you see an ACME error in the logs, double-check your cloud firewall rules (e.g. GCP, AWS security groups) and any host-level firewall. If `dig` already returns your IP then DNS is not the issue.

> **Important:** The domain in the Caddyfile must exactly match both your DNS A-record name and the `icann_domain` value in `config.toml`. A mismatch between any of these is a common source of silent failures.

## Verify the Deployment

### Check ICANN HTTPS

From your local machine, verify that HTTP redirects to HTTPS and the homeserver responds:

```bash
# Should return 308 Permanent Redirect
curl -I http://your_domain.com

# Should return 200
curl -I https://your_domain.com
```

### Find Your Public Key

The remaining checks need your homeserver's public key. Retrieve it via the admin API:

```bash
curl -s "http://127.0.0.1:6288/info" -H "X-Admin-Password: admin" | grep public_key  # use your configured password. Default is "admin".
```

### Check PKARR Record

Look up your public key on [pkdns.net](https://pkdns.net/). The record should contain your server's public IP and ICANN domain.

For a more thorough check, resolve the record from the DHT directly using the `resolve` example from the [pkarr](https://github.com/pubky/pkarr) repository:

```bash
cargo run --example resolve <homeserver-public-key>
```

This performs a cold lookup, a cached lookup, and a network-only lookup, printing the resolved DNS records and timings for each. Verify that the output contains an `A` record with your server's public IP and `HTTPS` (SVCB) records — one for the Pubky TLS port and one pointing to your ICANN domain.

### Check Pubky TLS

From a separate machine with the [pkarr](https://github.com/pubky/pkarr) repository cloned:

```bash
cargo run --features=reqwest-builder --example http-get https://<homeserver-public-key>
```

A successful response prints `Pubky Homeserver`.


## Production Notes

- Back up the homeserver's state regularly:
  - The keypair at `~/.pubky/secret` — this is the homeserver's identity. If lost, the server cannot be recovered.
  - User data — by default stored in `~/.pubky/data/files` (depends on `storage.type`).
  - The PostgreSQL database.
- Change the default admin password in `[admin].admin_password`.

## Troubleshooting

### Caddy fails to obtain a certificate

Ensure ports 80 and 443 are open and reachable from the internet. Caddy attempts ACME challenges on both port 80 (HTTP-01) and port 443 (TLS-ALPN-01) — both should be open. Check your cloud firewall and any host-level firewall (`ufw`, `iptables`).

### Pubky TLS connections time out

Verify that port 6287 is open in your firewall and that the homeserver is listening on `0.0.0.0:6287`, not `127.0.0.1:6287`. Check with:

```bash
sudo ss -tlnp | grep 6287
```

### PKARR record shows wrong IP

Check `pkdns.public_ip` in `~/.pubky/config.toml`. It must be the server's public IP, not a private or localhost address. After updating, restart the homeserver and allow a few minutes for the DHT record to propagate.

