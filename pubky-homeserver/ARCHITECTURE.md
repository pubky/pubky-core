# Pubky Homeserver Architecture

This document describes the internal architecture of the `pubky-homeserver` crate: how the code is organized, what each component does, and how they interact. It's intended to give developers (and LLMs) a high-level map before diving into the source.


## System Overview

The homeserver is a multi-tenant data server that stores and serves user data, keyed by Ed25519 public keys. It exposes three independent HTTP servers, runs background republishers for DHT presence, and persists data to PostgreSQL and a pluggable file storage backend.

```
┌─────────────────────────────────────────────────────────┐
│                    HomeserverApp                        │
│                                                         │
│  ┌──────────────┐ ┌──────────────┐ ┌─────────────────┐  │
│  │ ClientServer │ │ AdminServer  │ │  MetricsServer  │  │
│  │  :6286 HTTP  │ │    :6288     │ │     :6289       │  │
│  │  :6287 TLS   │ │              │ │                 │  │
│  └──────┬───────┘ └──────┬───────┘ └────────┬────────┘  │
│         │                │                  │           │
│         └────────┬───────┘──────────────────┘           │
│                  │                                      │
│           ┌──────▼──────┐                               │
│           │  AppContext  │  (shared dependency container)│
│           └──────┬──────┘                               │
│         ┌────────┼──────────┬──────────────┐            │
│    ┌────▼────┐ ┌─▼──────┐ ┌▼───────────┐ ┌▼─────────┐  │
│    │  SqlDb  │ │  File  │ │  Events    │ │  Pkarr   │  │
│    │ (Postgres)│ Service │ │  Service   │ │  Client  │  │
│    └─────────┘ └────────┘ └────────────┘ └──────────┘  │
│                                                         │
│  ┌───────────────────────┐ ┌──────────────────────────┐ │
│  │ UserKeysRepublisher   │ │ HomeserverKeyRepublisher │ │
│  │ (DHT, every 30+ min)  │ │ (DHT, every 1 hour)     │ │
│  └───────────────────────┘ └──────────────────────────┘ │
└─────────────────────────────────────────────────────────┘
```

## Entry Points

- **Binary** (`main.rs`): Parses `--data-dir` (defaults to `~/.pubky`), loads config, starts the app, and waits for Ctrl+C.
- **Library** (`lib.rs`): Exports `HomeserverApp`, individual server types, `AppContext`, and configuration types for embedding in other applications.

## Core Components

### HomeserverApp (`homeserver_app.rs`)

Top-level orchestrator. Owns the three servers, two republishers, and the shared `AppContext`. Startup sequence:

1. Load config and keypair from data directory
2. Initialize `AppContext` (connects to Postgres, runs migrations, sets up file storage)
3. Start ClientServer, AdminServer, MetricsServer
4. Start background republishers

### AppContext (`app_context.rs`)

Dependency injection container, cheaply cloneable (Arc internals). Holds:

| Field | Type | Purpose |
|-------|------|---------|
| `sql_db` | `SqlDb` | PostgreSQL connection pool |
| `file_service` | `FileService` | Blob storage abstraction |
| `config_toml` | `ConfigToml` | Parsed configuration |
| `keypair` | `Keypair` | Server's Ed25519 identity |
| `pkarr_client` | `pkarr::Client` | DHT client for key publishing/resolution |
| `events_service` | `EventsService` | In-memory broadcast + DB persistence for SSE |
| `metrics` | `Metrics` | Prometheus counters/histograms |

## Servers

### Client Server (`client_server/`)

The main data API. Listens on **two sockets**:

- **ICANN HTTP** (default `:6286`): Plain HTTP. Expects a reverse proxy for TLS in production.
- **Pubky TLS** (default `:6287`): TLS using the server's Ed25519 keypair (Raw Public Key / RustLS). Clients verify the server's identity via its public key.

Both sockets serve the same router, the difference is transport security, not available routes.

#### Route Structure

**Server-level routes:**

| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/` | Server info |
| `POST` | `/signup` | User registration |
| `GET` | `/signup_tokens/{token}` | Check token validity |
| `POST` | `/session` | User login |
| `GET` | `/events/` | Historical event feed (plain text) |
| `GET` | `/events-stream` | Live SSE event stream |

**Per-tenant routes** (pubkey extracted from Host header via `PubkyHostLayer`):

| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/{path}` | Read file or list directory |
| `HEAD` | `/{path}` | File metadata |
| `PUT` | `/{path}` | Write file (authenticated) |
| `DELETE` | `/{path}` | Delete file (authenticated) |
| `GET` | `/session` | Get current session |
| `DELETE` | `/session` | Logout |

#### Middleware Stack (`layers/`)

Tower layers execute outside-in (last added runs first on the request). The execution order for incoming requests is:

1. **TraceLayer** — Request/response logging (outermost)
2. **PubkyHostLayer** — Extracts tenant pubkey from Host header
3. **RateLimiter** — Configurable per-path rate limits
4. **CORS** — Permissive cross-origin access
5. **CookieManager** — Manages session cookies
6. **AuthorizationLayer** — Enforces read/write permissions based on session capabilities (innermost, applied only to tenant routes)

#### Authorization Rules

- **Public reads** (`GET /pub/*`): Always allowed, no auth needed
- **Non-public reads** (`GET /dav/*`, etc.): Forbidden
- **Writes** (`PUT`, `DELETE` to `/pub/*`): Require a valid session cookie with write capability for the target path

### Admin Server (`admin_server/`)

Control plane (default `:6288`). Protected routes require `X-Admin-Password` header.

| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/generate_signup_token` | Create signup token |
| `GET` | `/info` | Server metadata |
| `DELETE` | `/webdav/{path}` | Admin file deletion |
| `POST` | `/users/{pubkey}/disable` | Disable user |
| `POST` | `/users/{pubkey}/enable` | Re-enable user |

Also exposes a **WebDAV** interface at `/dav/*` for file management via standard file managers (Finder, Explorer).

### Metrics Server (`metrics_server/`)

Optional Prometheus endpoint (default `:6289`). Exposes event stream connections, query latencies, and broadcast channel health.

## Persistence

### PostgreSQL (`persistence/sql/`)

Primary data store. Migrations run automatically on startup.

**Tables:**

| Table | Key Fields | Purpose |
|-------|------------|---------|
| `users` | `public_key`, `disabled`, `used_bytes` | User registry and quota tracking |
| `sessions` | `secret`, `capabilities` | Authentication sessions |
| `entries` | `user_id`, `path`, `content_hash`, `content_type` | File metadata index |
| `events` | `id` (cursor), `type` (PUT/DEL), `path`, `content_hash` | Event history for SSE |
| `signup_codes` | `id`, `used_by` | Token-gated registration |

The codebase uses a **Repository pattern** (`UserRepository`, `SessionRepository`, `EntryRepository`, `EventRepository`, `SignupCodeRepository`) to encapsulate SQL logic, with `sea-query` for query building. A `UnifiedExecutor` abstraction allows the same repository code to work with both connection pools and transactions.

### File Storage (`persistence/files/`)

Blob storage via OpenDAL. Three backends:

- **Filesystem** (default): Files in `~/.pubky/data/files/`
- **In-Memory**: For tests
- **Google Cloud Storage**: With `storage-gcs` feature flag

File operations pass through a layered middleware stack (outermost first):

1. **EventsLayer** — Creates event records after inner layers complete on close
2. **EntryLayer** — Updates file metadata (hash, size, MIME type) in Postgres
3. **UserQuotaLayer** — Enforces per-user storage quotas
4. **OpenDAL base** — Physical storage I/O

### LMDB (Legacy)

Previously the primary store, now being phased out. An automatic LMDB→SQL migration runs on startup if legacy data exists. LMDB code remains for backward compatibility but is not used for new data.

## Event System

The homeserver provides a real-time event stream so clients and indexers can track changes.

**Two endpoints:**

1. **`GET /events/`** — Historical plain-text feed with cursor-based pagination
2. **`GET /events-stream`** — Server-Sent Events with two phases:
   - **Batch phase**: Replays historical events from the database
   - **Live phase**: Streams real-time events via an in-memory broadcast channel (capacity: 1000)

**Event types:** `PUT` (with content hash) and `DEL`.

**Flow:** File write/delete → EventsLayer creates DB record within transaction → after commit, broadcasts to SSE subscribers.

## Background Republishers

### UserKeysRepublisher

Periodically republishes all users' public keys to the Mainline DHT so they remain discoverable via Pkarr. Configurable interval (minimum 30 minutes, disabled by default).

### HomeserverKeyRepublisher

Publishes the homeserver's own identity and endpoint information (SVCB, A/AAAA DNS records) to the DHT every hour. This is how clients discover the homeserver's address from a public key.

## Authentication Flow

1. **Signup** (`POST /signup`): Client sends an `AuthToken` (public key + signature). Server verifies signature, optionally validates a signup token, creates user and session in a transaction, returns a session cookie.
2. **Signin** (`POST /session`): Same as signup but the user must already exist.
3. **Session cookie**: Named `{user_pubkey_z32}`, value is a random secret. On write requests, the `AuthorizationLayer` looks up the session, verifies the user matches the target tenant, and checks capabilities.

Capabilities use the format from `pubky-common` (e.g., `/pub/:rw` for read-write access to `/pub/*`).

## Configuration

Config lives at `~/.pubky/config.toml`, merged on top of embedded defaults. Key sections:

| Section | Controls |
|---------|----------|
| `[general]` | Signup mode (public/token/disabled), storage quota, database URL |
| `[drive]` | Listen addresses, rate limit rules |
| `[storage]` | Backend type (filesystem, memory, GCS) |
| `[admin]` | Admin server enable/disable, password |
| `[metrics]` | Metrics server enable/disable |
| `[pkdns]` | Public IP, ports, DHT bootstrap/relay nodes, republisher intervals |
| `[logging]` | Log level and per-module overrides |

## Key Data Flow: Writing a File

1. Request arrives at ClientServer (HTTP or TLS)
2. `PubkyHostLayer` extracts tenant pubkey from Host header
3. `AuthorizationLayer` validates session cookie and write capability
4. Handler streams request body to `FileService`
5. `UserQuotaLayer` checks storage limit
6. `EntryLayer` computes blake3 hash and updates metadata in Postgres
7. `EventsLayer` creates a PUT event in the database (within transaction)
8. Transaction commits
9. Event is broadcast to SSE subscribers
10. 200 OK returned to client
