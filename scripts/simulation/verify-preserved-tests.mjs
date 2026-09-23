import { lstat, readFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { assertions, classifyTracked, issue, itemAt, normalizeResultBody, parseCatFileBatch, parseHunks, protectedRustHunks, rustItems, sha256, verifyClassA, verifyInventoryMarkdown, verifyInventorySchema, verifyInventoryShape, verifyPerformanceFixtureReduction, verifySealedEvidence } from "./preserved-tests/core.mjs";

const root = new URL("../..", import.meta.url).pathname;
const inventoryPath = new URL("../../packages/engine/tests/preserved-test-inventory.json", import.meta.url);
const inventoryMarkdownPath = new URL("../../packages/engine/tests/preserved-test-inventory.md", import.meta.url);
const indexPath = new URL("../../.omo/evidence/escrow-parallel-engine/baseline-corpus/manifest.json", import.meta.url);
const sealedPath = new URL("../../.omo/evidence/escrow-parallel-engine/baseline-corpus/attempt-12/b-test-inventory.json", import.meta.url);
const manifestPath = new URL("../../.omo/evidence/escrow-parallel-engine/baseline-corpus/attempt-12/manifest.json", import.meta.url);
const sealPath = new URL("../../.omo/evidence/escrow-parallel-engine/baseline-corpus/attempt-12/seal.json", import.meta.url);
const overlayPath = new URL("../../.omo/evidence/escrow-parallel-engine/baseline-corpus/attempt-12/overlay.json", import.meta.url);
const closurePath = new URL("../../.omo/evidence/escrow-parallel-engine/baseline-corpus/attempt-12/closure.json", import.meta.url);
const gitCache = new Map();
const git = (args) => {
  const key = JSON.stringify(args);
  if (gitCache.has(key)) return gitCache.get(key);
  const output = spawnSync("git", args, { cwd: root, encoding: "utf8" });
  if (output.status !== 0) throw new Error(output.stderr.trim());
  gitCache.set(key, output.stdout);
  return output.stdout;
};
const readGitObjects = (specs) => {
  const unique = [...new Set(specs)];
  if (unique.length === 0) return new Map();
  const output = spawnSync("git", ["cat-file", "--batch"], {
    cwd: root,
    input: `${unique.join("\n")}\n`,
    maxBuffer: 64 * 1024 * 1024,
  });
  if (output.status !== 0) throw new Error(output.stderr.toString("utf8").trim());
  return parseCatFileBatch(unique, output.stdout);
};
const readRegular = async (relative) => {
  const source = new URL(`../../${relative}`, import.meta.url);
  const stat = await lstat(source);
  if (stat.isSymbolicLink() || !stat.isFile()) throw new Error(`preserved test input is not a regular file: ${relative}`);
  return readFile(source, "utf8");
};
for (const required of [indexPath, sealedPath, manifestPath, sealPath, overlayPath, closurePath]) {
  if (!existsSync(required)) {
    const path = new URL(required).pathname;
    console.log(JSON.stringify({ status: "BLOCKED", task9_acceptance: false, code: "MISSING_SEALED_EVIDENCE", path,
      detail: "sealed attempt-12 evidence is required; no legacy witness is fabricated" }));
    process.exit(2);
  }
}
const inventoryText = await readFile(inventoryPath, "utf8");
const inventoryMarkdown = await readFile(inventoryMarkdownPath, "utf8");
const inventory = JSON.parse(inventoryText);
const sealedBytes = await readFile(sealedPath);
const sealed = JSON.parse(sealedBytes);
const indexBytes = await readFile(indexPath);
const index = JSON.parse(indexBytes);
const manifestBytes = await readFile(manifestPath);
const manifest = JSON.parse(manifestBytes);
const sealBytes = await readFile(sealPath);
const seal = JSON.parse(sealBytes);
const overlayBytes = await readFile(overlayPath);
const overlay = JSON.parse(overlayBytes);
const closureBytes = await readFile(closurePath);
const closure = JSON.parse(closureBytes);
const issues = [];
// B5's policy metadata is frozen independently of the legacy corpus. Any
// policy change must update this verifier revision and receive its own review;
// it is never classified as a Rust hunk.
const FROZEN_INVENTORY_SHA256 = "b4755c32762dcf03da07995b3c096fe13b93165f064d860f020e75efacf9fb90";
const FROZEN_INVENTORY_MARKDOWN_SHA256 = "c047cf3d3596db0c560c8a1d1676e632b6105844239f33fdba558559043236a2";
if (sha256(inventoryText) !== FROZEN_INVENTORY_SHA256) issues.push(issue("RECLASSIFIED", "packages/engine/tests/preserved-test-inventory.json", null, "frozen metadata SHA-256"));
if (sha256(inventoryMarkdown) !== FROZEN_INVENTORY_MARKDOWN_SHA256) issues.push(issue("RECLASSIFIED", "packages/engine/tests/preserved-test-inventory.md", null, "frozen metadata SHA-256"));
issues.push(...verifyInventorySchema(inventory));
issues.push(...verifyInventoryMarkdown(inventoryMarkdown, inventory));
issues.push(...verifySealedEvidence({ index, indexBytes, manifest, manifestBytes, seal, sealBytes, closure, closureBytes, overlay, overlayBytes, sealed, sealedBytes }));
const baseline = manifest.preserved_test_baseline_sha;
if (manifest.status !== "sealed" || inventory.baseline !== baseline) issues.push(issue("RECLASSIFIED", "preserved-test-inventory.json", null, "sealed baseline identity"));
try { git(["rev-parse", "--verify", `${baseline}^{commit}`]); git(["merge-base", "--is-ancestor", baseline, "HEAD"]); } catch (error) { issues.push(issue("MISSING", baseline, null, `baseline/ancestry ${error.message}`)); }
try {
  const baselineTree = git(["rev-parse", `${baseline}^{tree}`]).trim();
  if (baselineTree !== manifest.head_tree || baselineTree !== closure.tree) issues.push(issue("RECLASSIFIED", "manifest.json", null, "baseline source tree fingerprint"));
} catch (error) { issues.push(issue("MISSING", baseline, null, `baseline tree fingerprint ${error.message}`)); }
if (Array.isArray(overlay?.entries) && overlay.entries.some((entry) => entry?.path?.startsWith("packages/engine/tests/"))) issues.push(issue("EXPANDED", "overlay.json", null, "test overlay entry"));
if (Array.isArray(sealed?.b_test_inventory)) issues.push(...verifyInventoryShape(inventory, sealed));
const metadataPaths = new Set(["packages/engine/tests/preserved-test-inventory.json", "packages/engine/tests/preserved-test-inventory.md"]);
const protectedPaths = [...new Set([...inventory.class_a.map((entry) => entry.path), ...inventory.class_b.map((entry) => entry.path), ...inventory.class_b_changes.map((entry) => entry.file), ...inventory.approved_divergence_changes.map((entry) => entry.path)])];
const diffRoots = ["packages/engine/tests", ...protectedPaths.filter((file) => file.startsWith("packages/engine/src/"))];
const allChangedFiles = git(["diff", "--name-only", baseline, "--", ...diffRoots]).trim().split("\n").filter(Boolean);
for (const path of allChangedFiles) if (!metadataPaths.has(path) && !path.endsWith(".rs")) issues.push(issue("ADDED", path, null, "non-Rust test artifact"));
const changedFiles = allChangedFiles.filter((path) => path.endsWith(".rs"));
const diff = git(["diff", "--unified=0", baseline, "--", ...diffRoots]);
const gitObjects = readGitObjects([
  ...changedFiles.map((path) => `${baseline}:${path}`),
  ...inventory.approved_divergence_changes.filter((entry) => entry.baseline_revision).map((entry) => `${entry.baseline_revision}:${entry.path}`),
  ...inventory.foundation_overlays.map((entry) => `${entry.foundation_revision}:${entry.path}`),
  ...inventory.performance_fixture_reductions.map((entry) => `${entry.baseline_revision}:${entry.path}`),
  ...inventory.class_a.map((entry) => `${baseline}:${entry.path}`),
]);
const gitObjectText = (spec) => gitObjects.get(spec)?.toString("utf8") ?? null;
const baselineFiles = new Map();
const currentFiles = new Map();
const approvedWholeFilePaths = new Set(inventory.approved_divergence_changes.filter((entry) => entry.current_sha256).map((entry) => entry.path));
for (const path of changedFiles) {
  const baselineSource = gitObjectText(`${baseline}:${path}`);
  if (baselineSource !== null) baselineFiles.set(path, baselineSource);
  else {
    if (!approvedWholeFilePaths.has(path) && !inventory.class_c.additive.some((entry) => entry.path === path)) issues.push(issue("EXPANDED", path, null, "tracked deletion/replacement"));
  }
  currentFiles.set(path, await readRegular(path));
}
const approvedBaselineFiles = new Map();
for (const entry of inventory.approved_divergence_changes) {
  if (!entry.baseline_revision) continue;
  const source = gitObjectText(`${entry.baseline_revision}:${entry.path}`);
  if (source === null) issues.push(issue("RECLASSIFIED", entry.path, entry.divergence_id, "approved baseline source unavailable"));
  else approvedBaselineFiles.set(entry.path, source);
}
const foundationRevisionErrors = new Map();
for (const revision of new Set(inventory.foundation_overlays.map((entry) => entry.foundation_revision))) {
  try {
    git(["rev-parse", "--verify", `${revision}^{commit}`]);
    git(["merge-base", "--is-ancestor", revision, "HEAD"]);
  } catch (error) {
    foundationRevisionErrors.set(revision, error.message);
  }
}
for (const entry of inventory.foundation_overlays) {
  const revisionError = foundationRevisionErrors.get(entry.foundation_revision);
  if (revisionError) issues.push(issue("RECLASSIFIED", entry.path, null, `foundation revision provenance: ${revisionError}`));
  else if (gitObjectText(`${entry.foundation_revision}:${entry.path}`) === null) issues.push(issue("RECLASSIFIED", entry.path, null, "foundation revision source unavailable"));
}
const performanceFixtureRevisionErrors = new Map();
for (const revision of new Set(inventory.performance_fixture_reductions.map((entry) => entry.baseline_revision))) {
  try {
    git(["rev-parse", "--verify", `${revision}^{commit}`]);
    git(["merge-base", "--is-ancestor", revision, "HEAD"]);
  } catch (error) {
    performanceFixtureRevisionErrors.set(revision, error.message);
  }
}
const verifiedPerformanceFixturePaths = new Set();
for (const entry of inventory.performance_fixture_reductions) {
  const revisionError = performanceFixtureRevisionErrors.get(entry.baseline_revision);
  if (revisionError) {
    issues.push(issue("RECLASSIFIED", entry.path, entry.symbol, `performance fixture revision provenance: ${revisionError}`));
    continue;
  }
  const baselineSource = gitObjectText(`${entry.baseline_revision}:${entry.path}`);
  const currentSource = currentFiles.get(entry.path);
  const coverageSource = currentFiles.get(entry.coverage.path) ?? await readRegular(entry.coverage.path);
  if (baselineSource === null || currentSource === undefined) {
    issues.push(issue("MISSING", entry.path, entry.symbol, "performance fixture source unavailable"));
    continue;
  }
  const directHunks = parseHunks(git(["diff", "--unified=0", entry.baseline_revision, "--", entry.path]));
  const result = verifyPerformanceFixtureReduction({ entry, baselineSource, currentSource, hunks: directHunks, coverageSource });
  if (result) {
    issues.push(result);
    continue;
  }
  if (entry.long_validation !== undefined) {
    const runner = await readRegular(entry.long_validation.runner_path);
    const symbol = entry.long_validation.filter.split("::").at(-1);
    const item = rustItems(currentSource).find((candidate) => candidate.symbol === symbol);
    const assertionHashes = item === undefined ? [] : assertions(currentSource, item).map((assertion) => assertion.current_hash);
    const ignoredPattern = new RegExp(`#\\[ignore[^\\]]*\\][\\s\\S]{0,160}fn\\s+${symbol}\\s*\\(`);
    if (sha256(runner) !== entry.long_validation.runner_sha256
      || item === undefined
      || sha256(normalizeResultBody(item.body)) !== entry.long_validation.item_sha256
      || JSON.stringify(assertionHashes) !== JSON.stringify(entry.long_validation.assertion_hashes)
      || !ignoredPattern.test(currentSource)
      || !runner.includes(entry.long_validation.filter)
      || !runner.includes('"--ignored"')) {
      issues.push(issue("RECLASSIFIED", entry.path, symbol, "required long validation source/runner binding"));
      continue;
    }
  }
  verifiedPerformanceFixturePaths.add(entry.path);
}
const inlineProtectedSymbols = new Map();
for (const entry of inventory.class_b) {
  if (!entry.path.startsWith("packages/engine/src/")) continue;
  const symbols = inlineProtectedSymbols.get(entry.path) ?? new Set();
  symbols.add(entry.symbol);
  for (const change of inventory.class_b_changes.filter((change) => change.file === entry.path && change.symbol === entry.symbol)) symbols.add(change.current_symbol);
  inlineProtectedSymbols.set(entry.path, symbols);
}
const baselineItems = new Map([...baselineFiles].map(([path, source]) => [path, rustItems(source)]));
const currentItems = new Map([...currentFiles].map(([path, source]) => [path, rustItems(source)]));
const hunks = protectedRustHunks(diff, protectedPaths).filter((hunk) => {
  const symbols = inlineProtectedSymbols.get(hunk.path);
  if (!symbols) return true;
  const oldItem = baselineItems.has(hunk.path) && itemAt(baselineItems.get(hunk.path), hunk.oldLine);
  const newItem = currentItems.has(hunk.path) && itemAt(currentItems.get(hunk.path), hunk.newLine);
  return symbols.has(oldItem?.symbol) || symbols.has(newItem?.symbol);
});
const tracked = classifyTracked({ hunks, baselineFiles, currentFiles, exactC: inventory.class_c.exact, additiveC: inventory.class_c.additive, approvedDivergences: inventory.approved_divergence_changes, foundationOverlays: inventory.foundation_overlays, approvedBaselineFiles, classB: sealed.b_test_inventory, classBChanges: inventory.class_b_changes, verifiedPerformanceFixturePaths, forbiddenTokens: inventory.forbidden_tokens });
issues.push(...tracked.issues);
for (const entry of inventory.class_a) {
  const source = await readRegular(entry.path);
  const baselineSource = gitObjectText(`${baseline}:${entry.path}`);
  if (baselineSource === null) { issues.push(issue("MISSING", entry.path, entry.symbol, "sealed A baseline file")); continue; }
  const result = verifyClassA({ entry, baselineSource, currentSource: source });
  if (result) issues.push(result);
}
const untrackedPaths = git(["ls-files", "--others", "--exclude-standard", "packages/engine/tests"]).trim().split("\n").filter(Boolean);
let untrackedClassified = 0;
for (const path of untrackedPaths) {
  if (path.endsWith("preserved-test-inventory.md") || path.endsWith("preserved-test-inventory.json")) continue;
  const entry = inventory.class_c.additive.find((item) => item.path === path);
  if (!entry) issues.push(issue("ADDED", path, null, "untracked test"));
  else if (sha256(await readRegular(path)) !== entry.sha256) issues.push(issue("RECLASSIFIED", path, null, "untracked hash"));
  else untrackedClassified += 1;
}
for (const item of issues.sort((left, right) => JSON.stringify(left).localeCompare(JSON.stringify(right)))) console.log(JSON.stringify(item));
if (issues.length) process.exitCode = 1;
else console.log(JSON.stringify({ status: "PASS", baseline, ancestry: true, overlay_test_entries: 0, class_a_entries: inventory.class_a.length, class_b_entries: inventory.class_b.length, class_b_mapped_assertions: sealed.b_test_inventory.reduce((sum, entry) => sum + entry.assertions.length, 0), active_b_changes: inventory.class_b_changes.length, performance_fixture_reductions: inventory.performance_fixture_reductions.length, class_c_entries: inventory.class_c.exact.length, tracked_files_total: changedFiles.length, tracked_hunks_total: hunks.length, tracked_hunks_classified: tracked.mechanical + tracked.exact, generic_mechanical_symbol_count: tracked.mechanicalSymbols, generic_mechanical_hunk_count: tracked.mechanical, exact_c_hunk_count: tracked.exact, untracked_count: untrackedPaths.filter((path) => !path.endsWith("preserved-test-inventory.md") && !path.endsWith("preserved-test-inventory.json")).length, untracked_classified: untrackedClassified }));
