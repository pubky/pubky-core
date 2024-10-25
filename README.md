<h1 align="center"><a href="https://pubky.org/"><img alt="pubky" src="./.svg/pubky-core-logo.svg" width="200" /></a></h1>

<h3 align="center">
	An open protocol for per-public-key backends for censorship resistant web applications.
</h3>

<div align="center">
  <h3>
    <a href="https://pubky.github.io/pubky-core/">
      Docs Site
    </a>
    <span> | </span>
    <a href="https://docs.rs/pubky">
      Rust Client's Docs
    </a>
    <span> | </span>
    <a href="https://github.com/pubky/pubky-core/releases">
      Releases
    </a>
    <span> | </span>
    <a href="https://www.npmjs.com/package/@synonymdev/pubky">
      JS bindings 
    </a>
  </h3>
</div>

> The Web, long centralized, must decentralize; Long decentralized, must centralize.

## Overview

Pubky-core combines a [censorship resistant public-key based alternative to DNS](https://pkarr.org) with conventional, tried and tested web technologies, to keep users in control of their identities and data, while enabling developers to build software with as much availability as web apps, without the costs of managing a central database.

## Features
- Public key based authentication.
- Public key based 3rd party authorization.
- Key-value store through PUT/GET/DELET HTTP API + pagination.

## Getting started

This repository contains a [Homeserver](./pubky-homeserver), and a [Client](./pubky) (both Rust and JS wasm bindings).
You can a run a local homeserver using `cargo run` with more instructions in the README.
Check  the [Examples](./examples) directory for small feature-focesed examples of how to use the Pubky client.

### JavaScript
If you prefer to use JavaScript in NodeJs/Browser or any runtime with Wasm support, you can either install from npm [`@synonymdev/pubky`](https://www.npmjs.com/package/@synonymdev/pubky)
or build the bindings yourself:
```bash
cd pubky/pkg
npm i
npm run build
```

#### Testing
There are unit tests for the JavaScript bindings in both NodeJs and headless web browser, but first you need to run a local temporary Homeserver
```bash
npm run testnet
```
Then in a different terminal window:
```bash
npm test
```
