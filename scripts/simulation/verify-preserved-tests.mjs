import { readFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { classifyTracked, issue, rustTestHunks, sha256, verifyClassA, verifyInventoryMarkdown, verifyInventorySchema, verifyInventoryShape } from "./preserved-tests/core.mjs";

const root = new URL("../..", import.meta.url).pathname;
const inventoryPath = new URL("../../packages/engine/tests/preserved-test-inventory.json", import.meta.url);
const inventoryMarkdownPath = new URL("../../packages/engine/tests/preserved-test-inventory.md", import.meta.url);
const sealedPath = new URL("../../.omo/evidence/escrow-parallel-engine/baseline-corpus/attempt-12/b-test-inventory.json", import.meta.url);
const manifestPath = new URL("../../.omo/evidence/escrow-parallel-engine/baseline-corpus/attempt-12/manifest.json", import.meta.url);
const overlayPath = new URL("../../.omo/evidence/escrow-parallel-engine/baseline-corpus/attempt-12/overlay.json", import.meta.url);
const git = (args) => { const output = spawnSync("git", args, { cwd: root, encoding: "utf8" }); if (output.status !== 0) throw new Error(output.stderr.trim()); return output.stdout; };
for (const required of [sealedPath, manifestPath, overlayPath]) {
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
const sealed = JSON.parse(await readFile(sealedPath));
const manifest = JSON.parse(await readFile(manifestPath));
const overlay = JSON.parse(await readFile(overlayPath));
const issues = [];
// B5's policy metadata is frozen independently of the legacy corpus. Any
// policy change must update this verifier revision and receive its own review;
// it is never classified as a Rust hunk.
const FROZEN_INVENTORY_SHA256 = "aea92a0e6f74eb736980cb5f222b7ce58c95aa5f40b289ceee7540aca98b5792";
const FROZEN_INVENTORY_MARKDOWN_SHA256 = "956fe03a1f79d7d089b4ed9e3f26f1ab22775c1c15e6f1bb85d583935b4af6f3";
if (sha256(inventoryText) !== FROZEN_INVENTORY_SHA256) issues.push(issue("RECLASSIFIED", "packages/engine/tests/preserved-test-inventory.json", null, "frozen metadata SHA-256"));
if (sha256(inventoryMarkdown) !== FROZEN_INVENTORY_MARKDOWN_SHA256) issues.push(issue("RECLASSIFIED", "packages/engine/tests/preserved-test-inventory.md", null, "frozen metadata SHA-256"));
issues.push(...verifyInventorySchema(inventory));
issues.push(...verifyInventoryMarkdown(inventoryMarkdown, inventory));
const baseline = manifest.preserved_test_baseline_sha;
if (manifest.status !== "sealed" || inventory.baseline !== baseline) issues.push(issue("RECLASSIFIED", "preserved-test-inventory.json", null, "sealed baseline identity"));
try { git(["rev-parse", "--verify", `${baseline}^{commit}`]); git(["merge-base", "--is-ancestor", baseline, "HEAD"]); } catch (error) { issues.push(issue("MISSING", baseline, null, `baseline/ancestry ${error.message}`)); }
if (overlay.entries.some((entry) => entry.path.startsWith("packages/engine/tests/"))) issues.push(issue("EXPANDED", "overlay.json", null, "test overlay entry"));
issues.push(...verifyInventoryShape(inventory, sealed));
const metadataPaths = new Set(["packages/engine/tests/preserved-test-inventory.json", "packages/engine/tests/preserved-test-inventory.md"]);
const allChangedFiles = git(["diff", "--name-only", baseline, "--", "packages/engine/tests"]).trim().split("\n").filter(Boolean);
for (const path of allChangedFiles) if (!metadataPaths.has(path) && !path.endsWith(".rs")) issues.push(issue("ADDED", path, null, "non-Rust test artifact"));
const changedFiles = allChangedFiles.filter((path) => path.endsWith(".rs"));
const diff = git(["diff", "--unified=0", baseline, "--", "packages/engine/tests"]);
const baselineFiles = new Map();
const currentFiles = new Map();
for (const path of changedFiles) {
  try { baselineFiles.set(path, git(["show", `${baseline}:${path}`])); } catch { issues.push(issue("EXPANDED", path, null, "tracked deletion/replacement")); }
  currentFiles.set(path, await readFile(new URL(`../../${path}`, import.meta.url), "utf8"));
}
const hunks = rustTestHunks(diff);
const tracked = classifyTracked({ hunks, baselineFiles, currentFiles, exactC: inventory.class_c.exact, classB: sealed.b_test_inventory, classBChanges: inventory.class_b_changes, forbiddenTokens: inventory.forbidden_tokens });
issues.push(...tracked.issues);
for (const entry of inventory.class_a) {
  const source = await readFile(new URL(`../../${entry.path}`, import.meta.url), "utf8");
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
  else if (sha256(await readFile(new URL(`../../${path}`, import.meta.url), "utf8")) !== entry.sha256) issues.push(issue("RECLASSIFIED", path, null, "untracked hash"));
  else untrackedClassified += 1;
}
for (const item of issues.sort((left, right) => JSON.stringify(left).localeCompare(JSON.stringify(right)))) console.log(JSON.stringify(item));
if (issues.length) process.exitCode = 1;
else console.log(JSON.stringify({ status: "PASS", baseline, ancestry: true, overlay_test_entries: 0, class_a_entries: inventory.class_a.length, class_b_entries: inventory.class_b.length, class_b_mapped_assertions: sealed.b_test_inventory.reduce((sum, entry) => sum + entry.assertions.length, 0), active_b_changes: inventory.class_b_changes.length, class_c_entries: inventory.class_c.exact.length, tracked_files_total: changedFiles.length, tracked_hunks_total: hunks.length, tracked_hunks_classified: tracked.mechanical + tracked.exact, generic_mechanical_symbol_count: tracked.mechanicalSymbols, generic_mechanical_hunk_count: tracked.mechanical, exact_c_hunk_count: tracked.exact, untracked_count: untrackedPaths.filter((path) => !path.endsWith("preserved-test-inventory.md") && !path.endsWith("preserved-test-inventory.json")).length, untracked_classified: untrackedClassified }));
