import assert from "node:assert/strict";
import { test } from "node:test";
import { isObserverOnlyFailure } from "./protocol-failure.ts";

test("错误边界保留可选未知详情和真实位置，既有三字段不要求虚构字段", async () => {
  const { parseHostFailure } = await import("./protocol-failure.ts");
  const minimal = { code: "STEP_FATAL", where: "engine.step", message: "失败" };
  assert.deepEqual(parseHostFailure(minimal), minimal);
  const enriched = { ...minimal, cause: { kind: "opaque" }, context: { tick: 7 }, recoverable: false, recoveryActions: ["检查环境"] };
  assert.deepEqual(parseHostFailure(enriched), enriched);
  assert.deepEqual(parseHostFailure({ code: minimal.code, message: minimal.message }, "remote.HostFailure"), {
    code: minimal.code, message: minimal.message, where: "remote.HostFailure",
  });
  assert.deepEqual(parseHostFailure({ ...minimal, cause: null, context: null, recoverable: null, recoveryActions: null }), {
    ...minimal, cause: null, context: null, recoverable: null, recoveryActions: null,
  });
});

test("非法核心字段、恢复标志和动作在边界明确拒绝，不回填默认值", async () => {
  const { parseHostFailure } = await import("./protocol-failure.ts");
  const base = { code: "STEP_FATAL", where: "engine.step", message: "失败" };
  for (const value of [
    { ...base, code: "" }, { ...base, where: null }, { ...base, message: 17 },
    { ...base, recoverable: "yes" }, { ...base, recoveryActions: [17] },
    { ...base, recoveryActions: [""] }, { ...base, secret: "unexpected" },
  ]) assert.throws(() => parseHostFailure(value));
  assert.throws(() => parseHostFailure({ code: base.code, message: base.message }), /where/);
  assert.throws(() => parseHostFailure({ ...base, where: "" }, "remote.HostFailure"), /where/);
});


test("投递失败不等于市场暂停；带 recoverable 的日界失败仍由权威暂停", { timeout: 10000 }, () => {
  assert.equal(isObserverOnlyFailure({ code: "WASM_WORKER_PUBLICATION", where: "worker-host.publisher", message: "发送失败" }), true);
  assert.equal(isObserverOnlyFailure({ code: "OBSERVER_ACK_FAILED", where: "tauri-host.engine_ack_observer", message: "确认失败", recoverable: true }), true);
  assert.equal(isObserverOnlyFailure({ code: "REPORT_CORRECTION_PREPARE_REJECTED", where: "web-wasm.market_runner.end_civil", message: "更正失败", recoverable: true }), false);
  assert.equal(isObserverOnlyFailure({ code: "OBSERVER_ACK_FAILED", where: "server.actor", message: "其他错误" }), false);
  assert.equal(isObserverOnlyFailure("未知故障"), false);
});

test("三端同一归档故障只暂停权威，不因存储失败重连观察者", { timeout: 10000 }, async () => {
  const { isRecoverableAuthorityFailure } = await import("./protocol-failure.ts");
  const failure = { code: "DAY_END_ARCHIVE_FAILED", where: "session.authority.save_day_end", message: "保存失败", recoverable: true,
    context: { operation: "saveDayEnd", generation: "2", tick: 1, seq: 3, day: 1 } };
  assert.equal(isRecoverableAuthorityFailure(failure, "2"), true);
  assert.equal(isRecoverableAuthorityFailure(failure, "3"), false);
  assert.equal(isRecoverableAuthorityFailure({ ...failure, where: "socket" }, "2"), false);
  assert.equal(isRecoverableAuthorityFailure({ ...failure, recoverable: false }, "2"), false);
});
