const makeFetchCookie = require("fetch-cookie").default;
const tough = require("tough-cookie");

// Create cookie jar explicitly so we can access it in tests
const jar = new tough.CookieJar();
let originalFetch = globalThis.fetch;
globalThis.fetch = makeFetchCookie(originalFetch, jar);
globalThis.__cookieJar = jar;
