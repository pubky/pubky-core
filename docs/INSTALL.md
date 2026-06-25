# Install and Run Pubky Homeserver

This guide is for running a standalone Pubky homeserver. For local app development against an ephemeral test network, see [Local Development](./LOCAL_DEVELOPMENT.md). For contributor test databases and CI setup, see [Testing](./TESTING.md).

## Contents

- [Docker Compose (coming soon)](#docker-compose-coming-soon)
- [Install the Homeserver](#install-the-homeserver)
  - [Release Binary](#release-binary) | [Build From Source](#build-from-source) | [Docker](#docker)
- [Set Up PostgreSQL](#set-up-postgresql)
  - [Docker](#docker-1) | [Native](#native) | [Existing](#existing-instance)
- [Run](#run)
- [First Run](#first-run)
- [Configuration](#configuration)
- [Production Notes](#production-notes)
- [Troubleshooting](#troubleshooting)

## Docker Compose (coming soon)

A `docker-compose.yml` that bundles the homeserver and PostgreSQL together is planned. This will be the simplest way to get started.

## Install the Homeserver

### Release Binary

Download the latest non-prerelease archive from the [Pubky Core releases page](https://github.com/pubky/pubky-core/releases). Choose the archive for your operating system and CPU architecture, extract it, and place `pubky-homeserver` somewhere on your `PATH`:

```bash
wget https://github.com/pubky/pubky-core/releases/download/v0.9.0/pubky-core-v0.9.0-linux-amd64.tar.gz
tar -xf pubky-core-v0.9.0-linux-amd64.tar.gz
cp pubky-core-v0.9.0-linux-amd64/pubky-homeserver /usr/local/bin
```

### Build From Source

Make sure you have the Rust toolchain installed and working.

- [Install Guide](https://rust-lang.org/tools/install/)
- On Ubuntu, you might also need `apt install build-essential git`

Build the homeserver from the repository root:

```bash
git clone https://github.com/pubky/pubky-core.git
cd pubky-core
git checkout vx.x.x   # Pick a version
cargo build --release -p pubky-homeserver
```

Place the built release binary somewhere on your `PATH`:

```bash
cp ./target/release/pubky-homeserver /usr/local/bin
```

### Docker

Build the homeserver image using the [Dockerfile](../Dockerfile) in the repo root:

```bash
docker build --build-arg BUILD_TARGET=homeserver -t pubky-homeserver .
```

## Set Up PostgreSQL

The homeserver requires a running PostgreSQL instance with an empty database. It runs migrations automatically on startup. The default connection string is `postgres://localhost:5432/pubky_homeserver`.

### Docker

Requires [Docker Engine](https://docs.docker.com/engine/install/ubuntu/).

Start a PostgreSQL container with the `pubky_homeserver` database:

```bash
docker run --name pubky-postgres \
  -e POSTGRES_HOST_AUTH_METHOD=trust \
  -e POSTGRES_DB=pubky_homeserver \
  -p 127.0.0.1:5432:5432 \
  -v postgres-data:/var/lib/postgresql/data \
  -d postgres:17
```

Verify it's running and the database exists:

```bash
docker exec pubky-postgres psql -U postgres -c '\l pubky_homeserver'
```

No config changes needed — the default connection string matches this setup.

### Native

Install PostgreSQL ([Ubuntu guide](https://www.digitalocean.com/community/tutorials/how-to-install-postgresql-on-ubuntu-22-04-quickstart)):

```bash
sudo apt install postgresql
```

Create the database:

```bash
sudo -u postgres createdb pubky_homeserver
```

Verify the connection:

```bash
psql "postgres://localhost:5432/pubky_homeserver" -c '\conninfo'
```

On most Ubuntu installs, peer/trust auth is the default, so no config changes are needed.

### Existing instance

Create a database on your existing PostgreSQL instance:

```bash
createdb -h <HOST> -U <USER> pubky_homeserver
```

Set the connection string in `~/.pubky/config.toml`:

```toml
[general]
database_url = "postgres://<USER>:<PASSWORD>@<HOST>:5432/pubky_homeserver"
```

Verify the connection:

```bash
psql "postgres://<USER>:<PASSWORD>@<HOST>:5432/pubky_homeserver" -c '\conninfo'
```

## Run

Start the homeserver:

```bash
pubky-homeserver
```

From source:

```bash
cargo run -p pubky-homeserver
```

With Docker (using host networking so it can reach PostgreSQL on localhost):

```bash
docker run --network host -v pubky-data:/root/.pubky pubky-homeserver
```

## First Run

On first run, the homeserver creates its data directory at `~/.pubky` unless you pass a different path:

```bash
pubky-homeserver --data-dir /path/to/pubky-data
```

The data directory contains:

| Path | Purpose |
| --- | --- |
| `config.toml` | Homeserver configuration. |
| `secret` | Homeserver key material. Keep this private and backed up. |
| `data/` | Local file storage when using the filesystem storage backend. |

The generated `config.toml` is based on [`pubky-homeserver/config.sample.toml`](../pubky-homeserver/config.sample.toml).

The default endpoints are:

| Endpoint | Default |
| --- | --- |
| Public HTTP API | `http://127.0.0.1:6286` |
| Pubky TLS API | `127.0.0.1:6287` |
| Admin API | `http://127.0.0.1:6288` |
| Metrics API | `http://127.0.0.1:6289` |

Standalone homeservers require signup tokens by default. Generate one through the admin API:

```bash
curl -X GET "http://127.0.0.1:6288/generate_signup_token" \
  -H "X-Admin-Password: admin"
```

You can also open signup entirely for a private or temporary deployment by setting `signup_mode = "open"` in `config.toml` (see [Configuration](#configuration)).

## Configuration

Important settings in `config.toml`:

| Setting | Purpose |
| --- | --- |
| `general.database_url` | PostgreSQL connection string. |
| `general.signup_mode` | Signup policy: `token_required` or `open`. |
| `drive.icann_listen_socket` | Regular HTTP API listen address. |
| `drive.pubky_listen_socket` | Pubky TLS API listen address. |
| `storage.type` | Storage backend: `file_system`, `google_bucket`, or `in_memory`. |
| `admin.enabled` | Enables the admin API. |
| `admin.listen_socket` | Admin API listen address. |
| `metrics.enabled` | Enables Prometheus metrics. |
| `metrics.listen_socket` | Metrics API listen address. |
| `pkdns.public_ip` | Public IP advertised for Pubky discovery. |
| `pkdns.icann_domain` | Domain used for regular browser HTTP access. |

Review the full documented sample at [`pubky-homeserver/config.sample.toml`](../pubky-homeserver/config.sample.toml).

## Production Notes

Before using a homeserver in production:

- Use a persistent PostgreSQL instance with password authentication and back it up. Do not use trust auth in production.
- Back up the homeserver `secret` file and any filesystem or bucket storage.
- Do not expose the admin or metrics APIs to the public internet.
- Change the default admin password in `[admin].admin_password`.
- Configure `pkdns.public_ip`, `pkdns.icann_domain`, and public ports for your deployment.
- Put the regular HTTP API behind a reverse proxy if you need browser-compatible HTTPS.
- Use persistent filesystem storage or a configured bucket backend, not in-memory storage.
- Monitor logs, PostgreSQL health, disk usage, and storage backend errors.

## Troubleshooting

### `database "pubky_homeserver" does not exist`

Create the database:

```bash
createdb pubky_homeserver
```

Or update `[general].database_url` in `~/.pubky/config.toml` to point at an existing database.

### PostgreSQL Connection Refused

Make sure PostgreSQL is running and listening on the host and port in `general.database_url`.

For the Docker examples above, check the container:

```bash
docker ps --filter name=pubky-postgres
```

### Invalid Configuration

Compare your config with [`pubky-homeserver/config.sample.toml`](../pubky-homeserver/config.sample.toml). If the config was generated on first run, the file is safe to edit in place.