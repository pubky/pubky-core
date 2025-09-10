# pubky_test_utils

Provides a test macro for the homeserver postgres test database so the database is always cleaned up after
the test completes.

See README's of the respective crates for more info.

## Usage

Install `pubky_test_utils` or `pubky_testnet`. It works with both.
The test must be `async` and wrap `#[tokio::test]`. Other async runners are not supported.

```rust
#[tokio::test]
#[pubky_test_utils::test] // Or #[pubky_testnet::test]
async fn my_test() {
    // Any SqlDb::test() (used in the homeserver) created postgres database
    // will be cleaned up by `#[pubky_test_utils::test]` after the test completed/paniced.
}
```

## Edge Case

Test databases are dropped after the test completes or panics. Aborting the test with a single CTRL+C works too. It will not be dropped if the test is manually killed or stopped with a double CTRL+C. 