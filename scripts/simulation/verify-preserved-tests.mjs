import { lstat, readFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { classifyTracked, issue, itemAt, protectedRustHunks, rustItems, sha256, verifyClassA, verifyInventoryMarkdown, verifyInventorySchema, verifyInventoryShape, verifySealedEvidence } from "./preserved-tests/core.mjs";

const root = new URL("../..", import.meta.url).pathname;
const inventoryPath = new URL("../../packages/engine/tests/preserved-test-inventory.json", import.meta.url);
const inventoryMarkdownPath = new URL("../../packages/engine/tests/preserved-test-inventory.md", import.meta.url);
const indexPath = new URL("../../.omo/evidence/escrow-parallel-engine/baseline-corpus/manifest.json", import.meta.url);
const sealedPath = new URL("../../.omo/evidence/escrow-parallel-engine/baseline-corpus/attempt-12/b-test-inventory.json", import.meta.url);
const manifestPath = new URL("../../.omo/evidence/escrow-parallel-engine/baseline-corpus/attempt-12/manifest.json", import.meta.url);
const sealPath = new URL("../../.omo/evidence/escrow-parallel-engine/baseline-corpus/attempt-12/seal.json", import.meta.url);
const overlayPath = new URL("../../.omo/evidence/escrow-parallel-engine/baseline-corpus/attempt-12/overlay.json", import.meta.url);
const closurePath = new URL("../../.omo/evidence/escrow-parallel-engine/baseline-corpus/attempt-12/closure.json", import.meta.url);
const git = (args) => { const output = spawnSync("git", args, { cwd: root, encoding: "utf8" }); if (output.status !== 0) throw new Error(output.stderr.trim()); return output.stdout; };
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
const FROZEN_INVENTORY_SHA256 = "f07d9a9928f8430c8dc395613db2952435ae5d66b80cc46dbe4df0aaba1d784f";
const FROZEN_INVENTORY_MARKDOWN_SHA256 = "3e918626be13ccd2709e5af5d5c86b163a85a258493c169d6383aeb7dea6100c";
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
const baselineFiles = new Map();
const currentFiles = new Map();
const approvedWholeFilePaths = new Set(inventory.approved_divergence_changes.filter((entry) => entry.current_sha256).map((entry) => entry.path));
for (const path of changedFiles) {
  try { baselineFiles.set(path, git(["show", `${baseline}:${path}`])); } catch {
    if (!approvedWholeFilePaths.has(path) && !inventory.class_c.additive.some((entry) => entry.path === path)) issues.push(issue("EXPANDED", path, null, "tracked deletion/replacement"));
  }
  currentFiles.set(path, await readRegular(path));
}
const approvedBaselineFiles = new Map();
for (const entry of inventory.approved_divergence_changes) {
  if (!entry.baseline_revision) continue;
  try {
    const source = git(["show", `${entry.baseline_revision}:${entry.path}`]);
    approvedBaselineFiles.set(entry.path, source);
  } catch (error) {
    issues.push(issue("RECLASSIFIED", entry.path, entry.divergence_id, `approved baseline source unavailable: ${error.message}`));
  }
}
for (const entry of inventory.foundation_overlays) {
  try {
    git(["rev-parse", "--verify", `${entry.foundation_revision}^{commit}`]);
    git(["merge-base", "--is-ancestor", entry.foundation_revision, "HEAD"]);
    const foundationSource = git(["show", `${entry.foundation_revision}:${entry.path}`]);
    if (!foundationSource) issues.push(issue("RECLASSIFIED", entry.path, null, "foundation revision source unavailable"));
  } catch (error) {
    issues.push(issue("RECLASSIFIED", entry.path, null, `foundation revision provenance: ${error.message}`));
  }
}
const inlineProtectedSymbols = new Map();
for (const entry of inventory.class_b) {
  if (!entry.path.startsWith("packages/engine/src/")) continue;
  const symbols = inlineProtectedSymbols.get(entry.path) ?? new Set();
  symbols.add(entry.symbol);
  for (const change of inventory.class_b_changes.filter((change) => change.file === entry.path && change.symbol === entry.symbol)) symbols.add(change.current_symbol);
  inlineProtectedSymbols.set(entry.path, symbols);
}
const hunks = protectedRustHunks(diff, protectedPaths).filter((hunk) => {
  const symbols = inlineProtectedSymbols.get(hunk.path);
  if (!symbols) return true;
  const oldItem = baselineFiles.has(hunk.path) && itemAt(rustItems(baselineFiles.get(hunk.path)), hunk.oldLine);
  const newItem = currentFiles.has(hunk.path) && itemAt(rustItems(currentFiles.get(hunk.path)), hunk.newLine);
  return symbols.has(oldItem?.symbol) || symbols.has(newItem?.symbol);
});
const tracked = classifyTracked({ hunks, baselineFiles, currentFiles, exactC: inventory.class_c.exact, additiveC: inventory.class_c.additive, approvedDivergences: inventory.approved_divergence_changes, foundationOverlays: inventory.foundation_overlays, approvedBaselineFiles, classB: sealed.b_test_inventory, classBChanges: inventory.class_b_changes, forbiddenTokens: inventory.forbidden_tokens });
issues.push(...tracked.issues);
for (const entry of inventory.class_a) {
  const source = await readRegular(entry.path);
  let baselineSource;
  try { baselineSource = git(["show", `${baseline}:${entry.path}`]); } catch { issues.push(issue("MISSING", entry.path, entry.symbol, "sealed A baseline file")); continue; }
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
else console.log(JSON.stringify({ status: "PASS", baseline, ancestry: true, overlay_test_entries: 0, class_a_entries: inventory.class_a.length, class_b_entries: inventory.class_b.length, class_b_mapped_assertions: sealed.b_test_inventory.reduce((sum, entry) => sum + entry.assertions.length, 0), active_b_changes: inventory.class_b_changes.length, class_c_entries: inventory.class_c.exact.length, tracked_files_total: changedFiles.length, tracked_hunks_total: hunks.length, tracked_hunks_classified: tracked.mechanical + tracked.exact, generic_mechanical_symbol_count: tracked.mechanicalSymbols, generic_mechanical_hunk_count: tracked.mechanical, exact_c_hunk_count: tracked.exact, untracked_count: untrackedPaths.filter((path) => !path.endsWith("preserved-test-inventory.md") && !path.endsWith("preserved-test-inventory.json")).length, untracked_classified: untrackedClassified }));
