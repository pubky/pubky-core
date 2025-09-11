# pubky_test

Main library for the test macro.

Merges the `drop_db_helper` and `test_macro` into one crate.

Procedural macros in rust must be in their own crate. It can't provide additional methods. That's why 3 crates are needed.

- `drop_db_helper` provides the base methods to drop test databases.
- `test_macro` uses drop_db_helper to drop the test databases after the test completes.
- `pubky_test` provides one convenient crate for both.