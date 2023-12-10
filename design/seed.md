# Seed

Kytz seed is an encrypted seed encoded as URI as follows:

```
kytz:seed:<suffix>
```

The `suffix` is a `z-base32` encoded bytes as follows:

```
<VERSION><Encrypted seed ciphertext>
```

## Version 0

For version 0 the encrypted seed is using [`bessie`](https://github.com/oconnor663/bessie/blob/44f9500ebeb0f28efc9689184ff5b1d79e2308e0/design.md) version 0.0.1

