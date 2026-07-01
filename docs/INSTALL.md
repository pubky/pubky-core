# Install and Run Pubky Homeserver

How to set up and operate a Pubky homeserver.

> [!WARNING]
> This project is under active development. Do not use it to run public, production, or mission-critical services without accepting that risk. Production deployments require infrastructure that is hardened, monitored, and maintained beyond what this guide covers.

> **Looking for something else?**
> See [Local Development](./LOCAL_DEVELOPMENT.md) for deploying a standalone homeserver and [Testing](./TESTING.md) for test databases and CI setup.

## Contents

- [Quick Start with Docker Compose](#quick-start-with-docker-compose)
- [Manual Setup](#manual-setup)
  - [Install the Homeserver](#install-the-homeserver)
    - [Release Binary](#release-binary) | [Build From Source](#build-from-source) ([Cargo](#build-a-binary-with-cargo) | [Docker](#build-a-docker-image))
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
docker compose --profile backend up --no-build -d postgres homeserver
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
cd pubky-core-vx.x.x-linux-amd64
cp pubky-homeserver /usr/local/bin
```

Verify the install:

```bash
pubky-homeserver --version
```

### Build From Source

On Ubuntu you might need: `apt install build-essential git curl`.

Clone the repository:

```bash
git clone https://github.com/pubky/pubky-core.git
cd pubky-core
git checkout vx.x.x   # Pick a version
```

#### Build a binary with Cargo

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

Verify the install:

```bash
pubky-homeserver --version
```

#### Build a Docker image

Requires [Docker Engine](https://docs.docker.com/engine/install/ubuntu/).

Build the homeserver image using the [Dockerfile](../Dockerfile):

```bash
docker build --build-arg BUILD_TARGET=homeserver -t pubky-homeserver .
```

Verify the image built correctly:

```bash
docker run --rm pubky-homeserver --version
```

## Initialise the Data Directory

Create the data directory, default `config.toml`, and server keypair without starting the server or connecting to PostgreSQL:

```bash
pubky-homeserver init
```

With Docker:

```bash
docker run -it -v ~/.pubky:/root/.pubky pubky-homeserver init
```

> **Note:** The `init` subcommand is available from v0.10 onwards. On v0.9 or earlier, the data directory is created automatically on first run. Start the homeserver once (it will fail if PostgreSQL is not yet configured, but the directory, sample config, and keypair will already be written to `~/.pubky/`).

This creates `~/.pubky/` with a sample config and a fresh server keypair. To use a different path:

```bash
pubky-homeserver --data-dir /path/to/pubky-data init
```

## Set Up PostgreSQL

The homeserver requires a running PostgreSQL instance with an empty database.

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

With Docker:

```bash
docker run -it --network=host -v ~/.pubky:/root/.pubky pubky-homeserver
```

Use `--network=host` so the container can reach PostgreSQL on the host and expose its endpoints. The volume mount shares the data directory (config and keypair) with the container.

> **macOS note:** Docker Desktop for macOS does not support `--network=host`. Use `-p 6286:6286 -p 6287:6287 -p 6288:6288 -p 6289:6289` instead to map the ports manually.

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

If you would like to test your homeserver with example clients, see [Run Examples](./LOCAL_DEVELOPMENT.md#run-examples) in the Local Development guide.

## Configuration

Important settings in `config.toml`:

| Setting | Purpose |
| --- | --- |
| `general.database_url` | PostgreSQL connection string. |
| `drive.icann_listen_socket` | Regular HTTP API listen address. |
| `drive.pubky_listen_socket` | Pubky TLS API listen address. |
| `storage.type` | Storage backend: `file_system`, `google_bucket`, or `in_memory`. |
| `admin.enabled` | Enables the admin API. |
| `admin.listen_socket` | Admin API listen address. |

Review the full documented sample at [`pubky-homeserver/config.sample.toml`](../pubky-homeserver/config.sample.toml).

## Production Notes

Before using a homeserver in production:

- Use a persistent PostgreSQL instance with password authentication and back it up.
- Back up the homeserver `secret` file and any filesystem or bucket storage.
- Do not expose the admin or metrics APIs to the public internet.
- Change the default admin password in `[admin].admin_password`.
- Configure `pkdns.public_ip`, `pkdns.icann_domain`, and public ports for your deployment.
- Use persistent filesystem storage or a configured bucket backend, not in-memory storage.
- The homeserver exposes two sockets: a **Pubky TLS** socket (`pubky_listen_socket`, default port 6287) and a regular **HTTP** socket (`icann_listen_socket`, default port 6286). Pubky TLS uses PKARR-based TLS and does not need a certificate so can be exposed directly. The HTTP socket serves browsers and should be put behind a reverse proxy if you need standard HTTPS with a domain certificate.

## Troubleshooting

### `database "pubky_homeserver" does not exist`

Create the database. With a native PostgreSQL install:

```bash
createdb -h <HOST> -U <USER> pubky_homeserver
```

Or if PostgreSQL is running in Docker:

```bash
docker exec pubky-postgres createdb -U postgres pubky_homeserver
```

Or update `[general].database_url` in `~/.pubky/config.toml` to point at an existing database.

### PostgreSQL Connection Refused

Check that the `host` and `port` in `general.database_url` match where PostgreSQL is actually listening.

**1. Is PostgreSQL running?**

Native install:

```bash
pg_isready
```

Docker:

```bash
docker exec pubky-postgres pg_isready
```

If it reports "no response" then start or restart PostgreSQL.

**2. Can you connect with the configured credentials?**

Test the exact connection string from your `config.toml`. With a native install:

```bash
psql "postgres://postgres:postgres@localhost:5432/pubky_homeserver" -c '\conninfo'
```

If PostgreSQL is running in Docker:

```bash
docker exec pubky-postgres psql -U postgres -d pubky_homeserver -c '\conninfo'
```

If this fails with "password authentication failed", check the username and password. If it fails with "connection refused", PostgreSQL may be listening on a different address or port - check `listen_addresses` and `port` in `postgresql.conf`.

### Invalid Configuration

Compare your config with [`pubky-homeserver/config.sample.toml`](../pubky-homeserver/config.sample.toml). If the config was generated on first run, the file is safe to edit in place.
