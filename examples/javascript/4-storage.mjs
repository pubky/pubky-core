#!/usr/bin/env node
// Read a public resource (no auth).
import { Pubky } from "@synonymdev/pubky";
import { args } from "./_cli.mjs";

const usage = `
Usage:
  npm run storage -- <pubky>/<absolute-path> [--testnet]

Example:
  npm run storage -- q5oo7majwe3mbkj6p49osws8o748b186bbojdxdn3asnn63enk6y/pub/my.app/hello.txt --testnet
  npm run storage -- operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/posts/0033X02JAN0SG
`;

const a = args(process.argv.slice(2), { usage });
const [resource] = a._;
if (!resource) {
  console.error(usage.trim());
  process.exit(1);
}

const pubky = a.testnet ? Pubky.testnet() : new Pubky();

// PublicStorage reads from addressed "<pk>/<abs-path>"
const exists = await pubky.publicStorage.exists(resource);
console.log(`Exists: ${exists ? "yes" : "no"}`);

const stats = await pubky.publicStorage.stats(resource);
console.log(`File stats: ${stats ? JSON.stringify(stats, null, 2) : "(none)"}`);

// Get bytes and render length (purely for demo)
const bytes = await pubky.publicStorage.getText(resource);
console.log(`Downloaded (binary) ${bytes.length} bytes`);
