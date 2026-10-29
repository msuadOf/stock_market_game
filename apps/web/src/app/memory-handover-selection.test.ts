import assert from "node:assert/strict";
import test from "node:test";
import { createMemoryHandoverSelection } from "./memory-handover-selection.ts";
import { commandDayEndArchiveFixture } from "./command-host-test-fixture.ts";
import type { MemoryHandoverPanelProps } from "./MemoryHandoverPanel.tsx";
import type { WasmMemoryHandover } from "../host/wasm-memory-handover.ts";
import { DEFAULT_SETUP } from "../config/defaults.ts";
const x = commandDayEndArchiveFixture(), y = { ...x, seed: "99" };
const frozen: WasmMemoryHandover = { kind: "frozen", code: "MEMORY_RESIDUAL_RELOAD_HANDOVER", residualBytes: 512, carriedResidualBytes: 512,
  thresholdEffective: 512, reloadCount: 1, pendingIntent: { kind: "load", archive: x }, last_committed_tick: x.snapshot.tick, day: 0, handoverNonce: "a".repeat(32) };
function fixture(loadFromFile: () => Promise<ReturnType<typeof commandDayEndArchiveFixture> | null>, createNewIntent = () => ({ kind: "new" as const, setup: DEFAULT_SETUP, seed: "123" })) {
  let panel: MemoryHandoverPanelProps | null = null;
  const choice = createMemoryHandoverSelection({ loadFromFile, createNewIntent, onChange: (value: MemoryHandoverPanelProps | null) => { panel = value; } });
  return { choice, panel: () => { assert.ok(panel); return panel; }, current: () => panel };
}
test("确认面板默认冻结X，严格文件Y只替换所选目标，确认才交出Y且不改X", { timeout: 10000 }, async () => {
  const f = fixture(async () => y), pending = f.choice.request(frozen);
  assert.deepEqual(f.panel().selectedIntent, frozen.pendingIntent);f.panel().onSelectFile();
  await new Promise<void>(resolve => setImmediate(resolve));
  const selected = f.panel().selectedIntent;
  assert.equal(selected?.kind, "load");
  if (selected?.kind === "load") assert.equal(selected.archive.seed, "99");
  assert.equal(f.panel().handover.kind, "frozen");assert.deepEqual(f.panel().handover, frozen);
  f.panel().onContinue();assert.deepEqual(await pending, { kind: "load", archive: y });assert.equal(f.current(), null);
});
test("文件选择器取消留原确认面板与X；面板取消返回null且不报错误", { timeout: 10000 }, async () => {
  const f = fixture(async () => null), pending = f.choice.request(frozen);
  f.panel().onSelectFile();await new Promise<void>(resolve => setImmediate(resolve));
  assert.equal(f.panel().error, null);assert.deepEqual(f.panel().selectedIntent, frozen.pendingIntent);
  f.panel().onCancel();assert.equal(await pending, null);assert.equal(f.current(), null);
});
test("文件坏档显式展示原因，保原X/不自动确认；explicit Y优先且不加载quickslot", { timeout: 10000 }, async () => {
  const f = fixture(async () => ({ ...y, snapshot: { ...y.snapshot, tick: -1 } })), pending = f.choice.request(frozen, { kind: "load", archive: y });
  f.panel().onSelectFile();await new Promise<void>(resolve => setImmediate(resolve));
  assert.match(f.panel().error!, /日终存档选择失败/);assert.equal(f.panel().selectedIntent?.kind, "load");
  f.panel().onContinue();assert.deepEqual(await pending, { kind: "load", archive: y });
});
test("effect结束时解除待确认Promise，晚到文件响应不能改新状态或复活旧选择", { timeout: 10000 }, async () => {
  let finish!: (value: typeof y) => void;const file = new Promise<typeof y>(resolve => { finish = resolve; });
  const f = fixture(() => file), pending = f.choice.request(frozen);f.panel().onSelectFile();assert.equal(f.panel().busy, true);
  f.choice.dispose();assert.equal(await pending, null);finish(y);await new Promise<void>(resolve => setImmediate(resolve));
  assert.equal(f.current(), null);
});

test("lost与空目标均可明确选当前配置新局，只有继续才交出new；取消不进局", { timeout: 10000 }, async () => {
  for (const handover of [{ kind: "lost" as const, reason: "carrier-unavailable" as const }, { ...frozen, pendingIntent: null }]) {
    let seeds = 0;
    const f = fixture(async () => { throw new Error("新局不得读取存档"); }, () => { seeds++; return { kind: "new", setup: DEFAULT_SETUP, seed: "123" }; });
    let resolved = false;
    const pending = f.choice.request(handover).then(value => { resolved = true; return value; });
    assert.equal(seeds, 0); assert.equal(f.panel().selectedIntent, null);
    f.panel().onSelectNew(); await new Promise<void>(resolve => setImmediate(resolve));
    assert.equal(seeds, 1); assert.equal(resolved, false);
    assert.deepEqual(f.panel().selectedIntent, { kind: "new", setup: DEFAULT_SETUP, seed: "123" });
    f.panel().onContinue(); assert.deepEqual(await pending, { kind: "new", setup: DEFAULT_SETUP, seed: "123" });
    const cancelled = f.choice.request(handover); f.panel().onSelectNew(); f.panel().onCancel();
    assert.equal(await cancelled, null);
  }
});

test("新局seed生成失败显式展示且保留原选择，不自动确认", { timeout: 10000 }, async () => {
  const f = fixture(async () => null, () => { throw new Error("seed熵不可用"); });
  const pending = f.choice.request(frozen);
  f.panel().onSelectNew();
  assert.match(f.panel().error!, /新局选择失败：seed熵不可用/);
  assert.deepEqual(f.panel().selectedIntent, frozen.pendingIntent);
  f.panel().onCancel(); assert.equal(await pending, null);
});
