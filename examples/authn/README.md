# Authentication examples

You can use these examples to test Signup or Signin to a provided homeserver using a keypair,
as opposed to using a the 3rd party [authorization flow](../authz).

## Usage

### Signup

```bash
cargo run --bin signup <homeserver pubky> </path/to/recovery file> <signup_token>
```
