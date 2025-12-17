// Prevent tape-run's source map loader from crashing on synthetic wasm:// URLs
// produced when mapping WASM stack traces. These URLs cannot be fetched via
// XHR, so we neutralize those requests in browser test runs.
if (typeof XMLHttpRequest !== "undefined") {
  const origOpen = XMLHttpRequest.prototype.open;
  const origSend = XMLHttpRequest.prototype.send;

  XMLHttpRequest.prototype.open = function (
    method: string,
    url: string | URL,
    async?: boolean,
    username?: string | null,
    password?: string | null
  ) {
    // @ts-expect-error private marker for wasm-source-map mitigation
    this.__wasmUrl = typeof url === "string" && url.startsWith("wasm://");
    return origOpen.apply(this, arguments as unknown as Parameters<
      XMLHttpRequest["open"]
    >);
  };

  XMLHttpRequest.prototype.send = function (
    ...args: Parameters<XMLHttpRequest["send"]>
  ) {
    // @ts-expect-error private marker for wasm-source-map mitigation
    if (this.__wasmUrl === true) {
      // Skip requests for wasm:// URLs to avoid synchronous send() exceptions.
      return undefined as void;
    }

    return origSend.apply(this, args);
  };
}
