# Pubky Homeserver

A pubky-core homeserver that acts as users' agent on the Internet, providing data availability and more.

## Usage

### Library

You can use the Homeserver as a library in other crates/binaries or for testing purposes.

```rust
use anyhow::Result;
use pubky_homeserver::Homeserver;

#[tokio::main]
async fn main() {
    let homeserver = unsafe {
        Homeserver::builder().run().await.unwrap()
    };

    println!("Shutting down Homeserver");

    homeserver.shutdown();
}
```

If homeserver is set to require signup tokens, you can create a new signup token using the admin endpoint:

```rust,ignore
let response = pubky_client
    .get(&format!("https://{homeserver_pubkey}/admin/generate_signup_token"))
    .header("X-Admin-Password", "admin") // Use your admin password. This is testnet default pwd.
    .send()
    .await
    .unwrap();
let signup_token = response.text().await.unwrap();
```

via CLI with `curl`

```bash
curl -X GET "https://<homeserver_ip:port>/admin/generate_signup_token" \
     -H "X-Admin-Password: admin"
     # Use your admin password. This is testnet default pwd.
```

or from JS

```js
const url = "http://${homeserver_address}/admin/generate_signup_token";
const response = await client.fetch(url, {
  method: "GET",
  headers: {
    "X-Admin-Password": "admin", // use your admin password, defaults to testnet password.
  },
});
const signupToken = await response.text();
```

### Binary

Use `cargo run`

```bash
cargo run -- --config=./src/config.toml
```

Or Build first then run from target.

Build

```bash
cargo build --release
```

Run with an optional config file

```bash
../target/release/pubky-homeserver --config=./src/config.toml
```
