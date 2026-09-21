#!/usr/bin/env node
import { readFile, readdir } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

/** Engine references are explicit `engine::symbol` tokens or engine-symbol comments. */
export function referencedEngineSymbols(text) {
  return [...new Set([
    ...[...text.matchAll(/`engine::([A-Za-z_][A-Za-z0-9_:]*)(?:\([^`]*\))?`/g)].map((match) => match[1].split("::").at(-1)),
    ...[...text.matchAll(/<!--\s*engine-symbol:\s*([A-Za-z_][A-Za-z0-9_]*)\s*-->/g)].map((match) => match[1]),
  ])].sort();
}

export async function checkDocSymbols(repoRoot, documents) {
  const sources = [];
  async function walk(directory) {
    for (const entry of await readdir(directory, { withFileTypes: true })) {
      const child = path.join(directory, entry.name);
      if (entry.isDirectory()) await walk(child);
      else if (entry.isFile() && entry.name.endsWith(".rs")) sources.push(await readFile(child, "utf8"));
    }
  }
  await walk(path.join(repoRoot, "packages/engine/src"));
  // Declarations/variants and impl members are deliberately checked lexically;
  // this is a missing-symbol guard, not proof of Rust public path resolution.
  const source = sources.join("\n");
  const checked = [];
  const missing = [];
  for (const document of documents) {
    const absolute = path.resolve(repoRoot, document);
    const relative = path.relative(repoRoot, absolute);
    if (relative === ".." || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative)) throw new Error(`document escapes repository: ${document}`);
    for (const symbol of referencedEngineSymbols(await readFile(absolute, "utf8"))) {
      const row = { document, symbol };
      checked.push(row);
      if (!new RegExp(`\\b${symbol}\\b`).test(source)) missing.push(row);
    }
  }
  return { schema: "engine-doc-symbol-check-v1", status: missing.length === 0 && checked.length > 0 ? "PASS" : "FAIL",
    scope: "explicit engine::symbol references; lexical existence, not type checking", checked, missing };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    if (process.argv.length < 3) throw new Error("usage: node scripts/check-doc-symbols.mjs <docs...>");
    const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
    const result = await checkDocSymbols(root, process.argv.slice(2));
    process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
    process.exitCode = result.status === "PASS" ? 0 : 1;
  } catch (error) { console.error(`doc symbol check failed: ${error.message}`); process.exitCode = 1; }
}
