const makeFetchCookie = require("fetch-cookie").default;

let originalFetch = globalThis.fetch;
globalThis.fetch = makeFetchCookie(originalFetch);
