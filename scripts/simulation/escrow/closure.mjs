import { createHash } from "node:crypto";
import { lstat, readFile, mkdir, writeFile, chmod, readdir } from "node:fs/promises";
import { dirname, join, relative, resolve } from "node:path";
import { git, run } from "./process.mjs";

export const HEAD = "7041d35dc362ca74f4f3313e6804db9499f0679a";
export const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
export const isHarness = (path) => path === "packages/engine/examples/escrow_baseline_corpus.rs" || path.startsWith("packages/engine/examples/escrow_baseline/");

export function validateEntries(entries) {
  const paths = new Set();
  for (const entry of entries) {
    const path = entry.path;
    if (typeof path !== "string" || !/^[A-Za-z0-9_.\-/]+$/.test(path) || path.startsWith("/") || path.split("/").some((part) => !part || part === "." || part === ".." || part.toLowerCase() === ".git")) throw new Error(`unsafe path: ${path}`);
    if (paths.has(path.toLowerCase())) throw new Error(`duplicate/case collision: ${path}`);
    paths.add(path.toLowerCase());
    if (entry.entry_type !== "file" || !["100644", "100755"].includes(entry.mode)) throw new Error(`unsupported entry type/mode: ${path}`);
    if (!Number.isSafeInteger(entry.byte_length) || entry.byte_length < 0 || !/^[0-9a-f]{64}$/.test(entry.sha256)) throw new Error(`invalid length/hash: ${path}`);
  }
}

async function safeFile(root, path) {
  let current = root;
  for (const part of path.split("/")) {
    current = join(current, part);
    const info = await lstat(current);
    if (info.isSymbolicLink()) throw new Error(`symlink rejected: ${path}`);
  }
  const info = await lstat(current);
  if (!info.isFile()) throw new Error(`not regular file: ${path}`);
  if (![0o644, 0o664, 0o755, 0o775].includes(info.mode & 0o7777)) throw new Error(`unsupported file mode: ${path}`);
  return info;
}

export async function captureFiles(root, paths, headEntries) {
  const entries = [];
  const blobs = {};
  for (const path of [...paths].sort()) {
    validateEntries([{ path, entry_type: "file", mode: "100644", byte_length: 0, sha256: sha256("") }]);
    const info = await safeFile(root, path);
    const bytes = await readFile(join(root, path));
    const hash = sha256(bytes);
    const head = headEntries.get(path);
    const mode = info.mode & 0o111 ? "100755" : "100644";
    const gitBlob = createHash("sha1").update(`blob ${bytes.length}\0`).update(bytes).digest("hex");
    entries.push({ path, entry_type: "file", mode, byte_length: bytes.length, sha256: hash,
      origin: head ? (head.blob === gitBlob && head.mode === mode ? "head" : "tracked_modified") : "untracked",
      head_blob: head ? head.blob : null,
      inclusion_reason: path.startsWith("packages/engine/") ? "engine source/tests/examples closure" : "Cargo workspace manifest, configuration or transitive path input",
    });
    blobs[hash] = bytes.toString("base64");
  }
  validateEntries(entries);
  return { entries, blobs };
}

export async function applyArchive(root, archive) {
  validateEntries(archive.entries);
  for (const entry of archive.entries) {
    const encoded = archive.blobs[entry.sha256];
    if (typeof encoded !== "string") throw new Error(`missing content: ${entry.path}`);
    const bytes = Buffer.from(encoded, "base64");
    if (bytes.length !== entry.byte_length || sha256(bytes) !== entry.sha256 || bytes.toString("base64") !== encoded) throw new Error(`content hash mismatch: ${entry.path}`);
  }
  for (const entry of archive.entries) {
    let current = root;
    for (const part of entry.path.split("/").slice(0, -1)) {
      current = join(current, part);
      await mkdir(current, { recursive: true });
      if (!(await lstat(current)).isDirectory()) throw new Error(`unsafe parent: ${entry.path}`);
    }
    const destination = join(root, entry.path);
    try { if (!(await lstat(destination)).isFile()) throw new Error(`unsafe destination: ${entry.path}`); }
    catch (error) { if (error.code !== "ENOENT") throw error; }
    await writeFile(destination, Buffer.from(archive.blobs[entry.sha256], "base64"));
    await chmod(destination, entry.mode === "100755" ? 0o755 : 0o644);
  }
}

export async function walk(root, prefix) {
  const paths = [];
  for (const entry of await readdir(join(root, prefix), { withFileTypes: true })) {
    const path = `${prefix}/${entry.name}`;
    if (entry.isDirectory()) paths.push(...await walk(root, path));
    else paths.push(path);
  }
  return paths;
}

export async function captureClosure(root) {
  const commit = (await git(root, ["rev-parse", "HEAD"])).trim();
  if (commit !== HEAD) throw new Error(`baseline HEAD mismatch: ${commit}`);
  const tree = (await git(root, ["rev-parse", `${HEAD}^{tree}`])).trim();
  const metadata = JSON.parse((await run("cargo", ["metadata", "--no-deps", "--format-version", "1", "--offline"], { cwd: root })).stdout);
  const engine = metadata.packages.find((pkg) => pkg.name === "engine");
  if (!engine) throw new Error("engine package absent");
  const roots = new Set(["packages/engine"]);
  const visit = (pkg) => {
    for (const dependency of pkg.dependencies.filter((dep) => dep.path)) {
      const path = relative(resolve(root), resolve(dependency.path));
      if (path.startsWith("..") || path.startsWith("/")) throw new Error("external path dependency rejected");
      if (!roots.has(path)) {
        roots.add(path);
        const target = metadata.packages.find((item) => dirname(item.manifest_path) === dependency.path);
        if (!target) throw new Error(`unresolved path dependency: ${path}`);
        visit(target);
      }
    }
  };
  visit(engine);
  const manifests = metadata.packages.map((pkg) => relative(resolve(root), resolve(pkg.manifest_path)));
  const inputs = ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml", ...manifests];
  const paths = new Set(inputs);
  for (const prefix of [...roots, ".cargo"]) for (const path of await walk(root, prefix)) if (!isHarness(path)) paths.add(path);
  const treeRecords = (await git(root, ["ls-tree", "-rz", HEAD])).split("\0").filter(Boolean);
  const head = new Map(treeRecords.map((record) => {
    const [meta, path] = record.split("\t");
    const [mode, type, blob] = meta.split(" ");
    if (paths.has(path) && (type !== "blob" || !["100644", "100755"].includes(mode))) throw new Error(`unsupported HEAD entry: ${path}`);
    return [path, { mode, blob }];
  }));
  for (const path of head.keys()) {
    if (!isHarness(path) && [...roots, ".cargo"].some((prefix) => path.startsWith(`${prefix}/`)) && !paths.has(path)) {
      throw new Error(`tracked closure deletion unsupported: ${path}`);
    }
  }
  const archive = await captureFiles(root, [...paths], head);
  const overlayEntries = archive.entries.filter((entry) => entry.origin !== "head");
  const overlay = { entries: overlayEntries, blobs: Object.fromEntries(overlayEntries.map((entry) => [entry.sha256, archive.blobs[entry.sha256]])) };
  return { commit, tree, roots: [...roots].sort(), manifests: manifests.sort(), closure: archive.entries, overlay,
    closure_digest: sha256(JSON.stringify(archive.entries)), overlay_digest: sha256(JSON.stringify(overlay)),
    cargo: (await run("cargo", ["--version"], { cwd: root })).stdout.trim(),
    rustc: (await run("rustc", ["-vV"], { cwd: root })).stdout.trim() };
}

export async function verifyClosure(root, baseline) {
  const paths = new Set(baseline.closure.map((entry) => entry.path));
  for (const prefix of [...baseline.roots, ".cargo"]) for (const path of await walk(root, prefix)) {
    if (!isHarness(path) && !paths.has(path)) throw new Error(`undeclared closure path: ${path}`);
  }
  const actual = await captureFiles(root, [...paths], new Map());
  for (let index = 0; index < actual.entries.length; index++) {
    const observed = actual.entries[index];
    const expected = baseline.closure[index];
    if (observed.path !== expected.path || observed.sha256 !== expected.sha256 || observed.mode !== expected.mode) throw new Error(`closure content/mode mismatch: ${observed.path}`);
  }
}
