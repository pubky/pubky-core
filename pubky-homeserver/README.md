# Pubky Homeserver

Pubky homeserver that acts as user's agent on the Internet, providing data availability and more.

## Usage

### Library

Use the Homeserver as a library in other crates/binaries or for testing purposes.
The `HomeserverSuite` is all bells and wistles included.

```rust
use anyhow::Result;
use pubky_homeserver::HomeserverSuite;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  let suite = HomeserverSuite::run_with_data_dir_path(PathBuf::from("~/.pubky")).await?;
  println!(
      "Homeserver HTTP listening on {}",
      server.core().icann_http_url()
  );
  println!(
      "Homeserver Pubky TLS listening on {} and {}",
      server.core().pubky_tls_dns_url(),
      server.core().pubky_tls_ip_url()
  );
  println!(
      "Admin server listening on http://{}",
      server.admin().listen_socket()
  );
  tokio::signal::ctrl_c().await?;

  println!("Shutting down Homeserver");
  Ok(())
}
```

Run the suite with a temporary directory and your custom config. This is a good way to test the server.

```rust
use anyhow::Result;
use pubky_homeserver::{HomeserverSuite, DataDirMock};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  let mut config = ConfigToml::default(); // Use ConfigToml::test() for random ports.
  // Set config values however you like
  config.admin.admin_password = "alternative_password".to_string();
  // Creates a temporary directory that gets cleaned up 
  // as soon as the suite is dropped.
  let mock_dir = DataDirMock::new(config, None).unwrap(); 
  let suite = HomeserverSuite::run_with_data_dir_mock(mock_dir).await.unwrap();
}


Run the `HomeserverCore` only without the admin server.

```rust
use anyhow::Result;
use pubky_homeserver::HomeserverCore;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut core = HomeserverCore::from_data_dir_path(PathBuf::from("~/.pubky")).await?;
    core.listen().await?;
    println!(
        "Homeserver HTTP listening on {}",
        core().icann_http_url()
    );
    println!(
        "Homeserver Pubky TLS listening on {} and {}",
        core().pubky_tls_dns_url(),
        core().pubky_tls_ip_url()
    );
}
```

### Binary

Use `cargo run -- --data-dir=~/.pubky`.

## Signup Token

If homeserver is set to require signup tokens, you can create a new signup token using the admin endpoint:

```rust,ignore
let response = pubky_client
    .get(&format!("http://127.0.0.1:6288/generate_signup_token"))
    .header("X-Admin-Password", "admin") // Use your admin password. This is testnet default pwd.
    .send()
    .await
    .unwrap();
let signup_token = response.text().await.unwrap();
```

via CLI with `curl`

```bash
curl -X GET "http://127.0.0.1:6288/generate_signup_token" \
     -H "X-Admin-Password: admin"
     # Use your admin password. This is testnet default pwd.
```

or from JS

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