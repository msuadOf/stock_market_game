import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { assertions, classifyTracked, issue, normalizeResultBody, normalizedTokens, parseHunks, rustItems, sha256, verifyBChange, verifyClassA, verifyInventorySchema, verifyInventoryShape } from "./preserved-tests/core.mjs";

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
  for (const path of ["scripts/simulation/verify-preserved-tests.mjs", "scripts/simulation/preserved-tests", ".omo/evidence/escrow-parallel-engine/baseline-corpus/attempt-12", "packages/engine/tests/preserved-test-inventory.json"]) { mkdirSync(join(root, path, ".."), { recursive: true }); cpSync(path, join(root, path), { recursive: true }); }
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
  const valid = { baseline: "7041d35dc362ca74f4f3313e6804db9499f0679a", forbidden_tokens: [], class_a: [], class_b: [], class_b_changes: [], class_c: { exact: [], additive: [] } };
  assert.deepEqual(verifyInventorySchema(valid), []);
  const malformed = structuredClone(valid);
  malformed.class_c.exact = "/* Markdown is not Rust */";
  const issues = verifyInventorySchema(malformed);
  assert.equal(issues.length, 1);
  assert.equal(issues[0].code, "RECLASSIFIED");
});
test("brace parser resolves test symbol", () => assert.equal(rustItems(before)[0].symbol, "sample"));
test("hash is deterministic", () => assert.equal(sha256("x"), sha256("x")));
test("only exact healthy suffix normalizes", () => assert.notDeepEqual(normalizedTokens('x.expect("other")'), normalizedTokens("x")));

function bFixture(effect = "E9-A_SELL_CASH_RESERVATION", oldAssertion = "assert_eq!(seller_reserved, 1);", newAssertion = "assert_eq!(seller_reserved, 2);") {
  const sourceBefore = `fn sample() { ${oldAssertion} }`;
  const sourceAfter = `fn sample() { ${newAssertion} }`;
  const oldItem = rustItems(sourceBefore)[0]; const newItem = rustItems(sourceAfter)[0]; const baseline = assertions(sourceBefore, oldItem); const current = assertions(sourceAfter, newItem);
  const entry = { file: "a.rs", symbol: "sample", effect_id: effect, allowed_transformation: effect === "E9-A_SELL_CASH_RESERVATION" ? "Only sell cash reservation and dependent available-cash/capped-intent output changes; buy reservation formula unchanged" : "Only sell InsufficientCash rejection and associated absent-order assertions become acceptance; preserve owned shares/T+1/nominal fees", candidate_terms: ["seller_reserved"], assertions: baseline.map((item, index) => ({ identity: `sample:assertion:${index}`, hunk_anchor: item.anchor })) };
  const change = { file: "a.rs", symbol: "sample", effect_id: effect, allowed_transformation: entry.allowed_transformation, assertions: [{ identity: "sample:assertion:0", baseline_anchor: baseline[0].anchor, current_hash: current[0].current_hash, effect_id: effect }] };
  return { entry, oldItem, newItem, sourceBefore, sourceAfter, change };
}
test("valid E9-A mapped seller assertion change passes", () => { const data = bFixture(); assert.equal(verifyBChange(data), null); });
test("valid E9-B acceptance change passes", () => { const data = bFixture("E9-B_SELL_ACCEPTANCE_FLIP", "assert!(events.iter().any(|e| matches!(e, IntentRejected { .. })));", "assert!(events.iter().any(|e| matches!(e, OrderAccepted { .. }))); "); data.entry.candidate_terms = ["IntentRejected", "OrderAccepted"]; assert.equal(verifyBChange(data), null); });
test("B buyer, scenario, non-assertion, and extra assertion changes expand", () => {
  for (const replacement of ["assert_eq!(buyer_reserved, 2);", "assert_eq!(qty, 2);", "let changed = true; assert_eq!(seller_reserved, 2);", "assert_eq!(seller_reserved, 2); assert_eq!(seller_reserved, 3);"]) { const data = bFixture(undefined, "assert_eq!(seller_reserved, 1);", replacement); assert.ok(["EXPANDED", "RECLASSIFIED"].includes(verifyBChange(data).code)); }
});
test("B identity, anchor, effect, and transformation mismatches reclassify", () => {
  for (const mutate of [(data) => { data.change.assertions[0].identity = "sample:assertion:9"; }, (data) => { data.change.assertions[0].baseline_anchor = "wrong"; }, (data) => { data.change.effect_id = "wrong"; }, (data) => { data.change.allowed_transformation = "wrong"; }]) { const data = bFixture(); mutate(data); assert.equal(verifyBChange(data).code, "RECLASSIFIED"); }
});
function trackedB(after, effect = "E9-A_SELL_CASH_RESERVATION") {
  const before = "fn sample() { assert!(matches!(event, IntentRejected { reason: InsufficientCash })); }\n";
  const hunks = parseHunks(`diff --git a/a.rs b/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-${before.trim()}\n+${after.trim()}`);
  const oldItem = rustItems(before)[0]; const newItem = rustItems(after)[0]; const oldAssertions = assertions(before, oldItem); const newAssertions = assertions(after, newItem);
  const sealed = { file: "a.rs", symbol: "sample", effect_id: effect, allowed_transformation: effect === "E9-A_SELL_CASH_RESERVATION" ? "Only sell cash reservation and dependent available-cash/capped-intent output changes; buy reservation formula unchanged" : "Only sell InsufficientCash rejection and associated absent-order assertions become acceptance; preserve owned shares/T+1/nominal fees", candidate_terms: ["IntentRejected", "InsufficientCash", "OrderAccepted"], assertions: oldAssertions.map((item, index) => ({ identity: `sample:assertion:${index}`, hunk_anchor: item.anchor })) };
  const declaration = { file: "a.rs", symbol: "sample", effect_id: effect, allowed_transformation: sealed.allowed_transformation, assertions: [{ identity: "sample:assertion:0", baseline_anchor: oldAssertions[0].anchor, current_hash: newAssertions[0].current_hash, effect_id: effect }] };
  return classifyTracked({ hunks, baselineFiles: new Map([["a.rs", before]]), currentFiles: new Map([["a.rs", after]]), exactC: [], classB: [sealed], classBChanges: [declaration], forbiddenTokens: [] });
}
test("tracked hunk E9-A declaration passes through production classifier", () => assert.equal(trackedB("fn sample() { assert!(!matches!(event, IntentRejected { reason: InsufficientCash })); }\n").issues.length, 0));
test("tracked hunk E9-B declaration passes through production classifier", () => assert.equal(trackedB("fn sample() { assert!(matches!(event, OrderAccepted { })); }\n", "E9-B_SELL_ACCEPTANCE_FLIP").issues.length, 0));
test("tracked B declaration rejects undeclared change", () => { const result = trackedB("fn sample() { assert!(matches!(event, IntentRejected { reason: InsufficientCash })); assert!(true); }\n"); assert.ok(result.issues.length > 0); });
