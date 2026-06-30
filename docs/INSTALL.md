# Install and Run Pubky Homeserver

This guide is for running a standalone Pubky homeserver. For local app development against a local testnet, see [Local Development](./LOCAL_DEVELOPMENT.md). For contributor test databases and CI setup, see [Testing](./TESTING.md).

## Contents

- [Quick Start with Docker Compose](#quick-start-with-docker-compose)
- [Manual Setup](#manual-setup)
  - [Install the Homeserver](#install-the-homeserver)
    - [Release Binary](#release-binary) | [Build From Source](#build-from-source) ([Cargo](#with-cargo) | [Docker](#with-docker))
  - [Initialise the Data Directory](#initialise-the-data-directory)
  - [Set Up PostgreSQL](#set-up-postgresql)
    - [Docker](#docker-1) | [Native](#native) | [Existing](#existing-instance)
  - [Configure the Homeserver with PostgreSQL](#configure-the-homeserver-with-postgresql)
  - [Run](#run)
- [Configuration](#configuration)
- [Production Notes](#production-notes)
- [Troubleshooting](#troubleshooting)


## Quick Start with Docker Compose

The [pubky-docker](https://github.com/pubky/pubky-docker) repository provides a Docker Compose setup that runs the homeserver and PostgreSQL together using pre-built images. This is the fastest way to get a homeserver running.

Requires [Docker Engine](https://docs.docker.com/engine/install/).

```bash
git clone https://github.com/pubky/pubky-docker.git
cd pubky-docker
cp .env-sample .env
docker compose --profile backend up -d postgres homeserver
```

This starts PostgreSQL and the homeserver with persistent storage, networking, and healthchecks pre-configured. The homeserver is available at `http://localhost:6286` and the admin API at `http://localhost:6288`.

To customise the homeserver, edit `homeserver.config.toml` in the pubky-docker directory. See [Configuration](#configuration) for available settings and [Production Notes](#production-notes) before exposing the server publicly.

## Manual Setup

The steps below walk through installing the homeserver binary, setting up PostgreSQL, and configuring everything individually. Use this path if you need full control over each component.

## Install the Homeserver

### Release Binary

Download the latest non-prerelease archive from the [Pubky Core releases page](https://github.com/pubky/pubky-core/releases). Choose the archive for your operating system and CPU architecture, extract it, and place `pubky-homeserver` somewhere on your `PATH`.

Requires `curl`. On Ubuntu/Debian: `apt install curl`.

```bash
curl -LO https://github.com/pubky/pubky-core/releases/download/vx.x.x/pubky-core-vx.x.x-linux-amd64.tar.gz
tar -xf pubky-core-vx.x.x-linux-amd64.tar.gz
cp pubky-core-vx.x.x-linux-amd64/pubky-homeserver /usr/local/bin
```

### Build From Source

On Ubuntu you might need: `apt install build-essential git curl`.

Clone the repository:

```bash
git clone https://github.com/pubky/pubky-core.git
cd pubky-core
git checkout vx.x.x   # Pick a version
```

#### With Cargo

Make sure you have the Rust toolchain installed and working.

<details>
<summary>Install Rust</summary>

Quick setup using [rustup](https://rustup.rs/) (recommended) on macOS or Linux:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
```

For other platforms or methods, see the [Rust Install Guide](https://rust-lang.org/tools/install/).

</details>

Build and place the binary on your `PATH`:

```bash
cargo build --release -p pubky-homeserver
cp ./target/release/pubky-homeserver /usr/local/bin
```

#### With Docker

Build the homeserver image using the [Dockerfile](../Dockerfile):

```bash
docker build --build-arg BUILD_TARGET=homeserver -t pubky-homeserver .
```

You can run the homeserver directly in Docker:

```bash
docker run -it --network=host pubky-homeserver
```

Use `--network=host` so the container can reach PostgreSQL on the host and expose its endpoints.

> **macOS note:** Docker Desktop for macOS does not support `--network=host`. Use `-p 6286:6286 -p 6287:6287 -p 6288:6288 -p 6289:6289` instead to map the ports manually.

See the Docker documentation for volume mounts and other options.

Or copy the binary out of the image and place it on your `PATH`:

```bash
docker create --name tmp-hs pubky-homeserver
docker cp tmp-hs:/usr/local/bin/homeserver /usr/local/bin/pubky-homeserver
docker rm tmp-hs
```

## Initialise the Data Directory

Create the data directory, default `config.toml`, and server keypair without starting the server or connecting to PostgreSQL:

```bash
pubky-homeserver init
```

This creates `~/.pubky/` with a sample config and a fresh server keypair. To use a different path:

```bash
pubky-homeserver --data-dir /path/to/pubky-data init
```

## Set Up PostgreSQL

The homeserver requires a running PostgreSQL instance with an empty database. It runs migrations automatically on startup. The default connection string is `postgres://localhost:5432/pubky_homeserver`.

### Docker

Requires [Docker Engine](https://docs.docker.com/engine/install/ubuntu/).

Start a PostgreSQL container with the `pubky_homeserver` database:

```bash
docker run --name pubky-postgres \
  -e POSTGRES_USER=postgres \
  -e POSTGRES_PASSWORD=postgres \
  -e POSTGRES_DB=pubky_homeserver \
  -p 127.0.0.1:5432:5432 \
  -v postgres-data:/var/lib/postgresql \
  -d postgres:18
```

Verify it's running and the database exists:

```bash
docker exec pubky-postgres psql -U postgres -c '\l pubky_homeserver'
```

### Native

Install PostgreSQL:

```bash
apt update && apt install -y postgresql
```

Start the server (not needed on systems with `systemd`, where PostgreSQL starts automatically):

```bash
pg_ctlcluster $(pg_lsclusters -h | awk '{print $1, $2}') start
```

Set a password and create the database:

```bash
su - postgres -c "psql -c \"ALTER USER postgres PASSWORD 'postgres';\" && createdb pubky_homeserver"
```

Verify the connection:

```bash
psql "postgres://postgres:postgres@localhost:5432/pubky_homeserver" -c '\conninfo'
```

### Existing instance

Create a database on your existing PostgreSQL instance:

```bash
createdb -h <HOST> -U <USER> pubky_homeserver
```

Verify the connection:

```bash
psql "postgres://<USER>:<PASSWORD>@<HOST>:5432/pubky_homeserver" -c '\conninfo'
```

## Configure the Homeserver with PostgreSQL

Update `database_url` in `~/.pubky/config.toml` to match your PostgreSQL connection string. For the Docker and Native examples above:

```toml
[general]
database_url = "postgres://postgres:postgres@localhost:5432/pubky_homeserver"
```

Replace the credentials and host if using an existing instance.

## Run

Start the homeserver:

```bash
pubky-homeserver
```

From source:

```bash
cargo run -p pubky-homeserver
```

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

If you would like to test your homeserver with example clients, see [Run Examples](./LOCAL_DEVELOPMENT.md#run-examples) in the Local Development guide.

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
createdb -h <HOST> -U <USER> pubky_homeserver
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
