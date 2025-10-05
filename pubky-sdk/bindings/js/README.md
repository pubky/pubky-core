# JS Pubky SDK bindings

Wasm-pack wrap of [Pubky](https://github.com/pubky/pubky-core) SDK, published on
[npm as `@synonymdev/pubky`](https://www.npmjs.com/package/@synonymdev/pubky).

Works in modern browsers and Node v20+.

For deeper dives, check out the
[examples/javascript](../../../examples/javascript) scripts and the
[npm package documentation](pkg/README.md).

## How To Build/Test the NPM Package

Make sure rust and wasm-pack are available.

```bash
curl https://sh.rustup.rs -sSf | sh
curl https://drager.github.io/wasm-pack/installer/init.sh -sSf | sh
```

1. Go to `pubky-sdk/bindings/js/pkg`.
2. Run `npm run build`.
3. Run a testnet mainline DHT, Pkarr relay and Homeserver `npm run testnet`
4. Run tests with `npm run test`.
