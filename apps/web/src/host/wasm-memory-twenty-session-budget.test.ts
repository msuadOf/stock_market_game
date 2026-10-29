import assert from "node:assert/strict";
import test from "node:test";
import { DEFAULT_SETUP } from "../config/defaults.ts";
import { WasmDocumentMemoryOwner } from "./wasm-document-memory-owner.ts";
import type { WasmFinalRelease } from "./wasm-document-memory-owner.ts";
import { WASM_MEMORY_MAXIMUM_BYTES, WASM_MEMORY_RECLAIM_BYTES } from "./wasm-memory-budget.ts";
import { WASM_PRODUCT_MEMORY_CAPACITY, WASM_PRODUCT_MEMORY_CAPACITY_BYTES } from "./wasm-memory-capacity.ts";
import { WasmMemoryStorage } from "./wasm-memory-storage.ts";
import { CHAIN_KEY, fixture, MiB, SNAPSHOT_KEY } from "./wasm-memory-storage.test-support.ts";

const FINAL_BYTES = 82_509_824;
const RUNS = 27;
const TWENTY_FINAL_BYTES = FINAL_BYTES * 20;
const TOTAL_FINAL_BYTES = FINAL_BYTES * RUNS;
const intent = { kind: "new" as const, setup: structuredClone(DEFAULT_SETUP), seed: "42" };

function harness() {
  const f = fixture({ maximumBytes: 3072 * MiB, reclaimBytes: 512 * MiB, liveUpperBoundBytes: 1024 * MiB });
  const document = () => new WasmDocumentMemoryOwner({ maximumBytes: WASM_MEMORY_MAXIMUM_BYTES,
    reclaimBytes: WASM_MEMORY_RECLAIM_BYTES,
    readCapacity: () => WASM_PRODUCT_MEMORY_CAPACITY,
    createStorage: (liveUpperBoundBytes) => new WasmMemoryStorage({ ...f.options, liveUpperBoundBytes }),
    randomNonce: () => "1234567890abcdef1234567890abcdef" });
  const anchor = () => JSON.parse(f.session.data.get(SNAPSHOT_KEY)!);
  return { f, document, anchor };
}

function released(finalBytes: number): WasmFinalRelease {
  return { kind: "released", finalBytes, growthStopped: true, referencesDropped: true };
}

test("20次产品预算候选闭环并延伸至27次耗尽，阈值reload保留原intent与账且不额外计入C", { timeout: 10000 }, async () => {
  const h = harness();
  assert.equal(WASM_MEMORY_MAXIMUM_BYTES, 3072 * MiB);
  assert.equal(WASM_PRODUCT_MEMORY_CAPACITY_BYTES, 1024 * MiB);
  assert.equal(WASM_MEMORY_RECLAIM_BYTES, 512 * MiB);
  assert.ok(WASM_PRODUCT_MEMORY_CAPACITY_BYTES <= WASM_MEMORY_MAXIMUM_BYTES - WASM_MEMORY_RECLAIM_BYTES);
  assert.ok(2 * WASM_PRODUCT_MEMORY_CAPACITY_BYTES <= WASM_MEMORY_MAXIMUM_BYTES);

  let owner = h.document();
  let finished = 0;
  let sessionChain: string | null = null;
  const reloadAfter = new Set([7, 14, 21]);
  for (let index = 0; index < RUNS; index += 1) {
    const result = owner.admit(intent);
    if (reloadAfter.has(finished)) {
      assert.equal(result.kind, "reload", `第${finished}次完成后应在下一分配前reload`);
      if (result.kind !== "reload") throw new Error("预期阈值reload");
      if (result.handover.kind !== "frozen") throw new Error("预期冻结handover");
      if (result.handover.pendingIntent === null) throw new Error("reload handover必须保留原intent");
      assert.equal(result.handover.pendingIntent.kind, "new");
      assert.deepEqual(result.handover.pendingIntent, intent);
      assert.equal(result.handover.carriedResidualBytes, FINAL_BYTES * finished);
      owner = h.document();
      const pending = owner.pendingHandover();
      assert.equal(pending?.kind, "frozen");
      if (pending?.kind !== "frozen") throw new Error("预期冻结handover");
      assert.deepEqual(pending.pendingIntent, intent);
      assert.equal(h.anchor().carryBase + h.anchor().sessionResident, FINAL_BYTES * finished);
      const confirmed = owner.admit(intent, { confirmedHandover: true });
      assert.equal(confirmed.kind, "admitted");
      if (confirmed.kind !== "admitted") throw new Error("确认原intent后应准入");
      confirmed.ticket.registerRelease(Promise.resolve(released(FINAL_BYTES)));
      owner.recordCommittedBoundary(confirmed.ticket, { tick: finished + 1, day: 1 });
      await owner.finish(confirmed.ticket);
      await owner.finish(confirmed.ticket);
      finished += 1;
      assert.equal(h.anchor().carryBase + h.anchor().sessionResident, FINAL_BYTES * finished);
      assert.equal(h.f.session.data.get(CHAIN_KEY), sessionChain);
      continue;
    }

    assert.equal(result.kind, "admitted", `第${finished + 1}次应准入`);
    if (result.kind !== "admitted") throw new Error("预期admitted");
    result.ticket.registerRelease(Promise.resolve(released(FINAL_BYTES)));
    owner.recordCommittedBoundary(result.ticket, { tick: finished + 1, day: 1 });
    if (finished + 1 < RUNS) {
      await owner.finish(result.ticket);
      await owner.finish(result.ticket);
    } else {
      await assert.rejects(owner.finish(result.ticket), (error: unknown) =>
        (error as { code: string }).code === "MEMORY_RESIDUAL_EXHAUSTED");
      await assert.rejects(owner.finish(result.ticket), (error: unknown) =>
        (error as { code: string }).code === "MEMORY_RESIDUAL_EXHAUSTED");
    }
    finished += 1;
    assert.equal(h.anchor().carryBase + h.anchor().sessionResident, FINAL_BYTES * finished);
    if (sessionChain === null) {
      const chain = h.f.session.data.get(CHAIN_KEY);
      if (chain === null || chain === undefined) throw new Error("首个finish后必须保留session chain");
      sessionChain = chain;
    }
    if (finished === 20) {
      assert.equal(TWENTY_FINAL_BYTES, 1_650_196_480);
      assert.equal(h.anchor().carryBase + h.anchor().sessionResident + WASM_PRODUCT_MEMORY_CAPACITY_BYTES, 2597.75 * MiB);
      assert.ok(h.anchor().carryBase + h.anchor().sessionResident + WASM_PRODUCT_MEMORY_CAPACITY_BYTES <= WASM_MEMORY_MAXIMUM_BYTES);
    }
  }

  assert.equal(finished, RUNS);
  assert.equal(h.anchor().carryBase + h.anchor().sessionResident, TOTAL_FINAL_BYTES);
  assert.equal(h.anchor().liveUpperBound, 0);
  assert.throws(() => owner.admit(intent), (error: unknown) =>
    (error as { code: string }).code === "MEMORY_RESIDUAL_EXHAUSTED");
  assert.equal(h.anchor().liveUpperBound, 0);
  assert.equal(h.f.session.data.get(CHAIN_KEY), sessionChain);

  const boundary = harness();
  boundary.f.existing((await import("./wasm-memory-storage.test-support.ts")).snapshot(2048 * MiB));
  const boundaryOwner = boundary.document(), last = boundaryOwner.admit(intent);
  assert.equal(last.kind, "admitted");
  if (last.kind !== "admitted") throw new Error("B-C边界应可准入");
  last.ticket.registerRelease(Promise.resolve(released(FINAL_BYTES)));
  boundaryOwner.recordCommittedBoundary(last.ticket, { tick: 27, day: 1 });
  await assert.rejects(boundaryOwner.finish(last.ticket), (error: unknown) =>
    (error as { code: string }).code === "MEMORY_RESIDUAL_EXHAUSTED");
  assert.throws(() => boundaryOwner.admit(intent), (error: unknown) =>
    (error as { code: string }).code === "MEMORY_RESIDUAL_EXHAUSTED");
});

test("产品预算的B-C边界恰等可准入且加一拒绝，保留大于2GiB的继承账", { timeout: 10000 }, async () => {
  const { f, document, anchor } = harness();
  f.existing((await import("./wasm-memory-storage.test-support.ts")).snapshot(2048 * MiB));
  const exact = document();
  assert.equal(exact.admit(intent).kind, "admitted");
  assert.equal(anchor().carryBase + anchor().sessionResident, 2048 * MiB);

  const above = harness();
  above.f.existing((await import("./wasm-memory-storage.test-support.ts")).snapshot(2048 * MiB + 1));
  assert.throws(() => above.document().admit(intent), (error: unknown) =>
    (error as { code: string }).code === "MEMORY_RESIDUAL_EXHAUSTED");
  assert.equal(above.anchor().carryBase + above.anchor().sessionResident, 2048 * MiB + 1);

  const large = harness();
  large.f.existing((await import("./wasm-memory-storage.test-support.ts")).snapshot(2200 * MiB));
  assert.throws(() => large.document().admit(intent), (error: unknown) =>
    (error as { code: string }).code === "MEMORY_RESIDUAL_EXHAUSTED");
  assert.equal(large.anchor().carryBase + large.anchor().sessionResident, 2200 * MiB);
  assert.ok(large.anchor().carryBase > 2 ** 31);
});

test("活动F5逐次继承live上界且未知release保留原cause并锁住live上界", { timeout: 10000 }, async () => {
  const h = harness();
  let owner = h.document();
  for (let count = 1; count <= 3; count += 1) {
    const admitted = owner.admit(intent);
    assert.equal(admitted.kind, "admitted", `第${count}次活动F5前应可分配`);
    assert.equal(h.anchor().carryBase + h.anchor().sessionResident + h.anchor().liveUpperBound, count * 1024 * MiB);
    owner = h.document();
  }
  assert.throws(() => owner.admit(intent), (error: unknown) =>
    (error as { code: string }).code === "MEMORY_RESIDUAL_EXHAUSTED");
  assert.equal(h.anchor().carryBase + h.anchor().sessionResident + h.anchor().liveUpperBound, 3072 * MiB);

  const unknown = harness(), lockedOwner = unknown.document(), ticketResult = lockedOwner.admit(intent);
  assert.equal(ticketResult.kind, "admitted");
  if (ticketResult.kind !== "admitted") throw new Error("预期admitted");
  const cause = new Error("release终值未知");
  ticketResult.ticket.registerRelease(Promise.reject(cause));
  await assert.rejects(lockedOwner.finish(ticketResult.ticket), (error) => error === cause);
  assert.equal(unknown.anchor().liveUpperBound, 1024 * MiB);
  assert.throws(() => lockedOwner.admit(intent), (error) => error === cause);
  assert.equal(unknown.anchor().liveUpperBound, 1024 * MiB);
});
