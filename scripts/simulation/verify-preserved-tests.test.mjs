import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { assertions, classifyTracked, issue, itemWithoutAssertionsWithCanonicalSymbol, normalizeResultBody, normalizedTokens, parseHunks, protectedRustHunks, rustItems, rustTestHunks, sha256, verifyApprovedDivergenceSchema, verifyBChange, verifyClassA, verifyInventoryMarkdown, verifyInventorySchema, verifyInventoryShape, verifySealedEvidence } from "./preserved-tests/core.mjs";

const before = "#[test]\nfn sample() {\n let qty = 1; session.save(); assert_eq!(cash, 1);\n}\n";
const hunk = (after) => parseHunks(`diff --git a/a.rs b/a.rs\n+++ b/a.rs\n@@ -2 +2 @@ fn sample() {\n-${before.split("\n")[2]}\n+${after.split("\n")[2]}`);
const classify = (after, exactC = []) => classifyTracked({ hunks: hunk(after), baselineFiles: new Map([["a.rs", before]]), currentFiles: new Map([["a.rs", after]]), exactC, forbiddenTokens: ["reserved_cash", "available_cash", "InsufficientCash", "IntentRejected", "OrderAccepted", "fee", "commission", "decision", "qty", "price", "SessionSetup"] });
const mechanical = "#[test]\nfn sample() {\n let qty = 1; session.save().expect(\"healthy save\"); assert_eq!(cash, 1);\n}\n";

test("multi-line Result-only item rewrite passes", () => assert.equal(classify(mechanical).issues.length, 0));
test("numeric assertion change is rejected", () => assert.equal(classify(mechanical.replace("cash, 1", "cash, 2")).issues[0].code, "EXPANDED"));
test("scenario input change is expanded", () => assert.equal(classify(mechanical.replace("qty = 1", "SessionSetup { qty: 2 }")).issues[0].code, "EXPANDED"));
test("price and every protected Result-mixed field are expanded", () => {
  for (const token of ["price", "reserved_cash", "available_cash", "InsufficientCash", "IntentRejected", "OrderAccepted", "fee", "commission", "decision"]) assert.equal(classify(mechanical.replace("cash", token)).issues[0].code, "EXPANDED", token);
});
test("assertion deletion is rejected", () => assert.equal(classify("#[test]\nfn sample() {\n let qty = 1; session.save().expect(\"healthy save\");\n}\n").issues[0].code, "EXPANDED"));
test("full restore item Result normalization preserves both filled-quantity assertions", () => {
  const path = "packages/engine/tests/company_scenarios/restore.rs";
  const symbol = "live_partial_fill_restores_and_continues_identically";
  const sourceBefore = spawnSync("git", ["show", `7041d35dc362ca74f4f3313e6804db9499f0679a:${path}`], { encoding: "utf8" }).stdout;
  const sourceAfter = readFileSync(path, "utf8");
  const baselineItem = rustItems(sourceBefore).find((item) => item.symbol === symbol);
  const currentItem = rustItems(sourceAfter).find((item) => item.symbol === symbol);
  assert.ok(currentItem.body.includes("uninterrupted") && currentItem.body.includes("restored"));
  assert.equal(normalizeResultBody(baselineItem.body), normalizeResultBody(currentItem.body));
});
function restoreMutation(source, assertion) {
  const path = "packages/engine/tests/company_scenarios/restore.rs";
  const symbol = "live_partial_fill_restores_and_continues_identically";
  const current = source.replace(assertion, "");
  const hunks = parseHunks(`diff --git a/${path} b/${path}\n+++ b/${path}\n@@ -131 +131,0 @@\n-${assertion.trim()}`);
  const result = classifyTracked({
    hunks,
    baselineFiles: new Map([[path, source]]),
    currentFiles: new Map([[path, current]]),
    exactC: [],
    forbiddenTokens: [],
  });
  return { result, path, symbol };
}
test("production classifier rejects deletion of either restore filled-quantity assertion", () => {
  const source = readFileSync("packages/engine/tests/company_scenarios/restore.rs", "utf8");
  for (const assertion of [
    "    assert_eq!(\n        uninterrupted\n            .save()\n            .expect(\"healthy save\")\n            .plans\n            .plan(buyer)\n            .unwrap()\n            .filled_qty,\n        200\n    );\n",
    "    assert_eq!(\n        restored\n            .save()\n            .expect(\"healthy save\")\n            .plans\n            .plan(buyer)\n            .unwrap()\n            .filled_qty,\n        200\n    );\n",
  ]) {
    const { result, path, symbol } = restoreMutation(source, assertion);
    assert.ok(result.issues.length > 0);
    assert.equal(result.issues[0].path, path);
    assert.equal(result.issues[0].symbol, symbol);
  }
});
test("production Class-A verifier rejects a semantic mutation", () => {
  const source = "fn protected() { assert_eq!(cash, 1); }";
  const entry = { path: "a.rs", symbol: "protected", anchor: "assert_eq!", baseline_hash: sha256(normalizeResultBody(rustItems(source)[0].body)) };
  assert.equal(verifyClassA({ entry, baselineSource: source, currentSource: source.replace("cash, 1", "cash, 2") }).code, "EXPANDED");
});
test("production classifier reclassifies a B identity or anchor mismatch", () => {
  for (const mutate of [(change) => { change.assertions[0].identity = "sample:assertion:9"; }, (change) => { change.assertions[0].baseline_anchor = "wrong"; }]) {
    const data = bFixture();
    mutate(data.change);
    const hunks = parseHunks(`diff --git a/a.rs b/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-${data.sourceBefore}\n+${data.sourceAfter}`);
    const result = classifyTracked({ hunks, baselineFiles: new Map([["a.rs", data.sourceBefore]]), currentFiles: new Map([["a.rs", data.sourceAfter]]), exactC: [], classB: [data.entry], classBChanges: [data.change], forbiddenTokens: [] });
    assert.equal(result.issues[0].code, "RECLASSIFIED");
  }
});
test("production classifier rejects undeclared Class-C and mixed Result semantic changes", () => {
  const semantic = classify(mechanical.replace("cash, 1", "cash, 2"));
  assert.equal(semantic.issues[0].code, "EXPANDED");
  const mixed = classify(mechanical.replace("cash, 1", "cash, 2").replace("healthy save", "healthy step"));
  assert.equal(mixed.issues[0].code, "EXPANDED");
});
function productionRoot() {
  const directory = mkdtempSync(join(tmpdir(), "preserved-tests-cli-"));
  const result = spawnSync("git", ["worktree", "add", "--detach", join(directory, "tree"), "7041d35dc362ca74f4f3313e6804db9499f0679a"], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  const root = join(directory, "tree");
  const evidence = existsSync(join(process.cwd(), ".omo/evidence/escrow-parallel-engine/baseline-corpus/attempt-12"))
    ? join(process.cwd(), ".omo/evidence/escrow-parallel-engine/baseline-corpus/attempt-12")
    : join(process.cwd(), "..", "..", ".omo/evidence/escrow-parallel-engine/baseline-corpus/attempt-12");
  const corpusRoot = join(evidence, "..");
  for (const [source, destination] of [["scripts/simulation/verify-preserved-tests.mjs", "scripts/simulation/verify-preserved-tests.mjs"], ["scripts/simulation/preserved-tests", "scripts/simulation/preserved-tests"], [corpusRoot, ".omo/evidence/escrow-parallel-engine/baseline-corpus"], ["packages/engine/tests/preserved-test-inventory.json", "packages/engine/tests/preserved-test-inventory.json"], ["packages/engine/tests/preserved-test-inventory.md", "packages/engine/tests/preserved-test-inventory.md"]]) { mkdirSync(join(root, destination, ".."), { recursive: true }); cpSync(source, join(root, destination), { recursive: true }); }
  for (const path of ["packages/engine/tests/company_scenarios/constraints.rs", "packages/engine/tests/company_scenarios/restore.rs"]) { mkdirSync(join(root, path, ".."), { recursive: true }); cpSync(path, join(root, path)); }
  return { directory, root };
}
function replaceRequired(source, expected, replacement) {
  const mutated = source.replace(expected, replacement);
  assert.notEqual(mutated, source, `production mutation target not found: ${expected}`);
  return mutated;
}
function runProductionMutation(mutate) {
  const { directory, root } = productionRoot();
  try {
    mutate(root);
    const diff = spawnSync("git", ["diff", "--unified=0", "7041d35dc362ca74f4f3313e6804db9499f0679a", "--", "packages/engine/tests"], { cwd: root, encoding: "utf8" });
    assert.equal(diff.status, 0, diff.stderr);
    assert.notEqual(diff.stdout, "", "production mutation must enter git diff <baseline>");
    const result = spawnSync("node", ["scripts/simulation/verify-preserved-tests.mjs"], { cwd: root, encoding: "utf8" });
    assert.equal(result.status, 1, result.stdout);
    return result.stdout.trim().split("\n").map(JSON.parse);
  } finally {
    spawnSync("git", ["worktree", "remove", "--force", root], { encoding: "utf8" });
    rmSync(directory, { recursive: true, force: true });
  }
}
test("production CLI rejects restore deletions, A semantics, B mappings, and mixed C semantics", () => {
  const evidence = join(process.cwd(), ".omo/evidence/escrow-parallel-engine/baseline-corpus/attempt-12");
  const fallbackEvidence = join(process.cwd(), "..", "..", ".omo/evidence/escrow-parallel-engine/baseline-corpus/attempt-12");
  if (!existsSync(evidence) && !existsSync(fallbackEvidence)) {
    test.skip("sealed attempt-12 evidence is not present in this isolated worktree");
    return;
  }
  const restorePath = "packages/engine/tests/company_scenarios/restore.rs";
  for (const assertion of [
    "    assert_eq!(\n        uninterrupted\n            .save()\n            .expect(\"healthy save\")\n            .plans\n            .plan(buyer)\n            .unwrap()\n            .filled_qty,\n        200\n    );\n",
    "    assert_eq!(\n        restored\n            .save()\n            .expect(\"healthy save\")\n            .plans\n            .plan(buyer)\n            .unwrap()\n            .filled_qty,\n        200\n    );\n",
  ]) {
    const issues = runProductionMutation((root) => writeFileSync(join(root, restorePath), replaceRequired(readFileSync(join(root, restorePath), "utf8"), assertion, "")));
    assert.ok(issues.some((issue) => issue.path === restorePath && issue.symbol === "live_partial_fill_restores_and_continues_identically"));
  }
  const aIssues = runProductionMutation((root) => {
    const path = "packages/engine/tests/company_scenarios/constraints.rs";
    writeFileSync(join(root, path), replaceRequired(readFileSync(join(root, path), "utf8"), "assert_eq!(result.grants[0].allocated_cash, Money::from_cents(500));", "assert_eq!(result.grants[0].allocated_cash, Money::from_cents(501));"));
  });
  assert.ok(aIssues.some((issue) => issue.code === "EXPANDED" && issue.symbol === "unfilled_cross_stock_sale_proceeds_never_finance_oversubscribed_buys"), JSON.stringify(aIssues));
  const bIssues = runProductionMutation((root) => {
    const inventoryPath = join(root, "packages/engine/tests/preserved-test-inventory.json");
    const inventory = JSON.parse(readFileSync(inventoryPath, "utf8"));
    inventory.class_b[0].assertions[0].hunk_anchor = "wrong";
    writeFileSync(inventoryPath, JSON.stringify(inventory));
  });
  assert.ok(bIssues.some((issue) => issue.code === "RECLASSIFIED" && issue.symbol === "planned_sell_fee_is_reserved_before_a_later_buy"));
  const mixedIssues = runProductionMutation((root) => {
    const resultOnly = replaceRequired(readFileSync(join(root, restorePath), "utf8"), ".expect(\"healthy save\")", ".expect(\"healthy step\")");
    writeFileSync(join(root, restorePath), replaceRequired(resultOnly, "filled_qty,\n        200", "filled_qty,\n        201"));
  });
  assert.ok(mixedIssues.some((issue) => issue.code === "ADDED" || issue.code === "EXPANDED"));
});
test("removed helper without exact C is rejected", () => assert.equal(classifyTracked({ hunks: hunk("fn other() {}\n"), baselineFiles: new Map([["a.rs", before]]), currentFiles: new Map([["a.rs", "fn other() {}\n"]]), exactC: [], forbiddenTokens: [] }).issues[0].code, "ADDED"));
test("exact C path/symbol mismatch is rejected", () => {
  const changed = mechanical.replace("cash, 1", "cash, 2");
  const entry = [{ path: "other.rs", symbol: "other", hunk_hashes: [hunk(changed)[0].hash] }];
  assert.equal(classify(changed, entry).issues[0].code, "EXPANDED");
});
test("tuple comma and token boundary stay semantically distinct", () => {
  const tuple = mechanical.replace("cash, 1", "(cash,)");
  assert.equal(classify(tuple).issues[0].code, "EXPANDED");
  const boundary = mechanical.replace("let qty", "let returnx").replace("qty = 1", "return x = 1");
  assert.equal(classify(boundary).issues[0].code, "EXPANDED");
});
test("lexer ignores braces only inside Rust literals and nested comments", () => {
  const source = '#[test] fn first<T>() where T: Copy { /* } /* { */ */ let a = r###"}"###; let b = b"}"; let c = br#"}"#; let d = \'{\'; } #[test] fn second() {}';
  assert.deepEqual(rustItems(source).map((item) => item.symbol), ["first", "second"]);
});
test("B metadata mismatches are reclassified", () => {
  const sealed = { b_test_inventory: [{ file: "a.rs", symbol: "sample", effect_id: "E", allowed_transformation: "allowed", reason: "r", candidate_terms: ["cash"], assertions: [{ identity: "a", line: 1, hunk_anchor: "h" }] }] };
  for (const field of ["effect_id", "allowed_transformation", "assertions"]) { const actual = structuredClone(sealed.b_test_inventory[0]); actual[field] = field === "assertions" ? [] : "wrong"; assert.ok(["RECLASSIFIED", "MISSING"].includes(verifyInventoryShape({ class_b: [actual] }, sealed)[0].code)); }
});
test("production Class-A verification rejects body and anchor mutations", () => {
  const baseline = "fn protected() { assert_eq!(cash, 1); }";
  const entry = { path: "a.rs", symbol: "protected", anchor: "assert_eq!", baseline_hash: sha256(normalizeResultBody(rustItems(baseline)[0].body)) };
  assert.equal(verifyClassA({ entry, baselineSource: baseline, currentSource: baseline }), null);
  assert.equal(verifyClassA({ entry, baselineSource: baseline, currentSource: baseline.replace("cash, 1", "cash, 2") }).code, "EXPANDED");
  assert.equal(verifyClassA({ entry, baselineSource: baseline, currentSource: baseline.replace("assert_eq!", "assert_ne!") }).code, "MISSING");
  assert.equal(verifyClassA({ entry, baselineSource: baseline, currentSource: baseline.replace("cash, 1", "cash, 2") }).code, "EXPANDED");
});
test("machine issue codes are stable", () => { for (const code of ["ADDED", "MISSING", "RECLASSIFIED", "EXPANDED"]) assert.equal(issue(code, "a", "s", "d").code, code); });
test("inventory metadata is schema-validated without invoking the Rust lexer", () => {
  const valid = { baseline: "7041d35dc362ca74f4f3313e6804db9499f0679a", forbidden_tokens: [], class_a: [], class_b: [], class_b_changes: [], approved_divergence_changes: [], foundation_overlays: [], class_c: { exact: [], additive: [] } };
  assert.deepEqual(verifyInventorySchema(valid), []);
  const malformed = structuredClone(valid);
  malformed.class_c.exact = "/* Markdown is not Rust */";
  const issues = verifyInventorySchema(malformed);
  assert.equal(issues.length, 1);
  assert.equal(issues[0].code, "RECLASSIFIED");
});
test("approved divergence metadata requires a bounded source identity", () => {
  const valid = [{
    path: "packages/engine/tests/auction.rs", divergence_id: "D4_SAME_TICK_ORDER_NOT_CANCELABLE",
    baseline_symbol: "baseline", baseline_normalized_sha256: "a".repeat(64),
    current_symbols: [{ symbol: "current", normalized_sha256: "b".repeat(64) }], hunk_hashes: ["c".repeat(64)],
    allowed_transformation: "bounded", forbidden_expansion: ["fees"],
  }];
  assert.deepEqual(verifyApprovedDivergenceSchema(valid), []);
  const malformed = structuredClone(valid);
  malformed[0].hunk_hashes.push(malformed[0].hunk_hashes[0]);
  assert.equal(verifyApprovedDivergenceSchema(malformed)[0].code, "RECLASSIFIED");
  const unbounded = structuredClone(valid);
  delete unbounded[0].current_symbols;
  assert.equal(verifyApprovedDivergenceSchema(unbounded)[0].code, "RECLASSIFIED");
});
test("approved divergence schema rejects unknown, empty, invalid revision, and invalid evidence fields", () => {
  const valid = { path: "a.rs", divergence_id: "D6", baseline_revision: "abcdef0", baseline_sha256: "a".repeat(64), current_sha256: "b".repeat(64), symbols: ["current"], allowed_transformation: "bounded", forbidden_expansion: ["fees"] };
  for (const mutate of [
    (entry) => { entry.unexpected = true; },
    (entry) => { entry.forbidden_expansion = [""]; },
    (entry) => { entry.baseline_revision = "not-a-commit"; },
    (entry) => { entry.evidence = { event_count: -1 }; },
    (entry) => { entry.evidence = { event_count: 1, unexpected: true }; },
  ]) { const entry = structuredClone(valid); mutate(entry); assert.equal(verifyApprovedDivergenceSchema([entry])[0].code, "RECLASSIFIED"); }
  const exact = { path: "a.rs", divergence_id: "D4", baseline_symbol: "before", baseline_normalized_sha256: "a".repeat(64), current_symbols: [{ symbol: "after", normalized_sha256: "b".repeat(64) }], hunk_hashes: ["c".repeat(64)], allowed_transformation: "bounded", forbidden_expansion: ["fees"] };
  for (const evidence of ["arbitrary", { event_count: 1, unexpected: true }]) { const entry = structuredClone(exact); entry.evidence = evidence; assert.equal(verifyApprovedDivergenceSchema([entry])[0].code, "RECLASSIFIED"); }
  for (const evidence of [{ event_count: 1, changed_serialized_event_positions: -1 }, { normalized_event_multiset_sha256: "not-a-hash" }, { mid_save_common_fields_sha256: 42 }]) { const entry = structuredClone(valid); entry.evidence = evidence; assert.equal(verifyApprovedDivergenceSchema([entry])[0].code, "RECLASSIFIED"); }
});
test("approved exact divergence hunk requires baseline and current symbol hashes", () => {
  const sourceBefore = "fn baseline() { assert_eq!(value, 1); }\n";
  const sourceAfter = "fn current() { assert_eq!(value, 2); }\n";
  const hunks = parseHunks(`diff --git a/a.rs b/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-${sourceBefore.trim()}\n+${sourceAfter.trim()}`);
  const approved = [{
    path: "a.rs", divergence_id: "D4", baseline_symbol: "baseline",
    baseline_normalized_sha256: sha256(normalizeResultBody(rustItems(sourceBefore)[0].body)),
    current_symbols: [{ symbol: "current", normalized_sha256: sha256(normalizeResultBody(rustItems(sourceAfter)[0].body)) }],
    hunk_hashes: [hunks[0].hash], allowed_transformation: "bounded", forbidden_expansion: ["fees"],
  }];
  assert.equal(classifyTracked({ hunks, baselineFiles: new Map([["a.rs", sourceBefore]]), currentFiles: new Map([["a.rs", sourceAfter]]), exactC: [], approvedDivergences: approved, forbiddenTokens: [] }).issues.length, 0);
  approved[0].current_symbols[0].normalized_sha256 = "0".repeat(64);
  assert.equal(classifyTracked({ hunks, baselineFiles: new Map([["a.rs", sourceBefore]]), currentFiles: new Map([["a.rs", sourceAfter]]), exactC: [], approvedDivergences: approved, forbiddenTokens: [] }).issues[0].code, "ADDED");
});
test("approved whole-file divergence requires the declared current bytes", () => {
  const sourceBefore = "fn baseline() { assert_eq!(value, 1); }\n";
  const sourceAfter = "fn current() { assert_eq!(value, 2); }\n";
  const hunks = parseHunks(`diff --git a/a.rs b/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-${sourceBefore.trim()}\n+${sourceAfter.trim()}`);
  const approved = [{
    path: "a.rs", divergence_id: "D7", baseline_revision: "base", baseline_sha256: sha256(sourceBefore),
    current_sha256: sha256(sourceAfter), symbols: ["current"], allowed_transformation: "bounded", forbidden_expansion: ["fees"],
  }];
  assert.equal(classifyTracked({ hunks, baselineFiles: new Map([["a.rs", sourceBefore]]), currentFiles: new Map([["a.rs", sourceAfter]]), exactC: [], approvedDivergences: approved, approvedBaselineFiles: new Map([["a.rs", sourceBefore]]), forbiddenTokens: [] }).issues.length, 0);
  assert.equal(classifyTracked({ hunks, baselineFiles: new Map([["a.rs", sourceBefore]]), currentFiles: new Map([["a.rs", sourceAfter.replace("2", "3")]]), exactC: [], approvedDivergences: approved, approvedBaselineFiles: new Map([["a.rs", sourceBefore]]), forbiddenTokens: [] }).issues[0].code, "ADDED");
});
test("foundation overlay cannot consume a path protected by Class-B", () => {
  const before = "fn sample() { assert!(event); }\n";
  const after = "fn sample() { assert!(event_changed); }\n";
  const hunks = parseHunks(`diff --git a/a.rs b/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-${before.trim()}\n+${after.trim()}`);
  const overlay = [{ path: "a.rs", baseline_revision: "7041d35dc362ca74f4f3313e6804db9499f0679a", foundation_revision: "c434f1d79ed07e29a09a0f23eb3809871f776470", baseline_sha256: sha256(before), current_sha256: sha256(after), symbols: ["sample"], allowed_transformation: "bounded", forbidden_expansion: ["fees"] }];
  const b = { file: "a.rs", symbol: "sample", effect_id: "E9-A_SELL_CASH_RESERVATION", allowed_transformation: "bounded", candidate_terms: ["event"], assertions: [{ identity: "sample:assertion:0", hunk_anchor: assertions(before, rustItems(before)[0])[0].anchor }] };
  const result = classifyTracked({ hunks, baselineFiles: new Map([["a.rs", before]]), currentFiles: new Map([["a.rs", after]]), exactC: [], foundationOverlays: overlay, classB: [b], classBChanges: [], forbiddenTokens: [] });
  assert.ok(result.issues.some((entry) => entry.code === "RECLASSIFIED"));
});
test("markdown inventory is validated independently and rejects missing policy markers", () => {
  const inventory = { baseline: "7041d35dc362ca74f4f3313e6804db9499f0679a" };
  const valid = "# 保留测试台账（Todo 3）\n## (a) 未改动保护\n## (b) 冻结 #9，严格四项\n## (c) 精确 Todo 2 合同改写\n`packages/engine/tests/preserved-test-inventory.json`\nscripts/simulation/verify-preserved-tests.mjs\ncommit `7041d35dc362ca74f4f3313e6804db9499f0679a`\n";
  assert.deepEqual(verifyInventoryMarkdown(valid, inventory), []);
  assert.equal(verifyInventoryMarkdown(valid.replace("## (b) 冻结 #9，严格四项", "## prose { not Rust }"), inventory)[0].code, "RECLASSIFIED");
});
test("Rust lexical classification accepts only Rust files below engine tests", () => {
  const diff = [
    "diff --git a/packages/engine/tests/kept.rs b/packages/engine/tests/kept.rs", "+++ b/packages/engine/tests/kept.rs", "@@ -1 +1 @@", "-fn before() {}", "+fn after() {}",
    "diff --git a/packages/engine/tests/preserved-test-inventory.md b/packages/engine/tests/preserved-test-inventory.md", "+++ b/packages/engine/tests/preserved-test-inventory.md", "@@ -1 +1 @@", "-prose", "+prose { fn not_rust() {} }",
    "diff --git a/packages/engine/src/not-a-test.rs b/packages/engine/src/not-a-test.rs", "+++ b/packages/engine/src/not-a-test.rs", "@@ -1 +1 @@", "-fn before() {}", "+fn after() {}",
  ].join("\n");
  assert.deepEqual(rustTestHunks(diff).map((hunk) => hunk.path), ["packages/engine/tests/kept.rs"]);
});
test("public Rust test hunk filter rejects traversal paths", () => {
  for (const path of ["packages/engine/tests/../src/session.rs", "packages/engine/tests/../../escape.rs"]) {
    const diff = `diff --git a/${path} b/${path}\n+++ b/${path}\n@@ -1 +1 @@\n-fn before() {}\n+fn after() {}`;
    assert.deepEqual(rustTestHunks(diff), [], path);
  }
});
test("an explicitly inventoried inline source test is lexed, but unrelated source is not", () => {
  const diff = [
    "diff --git a/packages/engine/src/session.rs b/packages/engine/src/session.rs", "+++ b/packages/engine/src/session.rs", "@@ -1 +1 @@", "-fn protected() {}", "+fn protected() {}",
    "diff --git a/packages/engine/src/market.rs b/packages/engine/src/market.rs", "+++ b/packages/engine/src/market.rs", "@@ -1 +1 @@", "-fn unrelated() {}", "+fn unrelated() {}",
  ].join("\n");
  assert.deepEqual(protectedRustHunks(diff, ["packages/engine/src/session.rs"]).map((hunk) => hunk.path), ["packages/engine/src/session.rs"]);
});
test("protected hunk filter rejects traversal paths and declarations without a diff", () => {
  const traversal = "diff --git a/packages/engine/tests/../src/session.rs b/packages/engine/tests/../src/session.rs\n+++ b/packages/engine/tests/../src/session.rs\n@@ -1 +1 @@\n-fn before() {}\n+fn after() {}";
  assert.deepEqual(protectedRustHunks(traversal, ["packages/engine/src/session.rs"]), []);
  const result = classifyTracked({ hunks: [], baselineFiles: new Map(), currentFiles: new Map(), exactC: [], classBChanges: [{ file: "a.rs", symbol: "missing", effect_id: "E", allowed_transformation: "x", assertions: [] }], forbiddenTokens: [] });
  assert.equal(result.issues[0].code, "MISSING");
});
test("sealed metadata rejects a mismatched closure source fingerprint and artifact hash", () => {
  const closure = { commit: "a".repeat(40), tree: "b".repeat(40), closure_digest: "c".repeat(64), overlay_digest: "d".repeat(64), closure: [] };
  const overlay = { entries: [], blobs: {} };
  const sealed = { candidates: [], b_test_inventory: [], explicit_exclusion: {}, direct_sell_cash_assertion_anchors: [] };
  const manifest = { schema: 2, status: "sealed", preserved_test_baseline_sha: closure.commit, head_tree: closure.tree, identity: { composite: sha256(JSON.stringify(closure)) }, closure_manifest_digest: closure.closure_digest, overlay_archive_digest: closure.overlay_digest, corpus_digest: "e".repeat(64), closure: [], inventory_file: "b-test-inventory.json", artifacts: [["closure.json", closure], ["overlay.json", overlay], ["b-test-inventory.json", sealed]].map(([file, value]) => ({ file, sha256: sha256(JSON.stringify(value)) })) };
  const manifestBytes = JSON.stringify(manifest);
  const index = { schema: 2, kind: "sealed-corpus-index", artifact_directory: "attempt-12", manifest: "attempt-12/manifest.json", manifest_sha256: sha256(manifestBytes), corpus_digest: manifest.corpus_digest, historical_attempts_are_not_current: true };
  const seal = { manifest_sha256: index.manifest_sha256, corpus_digest: index.corpus_digest, run_count: 0, cleanup_sha256: "f".repeat(64) };
  const input = { index, indexBytes: JSON.stringify(index), manifest, manifestBytes, seal, sealBytes: JSON.stringify(seal), closure, closureBytes: JSON.stringify(closure), overlay, overlayBytes: JSON.stringify(overlay), sealed, sealedBytes: JSON.stringify(sealed) };
  assert.deepEqual(verifySealedEvidence(input), []);
  input.index.manifest_sha256 = "0".repeat(64);
  assert.ok(verifySealedEvidence(input).some((entry) => entry.path === "../manifest.json"));
  input.index.manifest_sha256 = sha256(manifestBytes);
  input.seal.corpus_digest = "1".repeat(64);
  assert.ok(verifySealedEvidence(input).some((entry) => entry.path === "seal.json"));
  input.seal.corpus_digest = index.corpus_digest;
  input.manifest.head_tree = "e".repeat(40);
  assert.ok(verifySealedEvidence(input).some((entry) => entry.code === "RECLASSIFIED"));
  input.manifest.head_tree = closure.tree;
  input.overlay.entries.push({ path: "tampered.rs", sha256: "f".repeat(64), byte_length: 1 });
  input.overlay.blobs["f".repeat(64)] = "AA==";
  assert.ok(verifySealedEvidence(input).some((entry) => entry.path === "overlay.json"));
});
test("sealed metadata rejects malformed overlay and closure entries without throwing", () => {
  const closure = { commit: "a".repeat(40), tree: "b".repeat(40), closure_digest: "c".repeat(64), overlay_digest: "d".repeat(64), closure: [{ path: "../../escape", entry_type: "file", mode: "100644", byte_length: 0, sha256: "e".repeat(64), origin: "head" }] };
  const overlay = { entries: null, blobs: {} };
  const sealed = { candidates: [], b_test_inventory: [], explicit_exclusion: {}, direct_sell_cash_assertion_anchors: [] };
  const manifest = { schema: 2, status: "sealed", preserved_test_baseline_sha: closure.commit, head_tree: closure.tree, identity: { composite: sha256(JSON.stringify(closure)) }, closure_manifest_digest: closure.closure_digest, overlay_archive_digest: closure.overlay_digest, corpus_digest: "f".repeat(64), closure: structuredClone(closure.closure), inventory_file: "b-test-inventory.json", artifacts: [["closure.json", closure], ["overlay.json", overlay], ["b-test-inventory.json", sealed]].map(([file, value]) => ({ file, sha256: sha256(JSON.stringify(value)) })) };
  const manifestBytes = JSON.stringify(manifest);
  const index = { schema: 2, kind: "sealed-corpus-index", artifact_directory: "attempt-12", manifest: "attempt-12/manifest.json", manifest_sha256: sha256(manifestBytes), corpus_digest: manifest.corpus_digest, historical_attempts_are_not_current: true };
  const seal = { manifest_sha256: index.manifest_sha256, corpus_digest: index.corpus_digest, run_count: 0, cleanup_sha256: "d".repeat(64) };
  const issues = verifySealedEvidence({ index, indexBytes: JSON.stringify(index), manifest, manifestBytes, seal, sealBytes: JSON.stringify(seal), closure, closureBytes: JSON.stringify(closure), overlay, overlayBytes: JSON.stringify(overlay), sealed, sealedBytes: JSON.stringify(sealed) });
  assert.ok(issues.some((entry) => entry.path === "closure.json" && entry.code === "RECLASSIFIED"));
  assert.ok(issues.some((entry) => entry.path === "overlay.json" && entry.code === "RECLASSIFIED"));
});
test("CLI fails closed as BLOCKED when sealed legacy evidence is unavailable", () => {
  const { directory, root } = productionRoot();
  try {
    rmSync(join(root, ".omo/evidence/escrow-parallel-engine/baseline-corpus"), { recursive: true, force: true });
    const result = spawnSync("node", ["scripts/simulation/verify-preserved-tests.mjs"], { cwd: root, encoding: "utf8" });
    assert.equal(result.status, 2, result.stderr);
    assert.deepEqual(JSON.parse(result.stdout), {
      status: "BLOCKED",
      task9_acceptance: false,
      code: "MISSING_SEALED_EVIDENCE",
      path: join(root, ".omo/evidence/escrow-parallel-engine/baseline-corpus/manifest.json"),
      detail: "sealed attempt-12 evidence is required; no legacy witness is fabricated",
    });
  } finally {
    spawnSync("git", ["worktree", "remove", "--force", root], { encoding: "utf8" });
    rmSync(directory, { recursive: true, force: true });
  }
});
test("brace parser resolves test symbol", () => assert.equal(rustItems(before)[0].symbol, "sample"));
test("hash is deterministic", () => assert.equal(sha256("x"), sha256("x")));
test("only exact healthy suffix normalizes", () => assert.notDeepEqual(normalizedTokens('x.expect("other")'), normalizedTokens("x")));

function bFixture(effect = "E9-A_SELL_CASH_RESERVATION", oldAssertion = "assert_eq!(seller_reserved, 1);", newAssertion = "assert_eq!(seller_reserved, 2);") {
  const sourceBefore = `fn sample() { ${oldAssertion} }`;
  const sourceAfter = `fn sample() { ${newAssertion} }`;
  const oldItem = rustItems(sourceBefore)[0]; const newItem = rustItems(sourceAfter)[0]; const baseline = assertions(sourceBefore, oldItem); const current = assertions(sourceAfter, newItem);
  const entry = { file: "a.rs", symbol: "sample", effect_id: effect, allowed_transformation: effect === "E9-A_SELL_CASH_RESERVATION" ? "Only sell cash reservation and dependent available-cash/capped-intent output changes; buy reservation formula unchanged" : "Only sell InsufficientCash rejection and associated absent-order assertions become acceptance; preserve owned shares/T+1/nominal fees", candidate_terms: ["seller_reserved"], assertions: baseline.map((item, index) => ({ identity: `sample:assertion:${index}`, hunk_anchor: item.anchor })) };
  const change = { file: "a.rs", symbol: "sample", current_symbol: "sample", effect_id: effect, allowed_transformation: entry.allowed_transformation, assertions: [{ identity: "sample:assertion:0", baseline_anchor: baseline[0].anchor, current_hash: current[0].current_hash, effect_id: effect }], current_assertions: current.map((assertion) => assertion.current_hash), current_non_assertion_sha256: sha256(itemWithoutAssertionsWithCanonicalSymbol(newItem, current)) };
  return { entry, oldItem, newItem, sourceBefore, sourceAfter, change };
}
test("valid E9-A mapped seller assertion change passes", () => { const data = bFixture(); assert.equal(verifyBChange(data), null); });
test("valid E9-B acceptance change passes", () => { const data = bFixture("E9-B_SELL_ACCEPTANCE_FLIP", "assert!(events.iter().any(|e| matches!(e, IntentRejected { .. })));", "assert!(events.iter().any(|e| matches!(e, OrderAccepted { .. }))); "); data.entry.candidate_terms = ["IntentRejected", "OrderAccepted"]; assert.equal(verifyBChange(data), null); });
test("B buyer, scenario, non-assertion, and extra assertion changes expand", () => {
  for (const replacement of ["assert_eq!(buyer_reserved, 2);", "assert_eq!(qty, 2);", "let changed = true; assert_eq!(seller_reserved, 2);", "assert_eq!(seller_reserved, 2); assert_eq!(seller_reserved, 3);"]) {
    const data = bFixture();
    const sourceAfter = data.sourceAfter.replace("assert_eq!(seller_reserved, 2);", replacement);
    const newItem = rustItems(sourceAfter)[0];
    assert.ok(["EXPANDED", "RECLASSIFIED"].includes(verifyBChange({ ...data, sourceAfter, newItem }).code));
  }
});
test("B identity, anchor, effect, and transformation mismatches reclassify", () => {
  for (const mutate of [(data) => { data.change.assertions[0].identity = "sample:assertion:9"; }, (data) => { data.change.assertions[0].baseline_anchor = "wrong"; }, (data) => { data.change.effect_id = "wrong"; }, (data) => { data.change.allowed_transformation = "wrong"; }]) { const data = bFixture(); mutate(data); assert.equal(verifyBChange(data).code, "RECLASSIFIED"); }
});
function trackedB(after, effect = "E9-A_SELL_CASH_RESERVATION") {
  const before = "fn sample() { assert!(matches!(event, IntentRejected { reason: InsufficientCash })); }\n";
  const hunks = parseHunks(`diff --git a/a.rs b/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-${before.trim()}\n+${after.trim()}`);
  const oldItem = rustItems(before)[0]; const newItem = rustItems(after)[0]; const oldAssertions = assertions(before, oldItem); const newAssertions = assertions(after, newItem);
  const sealed = { file: "a.rs", symbol: "sample", effect_id: effect, allowed_transformation: effect === "E9-A_SELL_CASH_RESERVATION" ? "Only sell cash reservation and dependent available-cash/capped-intent output changes; buy reservation formula unchanged" : "Only sell InsufficientCash rejection and associated absent-order assertions become acceptance; preserve owned shares/T+1/nominal fees", candidate_terms: ["IntentRejected", "InsufficientCash", "OrderAccepted"], assertions: oldAssertions.map((item, index) => ({ identity: `sample:assertion:${index}`, hunk_anchor: item.anchor })) };
  const declaration = { file: "a.rs", symbol: "sample", current_symbol: "sample", effect_id: effect, allowed_transformation: sealed.allowed_transformation, assertions: [{ identity: "sample:assertion:0", baseline_anchor: oldAssertions[0].anchor, current_hash: newAssertions[0].current_hash, effect_id: effect }], current_assertions: newAssertions.map((assertion) => assertion.current_hash), current_non_assertion_sha256: sha256(itemWithoutAssertionsWithCanonicalSymbol(newItem, newAssertions)) };
  return classifyTracked({ hunks, baselineFiles: new Map([["a.rs", before]]), currentFiles: new Map([["a.rs", after]]), exactC: [], classB: [sealed], classBChanges: [declaration], forbiddenTokens: [] });
}
test("tracked hunk E9-A declaration passes through production classifier", () => assert.equal(trackedB("fn sample() { assert!(!matches!(event, IntentRejected { reason: InsufficientCash })); }\n").issues.length, 0));
test("tracked hunk E9-B declaration passes through production classifier", () => assert.equal(trackedB("fn sample() { assert!(matches!(event, OrderAccepted { })); }\n", "E9-B_SELL_ACCEPTANCE_FLIP").issues.length, 0));
test("tracked B declaration rejects undeclared change", () => { const result = trackedB("fn sample() { assert!(matches!(event, IntentRejected { reason: InsufficientCash })); assert!(true); }\n"); assert.ok(result.issues.length > 0); });
