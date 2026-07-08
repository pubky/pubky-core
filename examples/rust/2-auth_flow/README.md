# Pubky Grant Auth Signin Example

This example shows third-party grant authorization in Pubky.

The third-party app starts a grant auth flow, displays a Pubky Auth deep link with `cid` and `cpk` query parameters, and waits for a grant-backed session. The authenticator approves the request by signing a `pubky-grant` JWS, which the app exchanges for a self-refreshing session.

It consists of 2 parts:

1. [3rd party app](./3rd-party-app): A web component showing how to implement a Pubky Grant Auth widget.
2. [Authenticator CLI](./authenticator.rs): A CLI showing the authenticator (key chain) asking the user for consent and delivering the signed grant.

## Recovery File

This example defaults to `../sample_recovery.key`, which has an empty passphrase.
If that sample key cannot be decrypted with an empty passphrase, the CLI prompts for a passphrase.

(Optional) Generate a recovery file using the [keygen utility](../keygen.rs) when you want to use your own key:

```bash
cargo run --bin keygen
```

## Usage

First you need to be running a local testnet Homeserver, in the root of this repo run

```bash
cargo run -p pubky-testnet
```

Run the frontend of the 3rd party app

```bash
cd ./3rd-party-app
npm i
npm start
```

Copy the Pubky Auth URL from the frontend. It should use the `signin_grant` intent and include `cid` and `cpk` query parameters.

Finally run the CLI to paste the Pubky Auth in.

```bash
cargo run --bin authenticator "<Auth_URL>" --testnet

# with a custom recovery file
cargo run --bin authenticator "<Auth_URL>" --testnet --recovery-file <RECOVERY_FILE>
```

Where the auth URL should be within quotation marks, and `--testnet` uses the local homeserver.

You should see the frontend react by showing successful authorization and grant session details.
