#!/usr/bin/env node
// Read a public resource (no auth). Keep it minimal and Pubky-focused.
import { Pubky } from "@synonymdev/pubky";
import { args } from "./_cli.mjs";

const usage = `
Usage:
  npm run storage -- <pubky>/<absolute-path> [--testnet]

Example:
  npm run storage -- operrr8.../pub/pubky.app/posts/0033X02JAN0SG --testnet
`;

const a = args(process.argv.slice(2), { usage });
const [resource] = a._;
if (!resource) {
  console.error(usage.trim());
  process.exit(1);
}

const pubky = a.testnet ? Pubky.testnet() : new Pubky();

// PublicStorage reads from addressed "<pk>/<abs-path>"
const pub = pubky.publicStorage();

// Auto-choose a reader based on extension (purely for demo)
if (resource.endsWith(".json")) {
  console.log(await pub.getJson(resource));
} else if (resource.endsWith(".txt")) {
  console.log(await pub.getText(resource));
} else {
  const bytes = await pub.getBytes(resource);
  console.log(`(binary) ${bytes.length} bytes`);
}
