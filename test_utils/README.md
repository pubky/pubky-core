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


This is a script to delete left-over test databases:

```bash
#!/bin/bash

# Configuration
PGUSER="postgres"         # Change if your PostgreSQL superuser is different

# The pattern of databases to drop
PATTERN="pubky_test_%"

# SQL query to generate DROP DATABASE statements for matching databases
SQL_QUERY="SELECT 'DROP DATABASE \"' || datname || '\";' 
           FROM pg_database 
           WHERE datistemplate = false 
           AND datname LIKE '${PATTERN}';"

# Generate the drop statements into a temporary file
psql -U "$PGUSER" -d postgres -t -A -c "$SQL_QUERY" > drop_dbs.sql

# Execute the generated DROP DATABASE statements
if [[ -s drop_dbs.sql ]]; then
    echo "Dropping the following databases:"
    cat drop_dbs.sql

    psql -h "$PGHOST" -p "$PGPORT" -U "$PGUSER" -d postgres -f drop_dbs.sql
    echo "Databases dropped successfully."
else
    echo "No databases found matching pattern '${PATTERN}'."
fi

# Cleanup
rm -f drop_dbs.sql
```