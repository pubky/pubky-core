# Authentication examples

You can use these examples to test Signup or Signin to a provided homeserver using a keypair,
as opposed to using a the 3rd party [authorization flow](../3-auth_flow).

## Prerequisites

Generate a recovery file first using the [keygen utility](../keygen.rs):

```bash
cargo run --bin keygen
# or specify a custom output path
cargo run --bin keygen -- -o my_key.recovery
```

## Usage

### Signup

```bash
cargo run --bin signup <homeserver pubky> </path/to/recovery file> <signup_token>

# or use the local testnet defaults
cargo run --bin signup -- --testnet pubky8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo </path/to/recovery file>
```
