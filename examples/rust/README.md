# Pubky examples

Minimal examples for different flows and functions you might need to implement using Pubky.

## Utilities

- [**keygen**](./keygen.rs): Generate a keypair and save a passphrase-encrypted recovery file. Required by examples 2, 3, and 4 (write).

## Examples

0. [**Logging**](./0-logging/README.md): configure tracing and watch the SDK emit debug output during a storage roundtrip.
1. [**Testnet**](./1-testnet/README.md): shows how to build a pubky app offline against a local ephemeral homeserver.
2. [**Authentication**](./2-signup/README.md): shows how to signup, signin or signout to and from a homeserver.
3. [**Authorization Flow**](./3-auth_flow/README.md): shows how to setup Pubky authz for a 3rd party application and how to implement an authenticator to sign in such app.
4. [**Storage**](./4-storage/README.md): public reads and authenticated writes on homeserver storage.
5. [**Request**](./5-request/README.md): shows how to make direct HTTP requests to Pubky URLs.
6. [**Signup Authorization Flow**](./6-auth_flow_signup/README.md): shows how to setup Pubky authz for a 3rd party application and how to implement an authenticator to sign up such app.
7. [**Event Stream**](./7-events_stream/README.md): subscribe to Server-Sent Events from a user's homeserver.
