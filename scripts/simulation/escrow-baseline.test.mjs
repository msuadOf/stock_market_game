import assert from "node:assert/strict";
import { test } from "node:test";
import { mkdtemp, writeFile, mkdir, chmod, symlink, rm, readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { captureFiles, applyArchive, validateEntries, verifyClosure, sha256 } from "./escrow/closure.mjs";
import { run } from "./escrow/process.mjs";
import { compareFrames, verifyFrames, feasibility } from "./escrow/compare.mjs";
import { parseArgs, validateOutput, verifyReuse } from "./escrow/options.mjs";
import { inventoryCandidates, verifyInventory } from "./escrow/inventory.mjs";

async function fixture(context) {
  const root = await mkdtemp(join(tmpdir(), "escrow-test-"));
  context.after(() => rm(root, { recursive: true, force: true }));
  return root;
}

test("rejects malformed arguments and traversal output before side effects", async (context) => {
  const root = await fixture(context);
  for (const args of [["--wat"], ["collect"], ["verify", "x", "extra"]]) {
    assert.throws(() => parseArgs(args));
  }
  for (const path of ["/", root, join(root, "../escape"), join(root, ".git")]) {
    await assert.rejects(validateOutput(root, path));
  }
});

test("captures applicable untracked bytes and preserves content and executable mode", async (context) => {
  const root = await fixture(context);
  await mkdir(join(root, "packages"));
  await writeFile(join(root, "packages/input"), "untracked overlay\n", { mode: 0o755 });
  const archive = await captureFiles(root, ["packages/input"], new Map());
  assert.equal(archive.entries[0].origin, "untracked");
  assert.equal(archive.entries[0].mode, "100755");
  const destination = join(root, "detached");
  await mkdir(destination);
  await applyArchive(destination, archive);
  assert.equal(await readFile(join(destination, "packages/input"), "utf8"), "untracked overlay\n");
  assert.deepEqual(await captureFiles(root, ["packages/input"], new Map()), archive);
});

test("rejects corrupted content and mode archives", async (context) => {
  const root = await fixture(context);
  await writeFile(join(root, "input"), "original");
  const archive = await captureFiles(root, ["input"], new Map());
  const corrupt = structuredClone(archive);
  corrupt.blobs[corrupt.entries[0].sha256] = Buffer.from("mutated").toString("base64");
  await assert.rejects(applyArchive(root, corrupt), /hash|content/);
  await chmod(join(root, "input"), 0o755);
  assert.notEqual(sha256(JSON.stringify(archive)), sha256(JSON.stringify(await captureFiles(root, ["input"], new Map()))));
});

test("rejects traversal, duplicates, case collisions and unsupported entry modes", () => {
  const entry = { path: "file", entry_type: "file", mode: "100644", byte_length: 0, sha256: sha256("") };
  for (const entries of [
    [{ ...entry, path: "../escape" }], [{ ...entry, path: "/absolute" }],
    [{ ...entry, path: "a/./b" }], [{ ...entry, path: "a\\b" }],
    [entry, entry], [entry, { ...entry, path: "FILE" }],
    [{ ...entry, mode: "100777" }], [{ ...entry, entry_type: "symlink" }],
  ]) assert.throws(() => validateEntries(entries));
});

test("rejects actual symlinks and special files without following or blocking", async (context) => {
  const root = await fixture(context);
  await writeFile(join(root, "target"), "data");
  await symlink("target", join(root, "link"));
  await assert.rejects(captureFiles(root, ["link"], new Map()), /symlink|regular/);
  assert.equal(spawnSync("mkfifo", [join(root, "pipe")]).status, 0);
  await assert.rejects(captureFiles(root, ["pipe"], new Map()), /regular/);
});

test("rejects a symlink ancestor when applying overlay bytes", async (context) => {
  const root = await fixture(context);
  await mkdir(join(root, "source"));
  await mkdir(join(root, "outside"));
  await writeFile(join(root, "source/input"), "payload");
  const archive = await captureFiles(root, ["source/input"], new Map());
  const destination = join(root, "detached");
  await mkdir(destination);
  await symlink(join(root, "outside"), join(destination, "source"));
  await assert.rejects(applyArchive(destination, archive), /parent/);
});

test("rejects differing detached mode and undeclared closure file", async (context) => {
  const root = await fixture(context);
  await mkdir(join(root, "engine"));
  await mkdir(join(root, ".cargo"));
  await writeFile(join(root, "engine/input"), "payload");
  const archive = await captureFiles(root, ["engine/input"], new Map());
  const baseline = { closure: archive.entries, roots: ["engine"] };
  await chmod(join(root, "engine/input"), 0o755);
  await assert.rejects(verifyClosure(root, baseline), /mode/);
  await chmod(join(root, "engine/input"), 0o644);
  await writeFile(join(root, "engine/undeclared"), "payload");
  await assert.rejects(verifyClosure(root, baseline), /undeclared/);
});

test("rejects misleading stdout when child fails", async () => {
  await assert.rejects(run(process.execPath, ["-e", 'console.log("{\\"success\\":true}");process.exit(7)']), /exit.*7/);
});

test("kills hung child group on timeout", async () => {
  await assert.rejects(run(process.execPath, ["-e", "setInterval(()=>{},1000)"], { timeout: 50 }), /timeout/);
});

test("kills child group on interrupt signal", async () => {
  const controller = new AbortController();
  const pending = run(process.execPath, ["-e", "setInterval(()=>{},1000)"], { signal: controller.signal });
  controller.abort();
  await assert.rejects(pending, /abort|interrupt/);
});

function frames() {
  return [{ tick: 1, seq_from: 1, seq_to: 2, timeseries_payload: [{ price: 100 }], events: [
    { identity: [1, "OrderAccepted", "Account:0", 1], event: { OrderAccepted: { seq: 1, account: 0, id: 1, price: 100 } } },
    { identity: [1, "AuctionTick", "Stock:600001", 0], event: { AuctionTick: { seq: 2, code: "600001", price: 100 } } },
  ] }];
}

test("compares facts by identity and payload while allowing frame-local reordering", () => {
  const baseline = frames();
  const reordered = structuredClone(baseline);
  reordered[0].events.reverse();
  compareFrames(baseline, reordered);
  assert.equal(feasibility(baseline).reorder_accepted, true);
});

test("rejects missing duplicate collision payload swap wrong entity and timeseries changes", () => {
  const baseline = frames();
  const mutations = [
    (frame) => frame.events.pop(),
    (frame) => frame.events.push(structuredClone(frame.events[0])),
    (frame) => { frame.events[1].identity = frame.events[0].identity; },
    (frame) => { [frame.events[0].event, frame.events[1].event] = [frame.events[1].event, frame.events[0].event]; },
    (frame) => { frame.events[0].event.OrderAccepted.price = 101; },
    (frame) => { frame.events[0].identity[2] = "Account:9"; },
    (frame) => { frame.timeseries_payload = []; },
    (frame) => { frame.seq_to = 3; },
    (frame) => { frame.tick = 2; },
  ];
  for (const mutate of mutations) {
    const changed = structuredClone(baseline);
    mutate(changed[0]);
    assert.throws(() => compareFrames(baseline, changed));
  }
  assert.throws(() => verifyFrames([baseline[0], baseline[0]]));
});

test("rejects stale composite or harness identities and tampered run artifacts", async (context) => {
  const root = await fixture(context);
  await writeFile(join(root, "run.jsonl"), "{}\n");
  const identity = { composite: "old", harness: "harness" };
  const manifest = { identity, runs: [{ file: "run.jsonl", sha256: sha256("{}\n") }] };
  await verifyReuse(root, manifest, identity);
  await assert.rejects(verifyReuse(root, manifest, { ...identity, composite: "new" }), /identity/);
  await assert.rejects(verifyReuse(root, manifest, { ...identity, harness: "new" }), /identity/);
  await writeFile(join(root, "run.jsonl"), "mutated");
  await assert.rejects(verifyReuse(root, manifest, identity), /digest/);
});

test("rejects symlinked run receipt even if target digest matches", async (context) => {
  const root = await fixture(context);
  await writeFile(join(root, "target"), "same");
  await symlink("target", join(root, "run.jsonl"));
  const identity = { composite: "a", harness: "b" };
  await assert.rejects(verifyReuse(root, { identity, runs: [{ file: "run.jsonl", sha256: sha256("same") }] }, identity), /regular/);
});

test("enumerates candidate union and excludes the soft-budget anchor mechanically", () => {
  const source = '#[test]\nfn reserve() { assert_eq!(snapshot.reserved_cash, 400); }\n#[test]\nfn accepted() { assert!(matches!(event, Event::OrderAccepted { .. })); }\n#[test]\nfn unrelated() { assert_eq!(2, 2); }';
  const candidates = inventoryCandidates("tests/example.rs", source);
  assert.deepEqual(candidates.map((item) => item.symbol), ["reserve", "accepted"]);
  const soft = inventoryCandidates("packages/engine/tests/company_scenarios/constraints.rs", '#[test]\nfn soft() { let fee_reserve = 500; assert_eq!(allocated_cash, 200); }');
  assert.equal(soft[0].classification, "excluded");
  assert.match(soft[0].reason, /soft|软预算/);
  assert.ok(candidates.every((item) => item.source_sha256 && item.assertions.length > 0));
});

test("rejects assertion insertion movement and mutation against frozen inventory", () => {
  const path = "tests/input.rs";
  const baseline = '#[test]\nfn reservation() { assert_eq!(reserved_cash, 400); assert_eq!(available_cash, 600); }';
  const frozen = inventoryCandidates(path, baseline);
  verifyInventory(frozen, inventoryCandidates(path, baseline));
  for (const altered of [
    baseline.replace("assert_eq!(reserved_cash", "assert!(true); assert_eq!(reserved_cash"),
    baseline.replace("assert_eq!(reserved_cash, 400); assert_eq!(available_cash, 600);", "assert_eq!(available_cash, 600); assert_eq!(reserved_cash, 400);"),
    baseline.replace("400", "401"),
  ]) assert.throws(() => verifyInventory(frozen, inventoryCandidates(path, altered)), /inventory/);
});
