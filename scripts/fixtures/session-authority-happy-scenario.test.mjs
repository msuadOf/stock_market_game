import assert from "node:assert/strict";
import { it } from "node:test";
import { observeAuthorityPort } from "./session-authority-happy-scenario.mjs";

it("真实端口观测器保留提交与恢复事实，不把存档候选假装 durable", { timeout: 10000 }, async () => {
  const calls = [];
  const port = {
    state: async () => ({ tick: 3, seq: 7, civilDate: "2030-01-02" }),
    step: async () => ({ TickBatch: { frames: [{ tick: 4, seq_from: 7, seq_to: 9 }] } }),
    civil: async () => ({ CivilUpdate: { tick: 4, seq_from: 9, seq_to: 11, civil_date: "2030-01-03" } }),
    save: async (key) => { calls.push(key); return { snapshot: { tick: 4 } }; },
    restore: async (slot) => { calls.push(slot); },
    submit: async (intent) => { calls.push(intent); },
  };
  const observed = observeAuthorityPort(port);
  assert.deepEqual(await observed.host.state(), await port.state());
  await observed.host.step();
  await observed.host.civil();
  await observed.host.save({ seq: 11, settledDate: "2030-01-02" });
  await observed.host.restore({ saved: true });
  const report = observed.evidence();
  assert.equal(report.coverage, "observable-happy-path-only");
  assert.equal(report.rows[1].tick, "4");
  assert.equal(report.rows[1].seq, "9");
  assert.equal(report.rows[3].kind, "sealed-candidate");
  assert.ok(!Object.hasOwn(report.rows[3], "durable"));
  assert.ok(report.unavailable.includes("archive.durable"));
  assert.deepEqual(calls, [{ seq: 11, settledDate: "2030-01-02" }, { saved: true }]);
});

it("端口错误保留原错并记录失败，不生成成功trace", { timeout: 10000 }, async () => {
  const failure = new Error("真实端点拒绝");
  const port = observeAuthorityPort({ step: async () => { throw failure; } });
  await assert.rejects(port.host.step(), (error) => error === failure);
  assert.deepEqual(port.evidence().rows, [{ kind: "operation-failed", operation: "step", message: failure.message }]);
});
