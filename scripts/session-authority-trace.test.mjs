import assert from "node:assert/strict";
import { it } from "node:test";
import { AUTHORITY_SCENARIO, compareAuthorityContractEvidence, compareAuthorityTraces, runAuthorityScenario, validateAuthorityTrace } from "./session-authority-trace.mjs";

// 这些数据只验证验收器本身；不是任何产品端的实跑证据。
function fixture(host = "native", execution = "real") {
  return {
    schema: 1, host, execution, sourceSha256: "a".repeat(64), artifactSha256: "b".repeat(64),
    scenario: AUTHORITY_SCENARIO.map(({ id }) => id),
    rows: AUTHORITY_SCENARIO.map(({ id }, index) => ({
      id, generation: "1", day: 0, tick: String(index), seq: String(index), paused: true,
      terminal: null, archive: { queued: [], durable: null, blocked: false },
      observer: { generation: "1", lease: "1", sent: "0", flushed: "0", accepted: "0" },
    })),
  };
}

it("统一 trace 不接受编译结果冒充真实三端运行", { timeout: 10000 }, () => {
  const traces = [fixture(), fixture("chromium-shared-wasm"), fixture("linux-tauri", "compiled-only")];
  assert.throws(() => compareAuthorityTraces(traces), /实际运行/);
  assert.throws(() => compareAuthorityTraces(traces.slice(0, 2)), /三端/);
  assert.throws(() => compareAuthorityTraces([fixture(), fixture(), fixture("linux-tauri")]), /重复/);
});

it("统一 trace 必须匹配源码、场景和完整语义状态", { timeout: 10000 }, () => {
  const traces = [fixture(), fixture("chromium-shared-wasm"), fixture("linux-tauri")];
  assert.equal(compareAuthorityTraces(traces).comparedRows, AUTHORITY_SCENARIO.length);
  traces[1].rows[2].archive.blocked = true;
  assert.throws(() => compareAuthorityTraces(traces), /语义不一致/);
  traces[1] = fixture("chromium-shared-wasm");
  traces[1].sourceSha256 = "c".repeat(64);
  assert.throws(() => compareAuthorityTraces(traces), /源码指纹/);
});

it("同代旧 lease 接纳、假 durable 和倒退游标不能进入合格 trace", { timeout: 10000 }, () => {
  const trace = fixture();
  trace.rows[1].observer.accepted = "1";
  assert.throws(() => validateAuthorityTrace(trace), /accepted/);
  trace.rows[1].observer.accepted = "0";
  trace.rows[1].archive.durable = { generation: "1", writerEpoch: "1", seq: "99" };
  assert.throws(() => validateAuthorityTrace(trace), /durable/);
  trace.rows[1].archive.durable = null;
  delete trace.rows[1].observer.lease;
  assert.throws(() => validateAuthorityTrace(trace), /lease/);
});

it("统一 workload 顺序由单一 scenario 驱动，样本必须来自每次命令完成后的 port", { timeout: 10000 }, async () => {
  const expected = fixture();
  const called = [];
  const trace = await runAuthorityScenario({
    host: expected.host, sourceSha256: expected.sourceSha256, artifactSha256: expected.artifactSha256,
    execute: async (command) => { called.push(command.id); },
    capture: async (id) => expected.rows.find((row) => row.id === id),
  });
  assert.deepEqual(called, AUTHORITY_SCENARIO.map(({ id }) => id));
  assert.equal(trace.execution, "real");
  assert.deepEqual(trace.rows, expected.rows);
});

it("真实挂接次数允许不同，但旧lease动作不能改变当前身份", { timeout: 10000 }, () => {
  const traces = [fixture(), fixture("chromium-shared-wasm"), fixture("linux-tauri")];
  for (const row of traces[1].rows) row.observer.lease = "41";
  assert.equal(compareAuthorityTraces(traces).comparedRows, 17);
  traces[1].rows[4].observer.lease = "42";
  assert.equal(compareAuthorityTraces(traces).comparedRows, 17);
  traces[1].rows[13].observer.lease = "99";
  assert.throws(() => compareAuthorityTraces(traces), /旧lease/);
});

it("共同合同投影保留宿主诊断，仍拒绝每个市场及归档字段漂移", { timeout: 10000 }, () => {
  const traces = [fixture(), fixture("chromium-shared-wasm"), fixture("linux-tauri")];
  traces[1].rows[2].civilReady = true;
  traces[1].rows[2].diagnostics = { receiver: "真实Worker" };
  traces[1].rows[2].archive.storageDiagnostics = { transport: "IndexedDB" };
  const original = structuredClone(traces);
  assert.equal(compareAuthorityTraces(traces).comparedRows, 17);
  assert.deepEqual(traces, original, "比较不能删除原始诊断证据");
  const mutations = [
    row => { row.generation = "2"; }, row => { row.day = 1; },
    row => { row.tick = "99"; }, row => { row.seq = "99"; },
    row => { row.paused = false; }, row => { row.terminal = "STEP_FATAL"; },
    row => { row.archive.blocked = true; },
    row => { row.archive.queued = [{ generation: "1", writerEpoch: "1", seq: "1" }]; },
    row => { row.archive.durable = { generation: "1", writerEpoch: "1", seq: "1" }; },
  ];
  for (const mutate of mutations) {
    const changed = structuredClone(original); mutate(changed[1].rows[2]);
    assert.throws(() => compareAuthorityTraces(changed), /语义不一致/);
  }
});


it("混合级别合同保留真实证据标签，不能升级为三端端到端", { timeout: 10000 }, () => {
  const traces = [fixture("native-actor-component", "component"), fixture("tauri-actor-component", "component"), fixture("chromium-shared-wasm")];
  const report = compareAuthorityContractEvidence(traces);
  assert.equal(report.level, "mixed-level/non-E2E");
  assert.equal(report.endToEnd, false);
  assert.equal(report.comparedRows, 17);
  assert.deepEqual(report.evidence, traces);
  assert.throws(() => compareAuthorityTraces(traces), /未知 trace 宿主/);
  const invalid = structuredClone(traces); invalid[0].execution = "real";
  assert.throws(() => compareAuthorityContractEvidence(invalid), /证据级别/);
  invalid[0].execution = "compiled-only";
  assert.throws(() => compareAuthorityContractEvidence(invalid), /证据级别/);
  assert.throws(() => compareAuthorityContractEvidence([traces[0], traces[0], traces[2]]), /重复/);
});

it("混合证据仍验证指纹、完整场景、观察隔离及共同字段", { timeout: 10000 }, () => {
  const traces = [fixture("native-actor-component", "component"), fixture("tauri-actor-component", "component"), fixture("chromium-shared-wasm")];
  for (const mutate of [
    trace => { trace.sourceSha256 = "c".repeat(64); },
    trace => { trace.artifactSha256 = "未知"; },
    trace => { trace.rows.pop(); },
    trace => { trace.rows[13].observer.lease = "2"; },
    trace => { trace.rows[2].archive.blocked = true; },
  ]) {
    const invalid = structuredClone(traces); mutate(invalid[1]);
    assert.throws(() => compareAuthorityContractEvidence(invalid));
  }
});
