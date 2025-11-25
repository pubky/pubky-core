In this repo you can:

Run clippy with this command: `cargo clippy --workspace --all-features --exclude pubky-wasm -- -D warnings`
Run tests which require Postgres access by eg: `TEST_PUBKY_CONNECTION_STRING=postgres://postgres:postgres@localhost:5432/pubky_homeserver?pubky-test=true cargo test -p e2e list_deep`