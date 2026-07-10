# Pubky examples

Minimal examples for different flows and functions you might need to implement using Pubky.

## Utilities

- [**sample_recovery.key**](../sample_recovery.key): Sample recovery file with an empty passphrase, used by default in examples 1, 2, 3, and 7.
- [**keygen**](./keygen.rs): Generate a keypair and save a passphrase-encrypted recovery file when you want to use your own key.

## Examples

1. [**Authentication**](./1-signup/README.md): shows how to signup, signin or signout to and from a homeserver.
2. [**Authorization Flow**](./2-auth_flow/README.md): shows how to set up Pubky grant auth with a headless third-party client and an authenticator CLI.
3. [**Storage**](./3-storage/README.md): authenticated write, read, and delete lifecycle on homeserver storage.
4. [**Request**](./4-request/README.md): shows how to make direct HTTP requests to Pubky URLs.
5. [**Signup Authorization Flow**](./5-auth_flow_signup/README.md): shows how to setup Pubky authz for a 3rd party application and how to implement an authenticator to sign up such app.
6. [**Event Stream**](./6-events_stream/README.md): subscribe to Server-Sent Events from a user's homeserver.
7. [**Session Management**](./7-session_management/README.md): create, list, and delete grant-backed sessions from the command line.
8. [**Logging**](./8-logging/README.md): configure tracing and watch the SDK emit debug output during a storage roundtrip.
9. [**Testnet**](./9-testnet/README.md): shows how to build a pubky app offline against a local ephemeral homeserver.
