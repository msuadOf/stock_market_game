import { createHash, randomUUID } from "node:crypto";
import { linkSync, lstatSync, readFileSync, readdirSync, unlinkSync, writeFileSync } from "node:fs";
import { basename, join, resolve } from "node:path";

const manifestName = "task-3-wayland.integrity.json";
const reviewName = "task-3-wayland.review.md";
const lockName = "task-3-wayland.integrity.lock";
const hashBlockStart = "<!-- task-3-wayland-integrity:start -->";
const hashBlockEnd = "<!-- task-3-wayland-integrity:end -->";

function errorCode(error) {
  if (typeof error !== "object" || error === null || !("code" in error)) return null;
  return error.code;
}

function missingPath(path) {
  try {
    lstatSync(path);
    return false;
  } catch (error) {
    if (errorCode(error) === "ENOENT") return true;
    throw error;
  }
}

function requireRunDirectory(runDirectory) {
  const metadata = lstatSync(runDirectory);
  if (!metadata.isDirectory() || metadata.isSymbolicLink()) {
    throw new Error(`run directory is not a real directory: ${runDirectory}`);
  }
}

function isSafeRelativePath(path) {
  return path.length > 0
    && !path.startsWith("/")
    && !path.includes("\\")
    && !path.split("/").includes("..");
}

function regularFiles(runDirectory, excludedNames) {
  const files = [];
  const walk = (relativePath) => {
    const absolutePath = relativePath.length === 0 ? runDirectory : join(runDirectory, relativePath);
    const metadata = lstatSync(absolutePath);
    if (metadata.isSymbolicLink()) throw new Error(`symlink is not permitted: ${relativePath || "."}`);
    if (metadata.isDirectory()) {
      for (const entry of readdirSync(absolutePath, { withFileTypes: true }).sort((left, right) => left.name.localeCompare(right.name))) {
        const childPath = relativePath.length === 0 ? entry.name : `${relativePath}/${entry.name}`;
        walk(childPath);
      }
      return;
    }
    if (!metadata.isFile()) throw new Error(`unsupported evidence entry: ${relativePath}`);
    if (!excludedNames.has(relativePath)) files.push(relativePath);
  };
  walk("");
  return files.sort((left, right) => left.localeCompare(right));
}

function hashEntry(runDirectory, relativePath) {
  const absolutePath = join(runDirectory, relativePath);
  const before = lstatSync(absolutePath, { bigint: true });
  if (!before.isFile() || before.isSymbolicLink()) throw new Error(`artifact is not a regular file: ${relativePath}`);
  const bytes = readFileSync(absolutePath);
  const after = lstatSync(absolutePath, { bigint: true });
  if (before.size !== after.size || before.mtimeNs !== after.mtimeNs) {
    throw new Error(`artifact changed while hashing: ${relativePath}`);
  }
  return {
    path: relativePath,
    size: Number(before.size),
    sha256: createHash("sha256").update(bytes).digest("hex"),
  };
}

function renderReview(runId, entries) {
  const lines = [
    "# Task 3 Wayland Evidence Integrity Review",
    "",
    `Run ID: \`${runId}\``,
    "",
    "This review is generated from the exact completed run directory. The review and machine manifest are excluded from the evidence hash set to avoid a self-referential hash.",
    "",
    "## SHA-256 Evidence Set",
    "",
    hashBlockStart,
    ...entries.map((entry) => `${entry.sha256}  ${entry.path}`),
    hashBlockEnd,
    "",
    "Verify with:",
    "",
    `\`node scripts/desktop/wayland-evidence-integrity.mjs verify .omo/evidence/resolve-blockers-wayland/${runId}\``,
    "",
  ];
  return lines.join("\n");
}

function writeExclusive(path, content) {
  writeFileSync(path, content, { encoding: "utf8", flag: "wx", mode: 0o600 });
}

function publishExclusive(temporaryPath, finalPath) {
  try {
    linkSync(temporaryPath, finalPath);
  } catch (error) {
    if (errorCode(error) === "EEXIST") throw new Error(`refusing to overwrite existing integrity artifact: ${finalPath}`);
    throw error;
  } finally {
    if (!missingPath(temporaryPath)) unlinkSync(temporaryPath);
  }
}

function generate(runDirectory) {
  requireRunDirectory(runDirectory);
  const manifestPath = join(runDirectory, manifestName);
  const reviewPath = join(runDirectory, reviewName);
  const lockPath = join(runDirectory, lockName);
  if (!missingPath(manifestPath) || !missingPath(reviewPath) || !missingPath(lockPath)) {
    throw new Error(`refusing to overwrite existing integrity artifacts in ${runDirectory}`);
  }
  writeExclusive(lockPath, `${process.pid}\n`);
  const temporaryPaths = [];
  try {
    const entries = regularFiles(runDirectory, new Set([manifestName, reviewName, lockName]))
      .map((relativePath) => hashEntry(runDirectory, relativePath));
    const manifest = {
      format: "task-3-wayland-integrity/v1",
      algorithm: "sha256",
      run_id: basename(runDirectory),
      files: entries,
    };
    const manifestTemporaryPath = join(runDirectory, `.${manifestName}.${process.pid}.${randomUUID()}.tmp`);
    const reviewTemporaryPath = join(runDirectory, `.${reviewName}.${process.pid}.${randomUUID()}.tmp`);
    temporaryPaths.push(manifestTemporaryPath, reviewTemporaryPath);
    writeExclusive(manifestTemporaryPath, `${JSON.stringify(manifest, null, 2)}\n`);
    writeExclusive(reviewTemporaryPath, renderReview(manifest.run_id, entries));
    publishExclusive(manifestTemporaryPath, manifestPath);
    publishExclusive(reviewTemporaryPath, reviewPath);
  } finally {
    for (const temporaryPath of temporaryPaths) {
      if (!missingPath(temporaryPath)) unlinkSync(temporaryPath);
    }
    if (!missingPath(lockPath)) unlinkSync(lockPath);
  }
}

function requireManifestEntry(value) {
  if (typeof value !== "object" || value === null || !("path" in value) || !("size" in value) || !("sha256" in value)) {
    throw new Error("manifest contains an invalid file entry");
  }
  const { path, size, sha256 } = value;
  if (typeof path !== "string" || !isSafeRelativePath(path)) throw new Error(`manifest contains unsafe path: ${String(path)}`);
  if (!Number.isSafeInteger(size) || size < 0) throw new Error(`manifest contains invalid size for ${path}`);
  if (typeof sha256 !== "string" || !/^[a-f0-9]{64}$/.test(sha256)) throw new Error(`manifest contains invalid SHA-256 for ${path}`);
  return { path, size, sha256 };
}

function reviewEntries(review) {
  const lines = review.split("\n");
  const start = lines.indexOf(hashBlockStart);
  const end = lines.indexOf(hashBlockEnd);
  if (start < 0 || end < 0 || end <= start) throw new Error("review hash list does not match current artifacts");
  return lines.slice(start + 1, end).map((line) => {
    const match = /^([a-f0-9]{64})  (.+)$/.exec(line);
    if (!match) throw new Error("review hash list does not match current artifacts");
    return { sha256: match[1], path: match[2] };
  });
}

function sameEntries(left, right) {
  return left.length === right.length && left.every((entry, index) => entry.path === right[index].path && entry.sha256 === right[index].sha256);
}

function verify(runDirectory) {
  requireRunDirectory(runDirectory);
  const manifestPath = join(runDirectory, manifestName);
  const reviewPath = join(runDirectory, reviewName);
  if (missingPath(manifestPath) || missingPath(reviewPath)) throw new Error(`missing integrity review artifacts in ${runDirectory}`);
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  if (typeof manifest !== "object" || manifest === null || manifest.format !== "task-3-wayland-integrity/v1" || manifest.algorithm !== "sha256") {
    throw new Error("unsupported integrity manifest");
  }
  if (manifest.run_id !== basename(runDirectory)) throw new Error(`manifest run ID does not match directory: ${runDirectory}`);
  if (!Array.isArray(manifest.files)) throw new Error("manifest files must be an array");
  const entries = manifest.files.map(requireManifestEntry);
  const expectedPaths = entries.map((entry) => entry.path);
  if (!sameEntries(entries, [...entries].sort((left, right) => left.path.localeCompare(right.path)))) throw new Error("manifest files are not sorted");
  if (new Set(expectedPaths).size !== expectedPaths.length) throw new Error("manifest contains duplicate paths");
  if (expectedPaths.includes(manifestName) || expectedPaths.includes(reviewName)) throw new Error("manifest cannot hash integrity metadata");
  const actualPaths = regularFiles(runDirectory, new Set([manifestName, reviewName]));
  if (JSON.stringify(actualPaths) !== JSON.stringify(expectedPaths)) throw new Error("manifest file list does not match current artifacts");
  const review = readFileSync(reviewPath, "utf8");
  if (!review.includes(`Run ID: \`${manifest.run_id}\``) || !sameEntries(entries, reviewEntries(review))) {
    throw new Error("review hash list does not match current artifacts");
  }
  for (const entry of entries) {
    const actual = hashEntry(runDirectory, entry.path);
    if (actual.size !== entry.size || actual.sha256 !== entry.sha256) {
      throw new Error(`integrity mismatch for ${entry.path}`);
    }
  }
}

const [mode, requestedRunDirectory] = process.argv.slice(2);
if ((mode !== "generate" && mode !== "verify") || requestedRunDirectory === undefined) {
  throw new Error("usage: node scripts/desktop/wayland-evidence-integrity.mjs <generate|verify> <run-directory>");
}
const runDirectory = resolve(requestedRunDirectory);
if (mode === "generate") generate(runDirectory);
else verify(runDirectory);
