import { createHash } from "node:crypto";
import { lstat, readdir, readFile, realpath } from "node:fs/promises";
import path from "node:path";

// This is deliberately an allowlist instead of a repository walk: a source
// checkout can live below a workspace which also contains writable worktrees,
// caches and evidence. Every executable/documented delivery input is included,
// while those mutable siblings are impossible to pick up by accident.
const INPUTS = [
  ".cargo", ".omo/plans/escrow-parallel-engine.md", ".rustfmt.toml", "AGENTS.md",
  "CLAUDE.md", "CONTRIBUTING.md", "Cargo.lock", "Cargo.toml", "DESIGN.md", "README.md",
  "rust-toolchain.toml", "rustfmt.toml", "package.json", "pnpm-lock.yaml", "pnpm-workspace.yaml",
  "apps/desktop/src-tauri", "apps/server", "apps/web-wasm",
  "apps/web/.gitignore", "apps/web/.oxlintrc.json", "apps/web/README.md", "apps/web/e2e",
  "apps/web/index.html", "apps/web/inspector.html", "apps/web/package.json", "apps/web/playwright.config.ts",
  "apps/web/public", "apps/web/src", "apps/web/tsconfig.app.json", "apps/web/tsconfig.inspector.json",
  "apps/web/tsconfig.json", "apps/web/tsconfig.node.json", "apps/web/vite.config.ts",
  "docs", "packages/engine", "packages/engine-gpu", "scripts",
];
const digest = (bytes) => createHash("sha256").update(bytes).digest("hex");

/** Full content input, including untracked module sources; excludes all caches. */
export async function escrowSourceManifest(sourceRoot) {
  if (!path.isAbsolute(sourceRoot) || await realpath(sourceRoot) !== sourceRoot) {
    throw new Error("source root must be absolute, canonical and non-symlinked");
  }
  const files = [];
  const absent = [];
  async function visit(relative, optional = false) {
    const absolute = path.join(sourceRoot, relative);
    let stat;
    try { stat = await lstat(absolute); }
    catch (error) { if (optional && error.code === "ENOENT") { absent.push(relative); return; } throw error; }
    if (stat.isSymbolicLink()) throw new Error(`source manifest refuses symlink: ${relative}`);
    if (stat.isDirectory()) {
      for (const child of (await readdir(absolute)).sort()) await visit(path.posix.join(relative, child));
    } else if (stat.isFile()) {
      const bytes = await readFile(absolute);
      files.push({ path: relative, byte_length: String(bytes.length), sha256: digest(bytes) });
    } else throw new Error(`source manifest refuses special file: ${relative}`);
  }
  for (const input of INPUTS) await visit(input, true);
  files.sort((a, b) => a.path < b.path ? -1 : a.path > b.path ? 1 : 0);
  const content = { schema: "escrow-source-manifest-v1", inputs: INPUTS, files, absent: absent.sort() };
  return { ...content, sha256: digest(JSON.stringify(content)) };
}
