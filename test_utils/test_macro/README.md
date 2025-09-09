# pubky_test_macro

A Rust procedural macro crate that provides test wrappers with automatic database cleanup at the end of async test execution.

## Features

- **`#[pubky_testcase]`**: Wraps async test functions and executes `drop_dbs()` regardless of test outcome
- **Async-focused**: Designed for async functions with tokio runtime integration
- **Panic-safe**: The database cleanup executes even if the test panics
- **Flexible**: Works with any test attribute (`#[tokio::test]`, `#[ignore]`, etc.)
- **Database Management**: Automatically drops all registered test databases after each test

## Usage

Add this crate to your `Cargo.toml`:

```toml
[dependencies]
pubky_test_macro = "0.1.0"
```

### Asynchronous Tests

```rust
use pubky_test_macro::pubky_testcase;

#[tokio::test]
#[pubky_testcase]
async fn my_async_test() {
    // Your async test logic here
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    assert_eq!(3 + 3, 6);
}
```

### With Other Test Attributes

```rust
#[tokio::test]
#[ignore]
#[pubky_testcase]
async fn ignored_test() {
    // This test is ignored but would still print if run
    assert_eq!(1 + 1, 2);
}
```

## Database Cleanup

The macro will automatically execute `drop_dbs()` after each test, which:
- Drops all databases that were registered during the test execution
- Cleans up the global registry of databases to drop
- Executes regardless of whether the test passes, fails, or panics
- Works in async test contexts with tokio runtime

## Database Registration

To register a database for cleanup, use the `register_db_to_drop` function from the `pubky_test_utils` crate:

```rust
use pubky_test_utils;

#[tokio::test]
#[pubky_testcase]
async fn my_test() {
    // Create a test database
    let pool = create_test_database().await;
    
    // Register it for cleanup
    pubky_test_utils::register_db_to_drop("test_db_name".to_string(), pool).unwrap();
    
    // Your test logic here
    // The database will be automatically dropped after the test
}
```

## Dependencies

Add both crates to your `Cargo.toml`:

```toml
[dependencies]
pubky_test_macro = "0.1.0"
pubky_test_utils = "0.1.0"
```

## Requirements

- Rust 1.70+
- `tokio` runtime (the macro requires `#[tokio::test]` for async test execution)

## License

This project is licensed under the same terms as the main pubky project.
