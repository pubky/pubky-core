#!/usr/bin/env node
// Raw request using the Pubky HTTP client (supports pubky:// and https://).
import { Pubky } from "@synonymdev/pubky";
import { args, printHttpResponse } from "./_cli.mjs";

const usage = `
Usage:
  npm run request -- <METHOD> <URL> [--testnet] [-H "Name: value"]... [-d DATA]

Examples:
  npm run request -- GET pubky://<user>/pub/my.app/info.json --testnet
  npm run request -- \\
    -H "Content-Type: application/json" \\
    -H "Accept: application/json" \\
    -d '{"msg":"hello"}' \\
    POST https://example.com/data.json
`;

const a = args(process.argv.slice(2), {
  usage,
  aliases: { H: "header", d: "data" },
  defaults: { header: [] },
});
const [method, url] = a._;
if (!method || !url) {
  console.error(usage.trim());
  process.exit(1);
}

const pubky = a.testnet ? Pubky.testnet() : new Pubky();
const client = pubky.client();

const headers = {};
for (const h of Array.isArray(a.header)
  ? a.header
  : [a.header].filter(Boolean)) {
  const idx = h.indexOf(":");
  if (idx === -1) continue;
  headers[h.slice(0, idx).trim()] = h.slice(idx + 1).trim();
}

const res = await client.fetch(url, {
  method,
  headers,
  body: a.data ?? undefined,
  credentials: "include",
});

const body = res.headers.get("content-type")?.includes("application/json")
  ? JSON.stringify(await res.json(), null, 2)
  : await res.text();

printHttpResponse(res, body);
