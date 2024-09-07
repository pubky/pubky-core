# Pubky Auth Example

This example shows 3rd party authorization in Pubky.

It consists of 2 parts:

1. [3rd party app](./3rd-party-app): A web component showing the how to implement a Pubky Auth widget.
2. [Authenticator CLI](./authenticator): A CLI showing the authenticator (key chain) asking user for consent and generating the AuthToken.

## Usage

First you need to be running a local testnet Homeserver, in the root of this repo run

```bash
cargo run --bin pubky_homeserver -- --testnet
```

Run the frontend of the 3rd party app

```bash
cd ./3rd-party-app
npm start
```

Copy the Pubky Auth URL from the frontend.

Finally run the CLI to paste the Pubky Auth in.

You should see the frontend reacting by showing the success of authorization and session details.
