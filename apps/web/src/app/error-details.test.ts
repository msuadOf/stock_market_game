import assert from "node:assert/strict";
import { test } from "node:test";

test("context.operation 仅展示两个真实宿主操作枚举，不展示任意字符串", async () => {
  const { buildErrorFeedback } = await import("./error-details.ts");
  for (const operation of ["step", "endCivilDay"]) {
    const text = buildErrorFeedback({ code: "STEP_FATAL", where: "host", message: "失败", context: { operation, tick: 3 } });
    assert.match(text, new RegExp(`"operation": "${operation}"`));
    assert.match(text, /"tick": 3/);
  }
  for (const operation of ["private-operation", "step secret=private", "123"]) {
    const text = buildErrorFeedback({ code: "STEP_FATAL", where: "host", message: "失败", context: { operation } });
    assert.doesNotMatch(text, new RegExp(`"operation": "${operation}"`));
    assert.match(text, /已脱敏/);
  }
  for (const operation of [123, ["step"], { value: "endCivilDay" }]) {
    const text = buildErrorFeedback({ code: "STEP_FATAL", where: "host", message: "失败", context: { operation } });
    assert.match(text, /"operation": "\[已脱敏：非公开诊断值\]"/);
  }
});

test("构建反馈是纯函数，不捏造未知字段，展示原始 code/where/message", async () => {
  const { buildErrorFeedback } = await import("./error-details.ts");
  const failure = { code: "STEP_FATAL", where: "engine.commit_tick", message: "校验失败" };
  const text = buildErrorFeedback(failure);
  assert.equal(text, buildErrorFeedback(failure));
  assert.match(text, /错误码：STEP_FATAL/);
  assert.match(text, /位置：engine\.commit_tick/);
  assert.match(text, /错误：校验失败/);
  assert.match(text, /原因链：未提供/);
  assert.match(text, /上下文：未提供/);
  assert.match(text, /恢复能力：未知/);
  assert.match(buildErrorFeedback("原始可见错误"), /错误：原始可见错误/);
});

test("循环 context/cause、未知原因与无法访问的详情不会导致错误视图再次崩溃", async () => {
  const { buildErrorFeedback } = await import("./error-details.ts");
  const context: Record<string, unknown> = { tick: 12 };
  context.nested = context;
  const cause: Record<string, unknown> = { message: "真实原因", code: "INNER" };
  cause.cause = cause;
  const base = { code: "STEP_FATAL", where: "engine.step", message: "失败" };
  const text = buildErrorFeedback({ ...base, context, cause });
  assert.match(text, /循环引用/);
  assert.match(text, /真实原因/);
  const unreadable = new Proxy({}, { ownKeys: () => { throw new Error("details access denied"); } });
  assert.match(buildErrorFeedback({ ...base, context: unreadable }), /详情读取失败.*details access denied/);
  assert.match(buildErrorFeedback({ ...base, cause: null, context: null, recoverable: null }), /原因链：未提供/);
  assert.match(buildErrorFeedback({ ...base, cause: 17 }), /17/);
});

test("反馈详情剔除凭据、私人字段、完整存档和调用栈，保留公开诊断字段及原因链", async () => {
  const { buildErrorFeedback } = await import("./error-details.ts");
  const text = buildErrorFeedback({
    code: "HOST_FATAL",
    where: "host.response",
    message: "请求失败",
    context: {
      tick: 12,
      nested: { requestId: 4, seq: 6, authorization: "private-auth" },
      email: "private@example.com",
      sessionId: "private-session",
      saveSlot: { tick: 13, accounts: "private-accounts" },
      arbitraryString: "private-unknown-value",
    },
    cause: { message: "Bearer private-bearer https://user:private-pass@example.com/api?token=private-query#private-fragment", context: { cookie: "private-cookie" } },
    recoverable: true,
    recoveryActions: ["检查服务连接"],
  });
  assert.match(text, /requestId/);
  assert.match(text, /seq/);
  assert.match(text, /12/);
  assert.match(text, /宿主声明可恢复/);
  assert.match(text, /检查服务连接/);
  assert.match(text, /已脱敏/);
  assert.doesNotMatch(text, /private-|private@/);
});

test("未知 context 的动态键也脱敏，避免身份或凭据作为映射键泄露", async () => {
  const { buildErrorFeedback } = await import("./error-details.ts");
  const text = buildErrorFeedback({
    code: "HOST_FATAL", where: "host.response", message: "失败",
    context: { tick: 12, "private-token-as-key": { tick: 12 }, "private-user-as-key": 17 },
    cause: { message: "真实原因", "private-cause-as-key": "隐藏内容" },
  });
  assert.match(text, /12/);
  assert.match(text, /真实原因/);
  assert.match(text, /已脱敏/);
  assert.doesNotMatch(text, /private-token-as-key|private-user-as-key|private-cause-as-key/);
});

test("公开 context 键不会放行任意身份或凭据字符串", async () => {
  const { buildErrorFeedback, copyErrorFeedback } = await import("./error-details.ts");
  const failure = { code: "HOST_FATAL", where: "host.response", message: "失败", context: {
    actual: ["acct-123456", "private-session", { seq: "private-credential" }],
    expected: "private-save-content", requestId: "private-request-id", tick: 12,
    civilDate: "2030-01-02", phase: "Continuous",
  } };
  const text = buildErrorFeedback(failure);
  assert.doesNotMatch(text, /acct-123456|private-/);
  assert.match(text, /2030-01-02/);
  assert.match(text, /Continuous/);
  assert.match(text, /12/);
  let copied = "";
  await copyErrorFeedback(failure, () => {}, { writeText: async (value) => { copied = value; } });
  assert.equal(copied, text);
});

test("错误反馈使用真实交易协议阶段，不将虚构阶段标为公开", async () => {
  const { buildErrorFeedback } = await import("./error-details.ts");
  for (const phase of ["CallAuction", "PreOpen", "Continuous", "ClosingAuction"]) {
    assert.match(buildErrorFeedback({ code: "HOST_FATAL", where: "host", message: "失败", context: { phase } }), new RegExp(phase));
  }
  assert.doesNotMatch(buildErrorFeedback({ code: "HOST_FATAL", where: "host", message: "失败", context: { phase: "OpeningAuction" } }), /OpeningAuction/);
});

test("未注入 Clipboard 时使用浏览器接口，复制成功前不宣布成功", async () => {
  const { copyErrorFeedback } = await import("./error-details.ts");
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, "navigator");
  const copied: string[] = [];
  const statuses: string[] = [];
  let finishCopy: () => void = () => { throw new Error("Clipboard 尚未调用"); };
  Object.defineProperty(globalThis, "navigator", {
    configurable: true,
    value: { clipboard: { writeText: (text: string) => {
      copied.push(text);
      return new Promise<void>((resolve) => { finishCopy = resolve; });
    } } },
  });
  try {
    const copying = copyErrorFeedback("浏览器错误", (status) => statuses.push(status.kind));
    assert.deepEqual(statuses, ["copying"]);
    assert.equal(copied.length, 1);
    assert.match(copied[0]!, /浏览器错误/);
    finishCopy();
    await copying;
    assert.deepEqual(statuses, ["copying", "copied"]);
  } finally {
    if (descriptor === undefined) Reflect.deleteProperty(globalThis, "navigator");
    else Object.defineProperty(globalThis, "navigator", descriptor);
  }
});
