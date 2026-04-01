# Rate Limits & Per-User Resource Quotas

The homeserver enforces two independent layers of protection:

1. **Global path-based rate limits** — Speed caps (`5mb/s`) throttle individual streams via backpressure; request-count limits (`10r/m`) gate access to sensitive endpoints. Applied per-request, keyed by IP or user.
2. **Per-user resource quotas** — Bandwidth budgets (`500mb/d`) cap total bytes that can be read from or written to a user's stored resources per time window. Once exhausted, further requests against that user's data are rejected until the window resets.


---

## Global Path-Based Rate Limits

For restricting request volume or bandwidth on specific HTTP paths. Useful for protecting sensitive endpoints (e.g. signup token validation) from brute-force or abuse.

Two quota types are supported:

| Type | Example | Behaviour |
|------|---------|-----------|
| Request count | `10r/m` | 10 requests per minute. Exceeding returns `429`. |
| Bandwidth speed | `5mb/s` | Throttles streaming to 5 MB/s. Applies to both upload and download. |


### Configuration

Defined in `config.toml` under `[[drive.rate_limits]]` (an array of tables):

```toml
[[drive.rate_limits]]
path = "/signup_tokens/*"   # Glob pattern (fast_glob syntax)
method = "GET"              # HTTP method
quota = "10r/m"             # Rate: {count}{unit}/{time}
key = "ip"                  # "ip" or "user"
# burst = 5                 # Optional: burst size (defaults to the rate value)
# whitelist = ["127.0.0.1"] # Optional: exempt these IPs or user pubkeys
```

#### Quota format

```
{rate}{unit}/{time_unit}
```

- **rate**: A positive integer.
- **unit**: `r` (requests), `kb`, `mb`, or `gb`.
- **time_unit**: `s` (second), `m` (minute), `h` (hour), `d` (day).

Examples: `10r/m`, `1mb/s`, `100gb/d`.

---

## Per-User Resource Quotas

Four independent quotas enforced per user's resources (identified by the tenant pubkey in the request hostname):

| Quota | Config field | Purpose |
|-------|-------------|-----------|
| Storage quota | `storage_quota_mb` | Maximum total bytes a user can store. |
| Max sessions | `max_sessions` | Maximum concurrent authenticated sessions. |
| Read bandwidth budget | `rate_read` | Total bytes that can be read from a user's data per time window. |
| Write bandwidth budget | `rate_write` | Total bytes that can be written to a user's data per time window. |

For all four fields, `null` / `None` means **unlimited**.

### Configuration

#### Deploy-time defaults (config.toml)

Set in the `[quotas]` section. These apply to **newly created users** and were backfilled onto existing users during the initial migration:

```toml
[quotas]
storage_quota_mb = 500        # 500 MB quota. Omit for unlimited.
max_sessions = 10             # 10 concurrent sessions. Omit for unlimited.
rate_read = "500mb/d"         # 500 MB read per day. Omit for unlimited.
rate_write = "100mb/h"        # 100 MB write per hour. Omit for unlimited.
```

After the one-time database migration, changing these values in config.toml only affects users created afterwards. Existing users keep whatever was written to their DB row.

#### Bandwidth budget format

Bandwidth budgets use the same format as global quota values, but **only accept bandwidth units** (`kb`, `mb`, `gb`). Request-count units (`r`) are rejected.

```
{rate}{unit}/{time_unit}
```

Examples: `500mb/d` (500 MB per day), `2gb/h` (2 GB per hour), `10mb/m` (10 MB per minute).

The budget is a **total bytes** allowance per time window, not a speed cap. Once the budget is exhausted, subsequent requests return `429 Too Many Requests` with a `Retry-After` header. The window resets after the configured time period.

#### Per-user overrides (Admin API)

Override resource quotas for a specific user via the admin server:

**GET** `/users/{pubkey}/resource-quotas` — read current resource quotas.

**PUT** `/users/{pubkey}/resource-quotas` — replace all resource quotas. **All four fields are required.** Use `null` for unlimited. Omitting a field returns `422`.

```json
{
  "storage_quota_mb": 500,
  "max_sessions": 10,
  "rate_read": "500mb/d",
  "rate_write": null
}
```

**DELETE** `/users/{pubkey}/resource-quotas` — clear all custom resource quotas (sets all DB columns to NULL). The user becomes **fully unlimited**. This does not revert to deploy-time defaults.
