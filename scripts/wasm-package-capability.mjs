import { createHash } from "node:crypto";
import { lstat, readFile, readdir, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";

export const WASM_PACKAGE_METADATA_FILENAMES = Object.freeze([
  "wasm-build-provenance.json",
  "verified-restore-capability.json",
]);
export const WASM_RESTORE_REQUIRED_EXPORTS = Object.freeze([
  "restore_json",
  "facade_slot_init_for_restore_json",
  "set_step_runtime_override",
  "init_wasm_wait",
  "wasm_wait_abort_layout",
  "shutdown_session",
]);
const TOOLCHAIN = "nightly-2026-09-05";
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
const jsonBytes = (value) => Buffer.from(`${JSON.stringify(value, null, 2)}\n`, "utf8");
const exactKeys = (value, keys) => value !== null && typeof value === "object" && !Array.isArray(value)
  && Object.keys(value).length === keys.length && keys.every((key) => Object.hasOwn(value, key));

function fail(message) { throw new Error(`WASM package metadata invalid: ${message}`); }

export function isDefaultFeaturesRelease(command) {
  return command.args.includes("--release") && !command.args.some((arg) =>
    arg === "--features" || arg.startsWith("--features=") || arg.startsWith("-F")
    || arg === "--no-default-features" || arg === "--all-features");
}

export function publicGlueFunctions(ts, source) {
  const tree = ts.createSourceFile("web_wasm.js", source, ts.ScriptTarget.Latest, true, ts.ScriptKind.JS);
  if (tree.parseDiagnostics.length !== 0) fail("generated glue AST parsing failed");
  const functions = new Set();
  const exported = new Set();
  const hasExport = (node) => node.modifiers?.some((modifier) => modifier.kind === ts.SyntaxKind.ExportKeyword) === true;
  for (const node of tree.statements) {
    if (ts.isFunctionDeclaration(node) && node.name !== undefined) {
      functions.add(node.name.text);
      if (hasExport(node)) exported.add(node.name.text);
    } else if (ts.isVariableStatement(node)) {
      for (const declaration of node.declarationList.declarations) {
        if (ts.isIdentifier(declaration.name) && declaration.initializer !== undefined
            && (ts.isArrowFunction(declaration.initializer) || ts.isFunctionExpression(declaration.initializer))) {
          functions.add(declaration.name.text);
          if (hasExport(node)) exported.add(declaration.name.text);
        }
      }
    }
  }
  for (const node of tree.statements) {
    if (!ts.isExportDeclaration(node)) continue;
    if (node.moduleSpecifier !== undefined || node.exportClause === undefined || !ts.isNamedExports(node.exportClause)) {
      fail("generated glue contains unsupported re-export; refusing to infer public functions");
    }
    for (const entry of node.exportClause.elements) {
      const local = entry.propertyName === undefined ? entry.name.text : entry.propertyName.text;
      if (functions.has(local)) exported.add(entry.name.text);
    }
  }
  return [...exported].sort();
}

async function rustSourcePaths(root) {
  const paths = new Set([
    "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", ".cargo/config.toml",
    "apps/web-wasm/Cargo.toml", "apps/web-wasm/.cargo/config.toml", "apps/web-wasm/build.rs", "packages/engine/Cargo.toml", "packages/engine/build.rs",
  ]);
  for (const optional of ["apps/web-wasm/build.rs", "packages/engine/build.rs"]) {
    try { await lstat(path.join(root, optional)); } catch (error) { if (error.code === "ENOENT") paths.delete(optional); else throw error; }
  }
  for (const directory of ["apps/web-wasm/src", "packages/engine/src"]) {
    const visit = async (relative) => {
      const entries = await readdir(path.join(root, relative), { withFileTypes: true });
      entries.sort((a, b) => a.name < b.name ? -1 : a.name > b.name ? 1 : 0);
      for (const entry of entries) {
        const child = path.posix.join(relative, entry.name);
        if (entry.isSymbolicLink()) fail(`Rust source closure contains a symbolic link: ${child}`);
        if (entry.isDirectory()) await visit(child);
        else if (entry.isFile() && (entry.name.endsWith(".rs") || entry.name === "Cargo.toml" || entry.name === "build.rs")) paths.add(child);
      }
    };
    await visit(directory);
  }
  return [...paths].sort();
}

export async function captureWasmBuildSourceIdentity(root) {
  const rows = [];
  for (const relative of await rustSourcePaths(root)) {
    const filename = path.join(root, relative);
    const metadata = await lstat(filename);
    if (!metadata.isFile() || metadata.isSymbolicLink()) fail(`Rust source input is not a regular file: ${relative}`);
    rows.push({ path: relative, sha256: sha256(await readFile(filename)) });
  }
  return Object.freeze(rows.map((row) => Object.freeze(row)));
}

export function wasmBuildSourceSha256(rows) {
  return sha256(jsonBytes(rows));
}

async function loadTypeScript() {
  const filename = path.resolve(import.meta.dirname, "../apps/web/node_modules/typescript/lib/typescript.js");
  const imported = await import(pathToFileURL(filename));
  return imported.default === undefined ? imported : imported.default;
}

export async function invalidateWasmPackageMetadata(outDir) {
  for (const name of WASM_PACKAGE_METADATA_FILENAMES) {
    const filename = path.join(outDir, name);
    let metadata;
    try { metadata = await lstat(filename); } catch (error) { if (error.code === "ENOENT") continue; throw error; }
    if (!metadata.isFile() || metadata.isSymbolicLink() || metadata.nlink !== 1) fail(`refusing to invalidate aliased metadata file: ${filename}`);
    await rm(filename);
  }
}

export async function produceWasmPackageMetadata({ root, outDir, command, sourceSnapshot }) {
  if (!path.isAbsolute(root) || !path.isAbsolute(outDir)) fail("root and outDir must be absolute paths");
  if (!Array.isArray(sourceSnapshot) || sourceSnapshot.length === 0) fail("missing pre-build Rust source snapshot");
  if (command?.command !== "wasm-pack" || command.env?.RUSTUP_TOOLCHAIN !== TOOLCHAIN) fail(`actual wasm-pack command must pin RUSTUP_TOOLCHAIN=${TOOLCHAIN}`);
  if (!isDefaultFeaturesRelease(command)) {
    fail("metadata is produced only for release builds with default features");
  }
  const currentSnapshot = await captureWasmBuildSourceIdentity(root);
  if (JSON.stringify(currentSnapshot) !== JSON.stringify(sourceSnapshot)) fail("Rust source closure changed during wasm-pack build");
  const wasm = await readFile(path.join(outDir, "web_wasm_bg.wasm"));
  const glue = await readFile(path.join(outDir, "web_wasm.js"));
  if (wasm.length === 0 || glue.length === 0) fail("actual package WASM and glue must be non-empty regular files");
  const rawExports = WebAssembly.Module.exports(new WebAssembly.Module(wasm))
    .filter((entry) => entry.kind === "function").map((entry) => entry.name);
  const glueExports = publicGlueFunctions(await loadTypeScript(), glue.toString("utf8"));
  const glueSet = new Set(glueExports);
  const exports = rawExports.filter((name) => glueSet.has(name)).sort();
  for (const required of WASM_RESTORE_REQUIRED_EXPORTS) if (!exports.includes(required)) fail(`actual WASM and glue lack required public function ${required}`);
  const sourceSha256 = wasmBuildSourceSha256(sourceSnapshot);
  const provenance = {
    kind: "wasm-build-provenance",
    sourceSha256,
    wasmSha256: sha256(wasm),
    glueSha256: sha256(glue),
    profile: "release",
    toolchain: command.env.RUSTUP_TOOLCHAIN,
  };
  const provenanceBytes = jsonBytes(provenance);
  const capability = {
    kind: "verified-package",
    wasmSha256: provenance.wasmSha256,
    glueSha256: provenance.glueSha256,
    sourceSha256,
    buildManifestSha256: sha256(provenanceBytes),
    exports,
  };
  const capabilityBytes = jsonBytes(capability);
  const provenancePath = path.join(outDir, WASM_PACKAGE_METADATA_FILENAMES[0]);
  const capabilityPath = path.join(outDir, WASM_PACKAGE_METADATA_FILENAMES[1]);
  await writeFile(provenancePath, provenanceBytes, { flag: "wx" });
  try {
    await writeFile(capabilityPath, capabilityBytes, { flag: "wx" });
  } catch (error) {
    const { rm } = await import("node:fs/promises");
    await rm(provenancePath);
    throw error;
  }
  return { provenance, capability, provenanceBytes, capabilityBytes, provenancePath, capabilityPath };
}

export function parseWasmPackageMetadata(provenanceBytes, capabilityBytes) {
  const provenanceRaw = Buffer.from(provenanceBytes);
  const capabilityRaw = Buffer.from(capabilityBytes);
  let provenance;
  let capability;
  try {
    provenance = JSON.parse(provenanceRaw.toString("utf8"));
    capability = JSON.parse(capabilityRaw.toString("utf8"));
  } catch (error) { fail(`metadata JSON parsing failed: ${error.message}`); }
  if (!exactKeys(provenance, ["kind", "sourceSha256", "wasmSha256", "glueSha256", "profile", "toolchain"])
      || provenance.kind !== "wasm-build-provenance" || provenance.profile !== "release" || provenance.toolchain !== TOOLCHAIN) fail("provenance schema or build identity mismatch");
  if (!exactKeys(capability, ["kind", "wasmSha256", "glueSha256", "sourceSha256", "buildManifestSha256", "exports"])
      || capability.kind !== "verified-package" || !Array.isArray(capability.exports)) fail("capability schema mismatch");
  const hex = /^[a-f0-9]{64}$/;
  for (const value of [provenance.sourceSha256, provenance.wasmSha256, provenance.glueSha256,
    capability.sourceSha256, capability.wasmSha256, capability.glueSha256, capability.buildManifestSha256]) {
    if (typeof value !== "string" || !hex.test(value)) fail("identity hash is malformed");
  }
  if (capability.sourceSha256 !== provenance.sourceSha256 || capability.wasmSha256 !== provenance.wasmSha256
      || capability.glueSha256 !== provenance.glueSha256 || capability.buildManifestSha256 !== sha256(provenanceRaw)) fail("metadata identities do not match the provenance bytes");
  if (capability.exports.some((name) => typeof name !== "string")
      || new Set(capability.exports).size !== capability.exports.length
      || capability.exports.some((name, index) => index > 0 && capability.exports[index - 1] > name)
      || WASM_RESTORE_REQUIRED_EXPORTS.some((name) => !capability.exports.includes(name))) fail("capability exports are malformed or missing required public functions");
  return { provenance, capability, provenanceBytes: provenanceRaw, capabilityBytes: capabilityRaw };
}

export async function readWasmPackageMetadata(packageDirectory) {
  const [provenanceBytes, capabilityBytes] = await Promise.all(WASM_PACKAGE_METADATA_FILENAMES.map((name) => readFile(path.join(packageDirectory, name))));
  return parseWasmPackageMetadata(provenanceBytes, capabilityBytes);
}
