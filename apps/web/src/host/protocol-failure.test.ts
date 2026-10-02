import assert from "node:assert/strict";
import { test } from "node:test";

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
