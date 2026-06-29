# Pubky Homeserver

Reference homeserver implementation for Pubky. Stores and serves user data via HTTP APIs with public-key authentication.

For standalone installation and operation, see [Install and Run Pubky Homeserver](../docs/INSTALL.md). For local app development, use the [local testnet guide](../docs/LOCAL_DEVELOPMENT.md).

## Usage

### Library

Use the homeserver as a library in other crates or for testing.

`HomeserverApp` starts the full server stack (client server, admin server, metrics server, DHT republishers):

```rust
use pubky_homeserver::HomeserverApp;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let app = HomeserverApp::start_with_persistent_data_dir_path(
        PathBuf::from("~/.pubky")
    ).await?;

    println!("Homeserver HTTP: {}", app.icann_http_url());
    println!("Homeserver Pubky TLS: {}", app.pubky_url());

    if let Some(admin) = app.admin_server() {
        println!("Admin server: http://{}", admin.listen_socket());
    }

    tokio::signal::ctrl_c().await?;
    Ok(())
}
```

For testing, use `MockDataDir` to create a temporary directory that is cleaned up on drop:

```rust,ignore
use pubky_homeserver::{HomeserverApp, MockDataDir, ConfigToml};

let config = ConfigToml::default_test_config();
let mock_dir = MockDataDir::new(config, None).unwrap();
let app = HomeserverApp::start_with_mock_data_dir(mock_dir).await.unwrap();
```

### Binary

See [Install and Run Pubky Homeserver](../docs/INSTALL.md) for full setup instructions.

```bash
pubky-homeserver --data-dir ~/.pubky
```

## Signup Token

If the homeserver is set to require signup tokens, create one using the admin endpoint:

```bash
curl "http://127.0.0.1:6288/generate_signup_token" \
     -H "X-Admin-Password: admin"
     # Use your admin password. "admin" is the testnet default.
```

Or from JavaScript:

```js
const url = "http://127.0.0.1:6288/generate_signup_token";
const response = await client.fetch(url, {
  method: "GET",
  headers: {
    "X-Admin-Password": "admin", // use your admin password, defaults to testnet password.
  },
});
const signupToken = await response.text();
```
