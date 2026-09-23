import assert from "node:assert/strict";
import { execFile as callback } from "node:child_process";
import { createHash } from "node:crypto";
import { chmod, copyFile, readdir, readFile, writeFile } from "node:fs/promises";
import { promisify } from "node:util";

const execFile = promisify(callback);
export async function fileHash(path) {
  return createHash("sha256").update(await readFile(path)).digest("hex");
}

export async function assertArtifactHashes(hashes) {
  for (const [path, expected] of Object.entries(hashes)) {
    assert.equal(await fileHash(path), expected, `artifact changed: ${path}`);
  }
}

export async function assertPublicationFreshness(provenance, output) {
  for (const path of ["apps/web/wasm-pkg/web_wasm_bg.wasm", "apps/web/wasm-pkg/web_wasm.js", `${output}/tauri-default`, `${output}/tauri-feature`, `${output}/server`, `${output}/ws-parity-probe`]) {
    assert.equal(typeof provenance.hashes[path], "string", `missing required artifact: ${path}`);
  }
  await assertArtifactHashes(provenance.hashes);
  assert.deepEqual(await sourceInputs(), provenance.inputs, "declared build inputs changed during execution");
}

export async function sourceInputs() {
  const paths = [];
  const visit = async (path) => {
    for (const entry of await readdir(path, { withFileTypes: true })) {
      if (["node_modules", "target", "dist", "pkg", "wasm-pkg", ".git"].includes(entry.name)) continue;
      const child = `${path}/${entry.name}`;
      if (entry.isDirectory()) await visit(child);
      else if (entry.isFile()) paths.push(child);
      else throw new Error(`unsupported build input: ${child}`);
    }
  };
  for (const root of ["packages", "apps", ".cargo"]) await visit(root);
  const scripts = await readdir("scripts/simulation");
  paths.push(...scripts.filter((path) => path.startsWith("host-parity") && path.endsWith(".mjs")).map((path) => `scripts/simulation/${path}`));
  paths.push("Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "package.json", "pnpm-lock.yaml", "pnpm-workspace.yaml", "scripts/wasm-build.sh", "scripts/check-wasm-threading.mjs");
  return Object.fromEntries(await Promise.all(paths.sort().map(async (path) => [path, await fileHash(path)])));
}

export async function runBuild(output, command, args) {
  const result = await execFile(command, args, { maxBuffer: 32 * 1024 * 1024 });
  await writeFile(output, `${result.stdout}${result.stderr}`);
}

export async function buildParityInputs(output) {
  assert.equal(process.version, "v24.18.0", "pinned Node is required");
  const { stdout: corepack } = await execFile("corepack", ["--version"]);
  assert.equal(corepack.trim(), "0.35.0", "pinned Corepack is required");
  const inputs = await sourceInputs();
  const { stdout: revision } = await execFile("git", ["rev-parse", "HEAD"]);
  const { stdout: dirtyInputs } = await execFile("git", ["status", "--porcelain", "--untracked-files=all", "--", ...Object.keys(inputs)], { maxBuffer: 8 * 1024 * 1024 });
  await runBuild(`${output}/wasm-build.log`, "bash", ["scripts/wasm-build.sh"]);
  await runBuild(`${output}/server-build.log`, "cargo", ["build", "--locked", "-p", "server", "--features", "host-parity", "--bins"]);
  const binaries = {};
  for (const [label, features] of [["default", "host-parity"], ["feature", "host-parity,simulation-diagnostics"]]) {
    await runBuild(`${output}/tauri.${label}.host-parity-build.log`, "cargo", ["build", "--locked", "-p", "stock-market-game", "--features", features]);
    const path = `${output}/tauri-${label}`;
    await copyFile("target/debug/stock-market-game", path);
    await chmod(path, 0o500);
    binaries[label] = path;
  }
  for (const name of ["server", "ws-parity-probe"]) {
    await copyFile(`target/debug/${name}`, `${output}/${name}`);
    await chmod(`${output}/${name}`, 0o500);
  }
  assert.deepEqual(await sourceInputs(), inputs, "declared build inputs changed during build");
  const artifacts = ["apps/web/wasm-pkg/web_wasm_bg.wasm", "apps/web/wasm-pkg/web_wasm.js", binaries.default, binaries.feature, `${output}/server`, `${output}/ws-parity-probe`];
  const hashes = Object.fromEntries(await Promise.all(artifacts.map(async (path) => [path, await fileHash(path)])));
  const tools = {};
  for (const [name, command, args] of [["rustc", "rustc", ["-Vv"]], ["cargo", "cargo", ["-V"]], ["wasmNightly", "rustup", ["run", "nightly-2026-09-05", "rustc", "-Vv"]], ["wasmPack", "wasm-pack", ["--version"]]]) {
    tools[name] = (await execFile(command, args)).stdout.trim();
  }
  const provenance = { revision: revision.trim(), dirtyInputs: dirtyInputs.trim().split("\n").filter(Boolean), inputs, node: process.version, corepack: corepack.trim(), tools, binaries, hashes };
  await writeFile(`${output}/build-provenance.json`, `${JSON.stringify(provenance, null, 2)}\n`);
  return provenance;
}
