import assert from "node:assert/strict";
import { test } from "node:test";
import { CACHE_BUDGET_BYTES, planCachePrune, main } from "./prune-actions-cache.mjs";

const repository = "owner/game";
const hash = "a".repeat(64);
const native = (product = "desktop", platform = "ubuntu-24.04-X64") => `distribution-native-v1-${platform}-${product}-${hash}`;
const sealed = `sealed-cargo-v2-no-debug-Windows-X64-${hash}`;
function cache(id, prefix = native(), extra = {}) {
  return { id, key: `${prefix}-${id}-1`, ref: "refs/heads/main", size_in_bytes: 100,
    created_at: `2026-10-01T00:00:${String(id).padStart(2, "0")}Z`, ...extra };
}

test("retains the newest snapshot per ref, platform, architecture and product, regardless of manifest hash", () => {
  const entries = [cache(1), cache(2, native().replace(hash, "b".repeat(64))),
    cache(3, native("server")), cache(4, native("webui-server")),
    cache(5, native("desktop", "macos-15-ARM64")), cache(6, sealed), cache(7, sealed),
    cache(8, native(), { ref: "refs/pull/1/merge" }), cache(9, native(), { ref: "refs/heads/topic" })];
  const plan = planCachePrune(entries.reverse());
  assert.deepEqual(plan.remove.map((entry) => entry.id).sort(), [1, 6]);
  assert.equal(plan.beforeBytes, 900);
  assert.equal(plan.afterBytes, 700);
  assert.equal(CACHE_BUDGET_BYTES, 10_000_000_000);
});

test("keeps single snapshots, unknown cache formats and ordinary dependency caches", () => {
  const entries = [cache(1), cache(2, "Linux-pnpm"), cache(3, "v0-rust-build"),
    cache(4, "distribution-native-v1-unknown-X64-desktop"), cache(5, "distribution-native-v2-ubuntu-24.04-X64-desktop")];
  assert.deepEqual(planCachePrune(entries).remove, []);
});

test("successful immutable tag builds can retire only their explicitly selected tag caches", () => {
  const tag = "refs/tags/test-abc1234";
  const entries = [cache(1), cache(2, native(), { ref: tag }), cache(3, "Linux-pnpm", { ref: tag }),
    cache(4, native(), { ref: "refs/tags/test-other" })];
  assert.deepEqual(planCachePrune(entries, { retireTag: tag }).remove.map((entry) => entry.id).sort(), [2, 3]);
  for (const retireTag of ["refs/heads/main", "refs/tags/test-", "refs/tags/v1", "refs/tags/../../main"]) {
    assert.throws(() => planCachePrune(entries, { retireTag }), /tag/);
  }
});

test("breaks equal creation times deterministically and fails closed on malformed API inputs", () => {
  assert.deepEqual(planCachePrune([cache(1), cache(2, native(), { created_at: cache(1).created_at })]).remove.map((entry) => entry.id), [1]);
  for (const entries of [null, [cache(1), cache(1)], [cache(1, native(), { size_in_bytes: -1 })],
    [cache(1, native(), { created_at: "broken" })], [cache(1, native(), { ref: "" })],
    [cache(1, native(), { created_at: "0" })], [cache(1, native(), { created_at: "2026-02-30T00:00:00Z" })]]) {
    assert.throws(() => planCachePrune(entries), /cache/i);
  }
});

test("orders GitHub sub-millisecond timestamps without losing precision", () => {
  const entries = [cache(2, native(), { created_at: "2026-10-01T00:00:00.123001Z" }),
    cache(1, native(), { created_at: "2026-10-01T00:00:00.123002Z" })];
  assert.deepEqual(planCachePrune(entries).remove.map((entry) => entry.id), [2]);
});

function fakeGithub(entries, { active = [], releases = [], onDelete, onList } = {}) {
  const calls = [];
  let listCount = 0;
  const gh = async (args) => {
    calls.push(args);
    if (args[0] === "cache") {
      const id = Number(args[2]);
      if (onDelete) onDelete(id);
      entries = entries.filter((entry) => entry.id !== id);
      return "deleted";
    }
    if (args.at(-1).includes("/caches?")) {
      listCount += 1;
      if (onList) entries = onList(entries, listCount);
      return JSON.stringify([{ actions_caches: entries.slice(0, 1) }, { actions_caches: entries.slice(1) }]);
    }
    if (args.at(-1).includes("/releases?")) return JSON.stringify([releases]);
    return JSON.stringify([{ workflow_runs: active }]);
  };
  return { gh, calls };
}

test("defaults to a paginated dry run and never deletes without --apply", async () => {
  const fake = fakeGithub([cache(1), cache(2)]);
  const result = await main(["--repo", repository], { gh: fake.gh, env: {}, log: () => {} });
  assert.equal(result.remove.length, 1);
  assert.equal(fake.calls.length, 1);
  assert.ok(fake.calls[0].includes("--paginate"));
  assert.ok(fake.calls[0].includes("--slurp"));
});

test("later cleanup retires an earlier published tag deferred by concurrent runs, without deleting failed or foreign tags", async () => {
  const tag = "refs/tags/test-older";
  const entries = [cache(1), cache(2, native(), { ref: tag }), cache(3, native(), { ref: "refs/tags/test-failed" }),
    cache(4, native(), { ref: "refs/tags/test-foreign" })];
  const releases = [{ tag_name: "test-older", draft: false, assets: [{ name: "release-source.json" }] },
    { tag_name: "test-failed", draft: true, assets: [{ name: "release-source.json" }] },
    { tag_name: "test-foreign", draft: false, assets: [] }];
  const active = [{ id: 42, status: "in_progress" }];
  const fake = fakeGithub(entries, { active, releases });
  const options = { gh: fake.gh, env: { STOCK_PRUNE_RELEASE_CACHES: "true" }, log: () => {} };
  assert.equal((await main(["--repo", repository, "--apply"], options)).deferred, true);
  active.length = 0;
  const result = await main(["--repo", repository, "--apply"], options);
  assert.equal(result.beforeBytes, 300);
  assert.deepEqual(fake.calls.filter((call) => call[0] === "cache").map((call) => call[2]), ["2"]);
});

test("apply rechecks snapshots and verifies the actual remaining size", async () => {
  const fake = fakeGithub([cache(1), cache(2), cache(3, sealed)]);
  const result = await main(["--repo", repository, "--apply"], { gh: fake.gh, env: {}, log: () => {} });
  assert.equal(result.beforeBytes, 200);
  assert.deepEqual(fake.calls.filter((args) => args[0] === "cache"), [["cache", "delete", "1", "--repo", repository]]);
});

test("tag cache retirement needs successful publication evidence, is dry-run by default and respects active runs", async () => {
  const tag = "refs/tags/test-abc1234";
  const env = { GITHUB_REF: tag, STOCK_RELEASE_PUBLISHED: "true" };
  const args = ["--repo", repository, "--apply", "--retire-tag", tag];
  const fake = fakeGithub([cache(1), cache(2, native(), { ref: tag })]);
  await assert.rejects(main(args, { gh: fake.gh, env: {}, log: () => {} }), /successfully published/);
  assert.equal(fake.calls.length, 0);
  const dry = await main(["--repo", repository, "--retire-tag", tag], { gh: fake.gh, env, log: () => {} });
  assert.equal(dry.remove.length, 1);
  assert.equal(fake.calls.filter((entry) => entry[0] === "cache").length, 0);
  const result = await main(args, { gh: fake.gh, env, log: () => {} });
  assert.equal(result.beforeBytes, 100);
  assert.deepEqual(fake.calls.filter((entry) => entry[0] === "cache").map((entry) => entry[2]), ["2"]);
  const active = fakeGithub([cache(3, native(), { ref: tag })], { active: [{ id: 42, status: "in_progress" }] });
  assert.equal((await main(args, { gh: active.gh, env, log: () => {} })).deferred, true);
});

test("does not delete the last snapshot when a newer one disappeared before deletion", async () => {
  const fake = fakeGithub([cache(1), cache(2)], { onList: (entries, count) => count === 2 ? [cache(1)] : entries });
  await main(["--repo", repository, "--apply"], { gh: fake.gh, env: {}, log: () => {} });
  assert.equal(fake.calls.filter((args) => args[0] === "cache").length, 0);
});

test("defers for active builds, including queued and waiting runs, but ignores its own completed-job cleanup", async () => {
  for (const status of ["queued", "in_progress", "waiting", "pending", "requested"]) {
    const fake = fakeGithub([cache(1), cache(2)], { active: [{ id: 42, status }] });
    const logs = [];
    const result = await main(["--repo", repository, "--apply"], { gh: fake.gh, env: {}, log: (line) => logs.push(line) });
    assert.equal(result.deferred, true);
    assert.match(logs.join("\n"), /deferred.*42/);
    assert.equal(fake.calls.filter((args) => args[0] === "cache").length, 0);
  }
  const fake = fakeGithub([cache(1), cache(2)], { active: [{ id: 42, status: "in_progress" }] });
  await main(["--repo", repository, "--apply"], { gh: fake.gh, env: { GITHUB_RUN_ID: "42" }, log: () => {} });
  assert.equal(fake.calls.filter((args) => args[0] === "cache").length, 1);
});

test("reports over-budget protected caches and propagates API/delete failures", async () => {
  const huge = fakeGithub([cache(1, native(), { size_in_bytes: CACHE_BUDGET_BYTES + 1 })]);
  await assert.rejects(main(["--repo", repository, "--apply"], { gh: huge.gh, env: {}, log: () => {} }), /exceeds.*10000000000/);
  const failure = new Error("GitHub unavailable");
  await assert.rejects(main(["--repo", repository, "--apply"], { gh: async () => { throw failure; }, env: {}, log: () => {} }), /GitHub unavailable/);
  const fake = fakeGithub([cache(1), cache(2)], { onDelete: () => { throw failure; } });
  await assert.rejects(main(["--repo", repository, "--apply"], { gh: fake.gh, env: {}, log: () => {} }), /GitHub unavailable/);
  await assert.rejects(main(["--repo", repository], { gh: async () => '[{}]', env: {}, log: () => {} }), /actions_caches/);
  await assert.rejects(main(["--repo", "bad/../repo", "--apply"], { gh: fake.gh }), /usage/);
});

test("accepts exactly 10 GB and stops when another build starts between deletions", async () => {
  const exact = fakeGithub([cache(1, native(), { size_in_bytes: CACHE_BUDGET_BYTES })]);
  const result = await main(["--repo", repository, "--apply"], { gh: exact.gh, env: {}, log: () => {} });
  assert.equal(result.beforeBytes, CACHE_BUDGET_BYTES);
  const active = [];
  const fake = fakeGithub([cache(1), cache(2), cache(3)], {
    active, onDelete: () => active.push({ id: 42, status: "queued" }),
  });
  const deferred = await main(["--repo", repository, "--apply"], { gh: fake.gh, env: {}, log: () => {} });
  assert.equal(deferred.deferred, true);
  assert.equal(fake.calls.filter((args) => args[0] === "cache").length, 1);
});

test("rejects malformed active-run responses and unexpected flags before deletion", async () => {
  const fake = fakeGithub([cache(1), cache(2)]);
  const gh = (args) => args.at(-1).includes("/runs?")
    ? JSON.stringify([{ workflow_runs: [{ id: "invalid", status: "queued" }] }]) : fake.gh(args);
  await assert.rejects(main(["--repo", repository, "--apply"], { gh, env: {}, log: () => {} }), /Invalid active/);
  assert.equal(fake.calls.filter((args) => args[0] === "cache").length, 0);
  for (const args of [[], ["--repo", repository, "--delete-all"], ["--repo", repository, "--apply", "extra"]]) {
    await assert.rejects(main(args, { gh: fake.gh }), /usage/);
  }
  await assert.rejects(main(["--repo", repository, "--apply"], { gh: fake.gh, env: { GITHUB_RUN_ID: "bad" } }), /GITHUB_RUN_ID/);
});

test("rechecks replacement existence after the run query, immediately before deletion", async () => {
  const fake = fakeGithub([cache(1), cache(2)], { onList: (entries, count) => count >= 3 ? [cache(1)] : entries });
  const result = await main(["--repo", repository, "--apply"], { gh: fake.gh, env: {}, log: () => {} });
  assert.equal(result.beforeBytes, 100);
  assert.equal(fake.calls.filter((args) => args[0] === "cache").length, 0);
});

test("queries all run statuses together to avoid missing queued-to-running transitions", async () => {
  const fake = fakeGithub([cache(1), cache(2)], { active: [{ id: 42, status: "in_progress" }, { id: 43, status: "completed" }] });
  const result = await main(["--repo", repository, "--apply"], { gh: fake.gh, env: {}, log: () => {} });
  assert.equal(result.deferred, true);
  const queries = fake.calls.filter((args) => args.at(-1).includes("/runs?"));
  assert.equal(queries.length, 1);
  assert.equal(queries[0].at(-1), `repos/${repository}/actions/runs?per_page=100`);
  assert.equal(fake.calls.filter((args) => args[0] === "cache").length, 0);
  const completed = fakeGithub([cache(1), cache(2)], { active: [{ id: 43, status: "completed" }] });
  await main(["--repo", repository, "--apply"], { gh: completed.gh, env: {}, log: () => {} });
  assert.equal(completed.calls.filter((args) => args[0] === "cache").length, 1);
});
