# Getting Started

## Install Binaries

Downloading and installing an official release binary is recommended for use on mainnet. [Visit the release page on GitHub](https://github.com/pubky/pubky-core/releases) and select the latest version that does not have the "Pre-release" label set (unless you explicitly want to help test a Release Candidate, RC).

Choose the package that best fits your operating system and system architecture. It is recommended to choose 64bit versions over 32bit ones, if your operating system supports both.

Extract the package and place the binary (pubky-homeserver or pubky-homeserver.exe on Windows) somewhere where the operating system can find them.

## Run

> The homeserver depends on Postgresql as its database backend. Make sure you have it installed.

Executing the `pubky-homeserver` binary will create the `~/.pubky` application folder. All data related to the homeserver is stored there. A sample `config.toml` is written to app folder.
It includes all important configuration values. 

For the homeserver to start, it needs the correct `database_url` to connect to the postgres database. 

```toml
# Example database url in ~/.pubky/config.toml
database_url = "postgres://username:password@localhost:5432/pubky_homeserver"
```

Make sure to create the `pubky_homeserver` database in postgres so the homeserver can connect to it.

Setting the database_url is enough to run the homeserver locally:

```bash
./pubky_homeserver
```



