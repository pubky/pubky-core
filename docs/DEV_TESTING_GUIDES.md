# Developer / Testing Guides

This documents describes common problems and their solution that developers or tester encounter.


## Postgres

The easiest way to run postgres is with a docker container

```bash
docker run --name postgres \
  -e POSTGRES_USER=postgres \
  -e POSTGRES_PASSWORD=postgres \
  -e POSTGRES_DB=pubky_homeserver \
  -p 5432:5432 \
  -d postgres:17
```

This command creates a postgres container and also automatically creates the `pubky_homeserver` database. Use this connection string in the homeserver config:

```toml
[general]
database_url = "postgres://postgres:postgres@localhost:5432/pubky_homeserver"
```

[pgadmin](https://www.pgadmin.org/) is a great explorer to inspect database values.

### Create Database Manually

The docker image creates the database automatically. If you have postgres installed with a different method, you need to create the database manually.

This can be done with psql.

```bash
sudo -u postgres psql -c 'create database pubky_homeserver;'
```


## Test Databases

If compiled with the `testing` feature, `?pubky-test=true` can be added to the database url.
This way, an empheral test database is created and dropped after the test.

**Example** `postgres://postgres:postgres@localhost:5432/postgres?pubky-test=true` For each test, the homeserver will connect to the
specified database (postgres in this case) and create a new test database. With the `#[pubky_test_utils::test]` macro, the test database is
dropped again after the test completes/panics.
