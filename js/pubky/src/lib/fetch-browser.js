/* eslint-disable no-prototype-builtins */
const g =
  (typeof globalThis !== 'undefined' && globalThis) ||
  // eslint-disable-next-line no-undef
  (typeof self !== 'undefined' && self) ||
  // eslint-disable-next-line no-undef
  (typeof global !== 'undefined' && global) ||
  {}

// @ts-ignore
export default g.fetch
