# Pubky app

A [crux](https://github.com/redbadger/crux) based cross-platform application.

This code is based on the counter example. For the moment, only a Tauri shell is supported.

## Rust shared library

1. Make sure the core builds

```sh
cargo build --package shared
```

2. Generate the shared types for your client applications

```sh
cargo build --package shared_types
```

## Tauri

To build and run the [Tauri](https://tauri.app/) app:

```sh
cd tauri
pnpm install
pnpm tauri dev
```
