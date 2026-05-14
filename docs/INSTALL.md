# Install and Run Pubky Homeserver

This guide is for running a standalone Pubky homeserver. For local app development against an ephemeral test network, see [Local Development](./LOCAL_DEVELOPMENT.md). For contributor test databases and CI setup, see [Testing](./TESTING.md).

## Choose an Install Method

### Install a Release Binary

Download the latest non-prerelease archive from the [Pubky Core releases page](https://github.com/pubky/pubky-core/releases).

Choose the archive for your operating system and CPU architecture, extract it, and place `pubky-homeserver` somewhere on your `PATH`.

On Windows, the binary is named `pubky-homeserver.exe`.

### Build From Source Ubuntu

<details>
<summary>Rust Toolchain required</summary>

#### Install Toolchain

Make sure you have the rust toolchain installed and working.

- [Install Guide](https://rust-lang.org/tools/install/)
- On Ubuntu, you might also need `apt install build-essential git`

</details>

Build the homeserver from the repository root:

```bash
git clone https://github.com/pubky/pubky-core.git
cd pubky-core
git checkout vx.x.x   # Pick a version
cargo build --release -p pubky-homeserver
```

Place the built release binary `pubky-homeserver` somewhere on your `PATH`. For example:

```bash
cp ./target/release/pubky-homeserver /usr/local/bin
```

## PostgreSQL

The standalone homeserver requires PostgreSQL. 

### Docker

> Make sure you have the [Docker Engine](https://docs.docker.com/engine/install/ubuntu/) installed.

For a local Docker PostgreSQL instance with password authentication:

```bash
docker run --name pubky-postgres \
  -e POSTGRES_USER=postgres \
  -e POSTGRES_PASSWORD=postgres \
  -e POSTGRES_DB=pubky_homeserver \
  -p 127.0.0.1:5432:5432 \
  -v postgres-data:/var/lib/postgresql/data \
  -d postgres
```

Set the homeserver database URL in `~/.pubky/config.toml`:

```toml
[general]
database_url = "postgres://postgres:postgres@localhost:5432/pubky_homeserver"
```

### Native Ubuntu

In order for LND to run on Postgres, an empty database should already exist. A database can be created via the usual ways (psql, pgadmin, etc.). A user with access to this database is also required.

Install postgres

```bash
sudo apt update
sudo apt install postgresql postgresql-contrib
```

The install creates a new postgres Linux user. Use this command to create the database:

```bash
sudo -u postgres createuser --superuser pubky
sudo -u postgres psql -c 'create database pubky_homeserver;'
```

Set the homeserver database URL in `~/.pubky/config.toml`:

```toml
[general]
database_url = "postgres://postgres:postgres@:5432/pubky_homeserver"




## First Run

Start the homeserver:

```bash
pubky-homeserver
```

Or from source:

```bash
cargo run -p pubky-homeserver
```

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

## Default Endpoints

| Endpoint | Default |
| --- | --- |
| Public HTTP API | `http://127.0.0.1:6286` |
| Pubky TLS API | `127.0.0.1:6287` |
| Admin API | `http://127.0.0.1:6288` |
| Metrics API | `http://127.0.0.1:6289` |

## Signup Tokens

Standalone homeservers require signup tokens by default. Generate one through the admin API:

```bash
curl -X GET "http://127.0.0.1:6288/generate_signup_token" \
  -H "X-Admin-Password: admin"
```

Change the admin password before exposing a homeserver beyond local development:

```toml
[admin]
admin_password = "change-me"
```

You can also open signup entirely for a private or temporary deployment:

```toml
[general]
signup_mode = "open"
```

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

- Use a persistent PostgreSQL instance and back it up.
- Back up the homeserver `secret` file and any filesystem or bucket storage.
- Do not expose the admin or metrics APIs to the public internet.
- Change the default admin password.
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

For the Docker example above, check the container:

```bash
docker ps --filter name=pubky-postgres
```

### Port Already In Use

Change the relevant listen socket in `~/.pubky/config.toml`:

```toml
[drive]
icann_listen_socket = "127.0.0.1:6286"
pubky_listen_socket = "127.0.0.1:6287"

[admin]
listen_socket = "127.0.0.1:6288"

[metrics]
listen_socket = "127.0.0.1:6289"
```

### Invalid Configuration

Compare your config with [`pubky-homeserver/config.sample.toml`](../pubky-homeserver/config.sample.toml). If the config was generated on first run, the file is safe to edit in place.
