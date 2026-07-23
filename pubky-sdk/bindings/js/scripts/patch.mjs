// This script is used to generate isomorphic code for web and nodejs
//
// Based on hacks from [this issue](https://github.com/rustwasm/wasm-pack/issues/1334)

import { readFile, writeFile, rename } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import path, { dirname } from "node:path";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

const cargoTomlContent = await readFile(
  path.join(__dirname, "../Cargo.toml"),
  "utf8",
);
const pkgName = /\[package\]\nname = "(.*?)"/.exec(cargoTomlContent)[1];
const base = pkgName.replace(/-wasm$/, "");
const name = base.replace(/-/g, "_");

const content = await readFile(
  path.join(__dirname, `../pkg/nodejs/${name}.js`),
  "utf8",
);

const inlineSnippetPattern =
  /const \{[^}]*\} = require\(String\.raw`\.\/snippets\/([^`]+\/inline\d+\.js)`\);\n?/g;
const inlineSnippetModulePattern =
  /const (import\d+) = require\("\.\/snippets\/([^"]+\/inline\d+\.js)"\);\n?/g;
const inlineSnippets = new Map();
async function loadInlineSnippet(snippetPath) {
  if (inlineSnippets.has(snippetPath)) {
    return inlineSnippets.get(snippetPath);
  }

  let snippet = await readFile(
    path.join(__dirname, "../pkg/nodejs/snippets", snippetPath),
    "utf8",
  );
  snippet = snippet.replace(/export async function/g, "async function");
  snippet = snippet.replace(/export function/g, "function");
  inlineSnippets.set(snippetPath, snippet);
  return snippet;
}

const inlineSnippetReplacements = new Map();
for (const match of content.matchAll(inlineSnippetPattern)) {
  inlineSnippetReplacements.set(match[0], await loadInlineSnippet(match[1]));
}

const inlineSnippetModuleReplacements = new Map();
const inlineSnippetModuleObjects = new Map();
for (const match of content.matchAll(inlineSnippetModulePattern)) {
  const snippetPath = match[2];
  const rawSnippet = await readFile(
    path.join(__dirname, "../pkg/nodejs/snippets", snippetPath),
    "utf8",
  );
  const names = [...rawSnippet.matchAll(/export (?:async )?function (\w+)/g)].map(
    (m) => m[1],
  );
  const snippet = await loadInlineSnippet(snippetPath);
  const object = `const ${match[1]} = {
${names.map((name) => `  ${name},`).join("\n")}
};\n`;
  inlineSnippetModuleObjects.set(match[0], object);
  inlineSnippetModuleReplacements.set(
    match[0],
    `${snippet}
${object}`,
  );
}

const needsNamedExport = new Set();

const hasModuleExports = content.includes("= module.exports");

let patched = content
  .replace(inlineSnippetPattern, (match, snippetPath) => {
    const replacement = inlineSnippetReplacements.get(match) ?? match;
    if (inlineSnippets.get(`emitted:esm:${snippetPath}`)) {
      return "";
    }
    inlineSnippets.set(`emitted:esm:${snippetPath}`, true);
    return replacement;
  })
  .replace(inlineSnippetModulePattern, (match, _importName, snippetPath) => {
    const replacement = inlineSnippetModuleReplacements.get(match) ?? match;
    if (inlineSnippets.get(`emitted:esm:${snippetPath}`)) {
      return inlineSnippetModuleObjects.get(match) ?? "";
    }
    inlineSnippets.set(`emitted:esm:${snippetPath}`, true);
    return replacement;
  })
  // use global TextDecoder TextEncoder
  .replace("require(`util`)", "globalThis")
  // attach to `imports` instead of module.exports
  .replace("= module.exports", "= imports")
  // Export classes
  .replace(/\nclass (.*?) \{/g, "\n export class $1 {")
  // Export functions
  .replace(
    /\n(?:module\.exports|exports)\.(\w+)\s*=\s*function/g,
    (_match, fn) => {
      needsNamedExport.delete(fn);
      return `\nimports.${fn} = ${fn};\nexport function ${fn}`;
    },
  )
  // Add exports to 'imports'
  .replace(
    /\n(?:module\.exports|exports)\.(\w+)\s*=\s*([^;\n]+)(;?)/g,
    (_match, name, value, suffix) => {
      const trimmed = value.trim();
      if (trimmed === name) {
        needsNamedExport.add(name);
      }
      return `\nimports.${name} = ${trimmed}${suffix}`;
    },
  )
  .replace(/= exports\./g, "= imports.");

if (!hasModuleExports) {
  patched = "const imports = {};\n" + patched;
}

for (const name of needsNamedExport) {
  if (
    name !== "default" &&
    !new RegExp(`export (?:class|function|const|let|var) ${name}\\b`).test(patched)
  ) {
    patched += `\nexport { ${name} };`;
  }
}

patched += "\nexport default imports";
patched = patched
  // inline wasm bytes
  .replace(
    /\nconst (?:path.*\nconst bytes.*|wasmPath.*\nconst wasmBytes.*)\nconst wasmModule.*\n/,
    `
var __toBinary = /* @__PURE__ */ (() => {
  var table = new Uint8Array(128);
  for (var i = 0; i < 64; i++)
    table[i < 26 ? i + 65 : i < 52 ? i + 71 : i < 62 ? i - 4 : i * 4 - 205] = i;
  return (base64) => {
    var n = base64.length, bytes = new Uint8Array((n - (base64[n - 1] == "=") - (base64[n - 2] == "=")) * 3 / 4 | 0);
    for (var i2 = 0, j = 0; i2 < n; ) {
      var c0 = table[base64.charCodeAt(i2++)], c1 = table[base64.charCodeAt(i2++)];
      var c2 = table[base64.charCodeAt(i2++)], c3 = table[base64.charCodeAt(i2++)];
      bytes[j++] = c0 << 2 | c1 >> 4;
      bytes[j++] = c1 << 4 | c2 >> 2;
      bytes[j++] = c2 << 6 | c3;
    }
    return bytes;
  };
})();

const bytes = __toBinary(${JSON.stringify(
      await readFile(
        path.join(__dirname, `../pkg/nodejs/${name}_bg.wasm`),
        "base64",
      ),
    )});
const wasmModule = new WebAssembly.Module(bytes);
`,
  );

await writeFile(
  path.join(__dirname, `../pkg/index.js`),
  patched + "\nglobalThis['pubky'] = imports",
);

// Move outside of nodejs

await Promise.all(
  [".js", ".d.ts", "_bg.wasm"].map((suffix) =>
    rename(
      path.join(__dirname, `../pkg/nodejs/${name}${suffix}`),
      path.join(
        __dirname,
        `../pkg/${suffix === ".js" ? "index.cjs" : name + suffix}`,
      ),
    ),
  ),
);

// Add index.cjs headers

const indexcjsPath = path.join(__dirname, `../pkg/index.cjs`);

const headerContent = await readFile(
  path.join(__dirname, `../pkg/node-header.cjs`),
  "utf8",
);
let indexcjsContent = await readFile(indexcjsPath, "utf8");
indexcjsContent = indexcjsContent
  .replace(inlineSnippetPattern, (match, snippetPath) => {
    const replacement = inlineSnippetReplacements.get(match) ?? match;
    if (inlineSnippets.get(`emitted:cjs:${snippetPath}`)) {
      return "";
    }
    inlineSnippets.set(`emitted:cjs:${snippetPath}`, true);
    return replacement;
  })
  .replace(inlineSnippetModulePattern, (match, _importName, snippetPath) => {
    const replacement = inlineSnippetModuleReplacements.get(match) ?? match;
    if (inlineSnippets.get(`emitted:cjs:${snippetPath}`)) {
      return inlineSnippetModuleObjects.get(match) ?? "";
    }
    inlineSnippets.set(`emitted:cjs:${snippetPath}`, true);
    return replacement;
  });

await writeFile(indexcjsPath, headerContent + "\n" + indexcjsContent, "utf8");
