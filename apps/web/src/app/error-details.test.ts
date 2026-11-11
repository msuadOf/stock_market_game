import assert from "node:assert/strict";
import { test } from "node:test";

test("context.operation 保留任意原始诊断值，不以展示层枚举掩盖协议问题", async () => {
  const { buildErrorFeedback } = await import("./error-details.ts");
  for (const operation of ["step", "endCivilDay", "restoreArchive", 123, ["step"], { value: "endCivilDay" }]) {
    const text = buildErrorFeedback({ code: "STEP_FATAL", where: "host", message: "失败", context: { operation, tick: 3 } });
    assert.ok(text.includes(JSON.stringify(operation, null, 2).split("\n")[0]!));
    assert.match(text, /"tick": 3/);
    assert.doesNotMatch(text, /已脱敏/);
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

test("反馈详情保留原始字段、完整存档片段及原因链", async () => {
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
  assert.doesNotMatch(text, /已脱敏/);
  for (const expected of ["private-auth", "private@example.com", "private-session", "private-accounts", "private-unknown-value", "private-bearer", "private-query", "private-cookie"]) assert.ok(text.includes(expected), expected);
});

test("未知 context 和 cause 的动态键原样展示", async () => {
  const { buildErrorFeedback } = await import("./error-details.ts");
  const text = buildErrorFeedback({
    code: "HOST_FATAL", where: "host.response", message: "失败",
    context: { tick: 12, "private-token-as-key": { tick: 12 }, "private-user-as-key": 17 },
    cause: { message: "真实原因", "private-cause-as-key": "隐藏内容" },
  });
  assert.match(text, /12/);
  assert.match(text, /真实原因/);
  assert.doesNotMatch(text, /已脱敏/);
  for (const key of ["private-token-as-key", "private-user-as-key", "private-cause-as-key"]) assert.ok(text.includes(key));
});

test("context 中的原始身份、输入字符串在显示与复制中一致", async () => {
  const { buildErrorFeedback, copyErrorFeedback } = await import("./error-details.ts");
  const failure = { code: "HOST_FATAL", where: "host.response", message: "失败", context: {
    actual: ["acct-123456", "private-session", { seq: "private-credential" }],
    expected: "private-save-content", requestId: "private-request-id", tick: 12,
    civilDate: "2030-01-02", phase: "Continuous",
  } };
  const text = buildErrorFeedback(failure);
  for (const value of ["acct-123456", "private-session", "private-credential", "private-save-content", "private-request-id"]) assert.ok(text.includes(value));
  assert.match(text, /2030-01-02/);
  assert.match(text, /Continuous/);
  assert.match(text, /12/);
  let copied = "";
  await copyErrorFeedback(failure, () => {}, { writeText: async (value) => { copied = value; } });
  assert.equal(copied, text);
});

test("错误反馈原样展示阶段值，保留协议不一致的证据", async () => {
  const { buildErrorFeedback } = await import("./error-details.ts");
  for (const phase of ["CallAuction", "PreOpen", "Continuous", "ClosingAuction"]) {
    assert.match(buildErrorFeedback({ code: "HOST_FATAL", where: "host", message: "失败", context: { phase } }), new RegExp(phase));
  }
  assert.match(buildErrorFeedback({ code: "HOST_FATAL", where: "host", message: "失败", context: { phase: "OpeningAuction" } }), /OpeningAuction/);
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

test("tombstone 反馈保留完整 Worker 诊断字段与原始消息", { timeout: 10000 }, async () => {
  const { buildErrorFeedback } = await import("./error-details.ts");
  const text = buildErrorFeedback({ code: "HOST_FATAL", where: "host", message: "失败", context: {
    incomplete: true,
    records: [
      { identity: { workerId: 7, slot: 2, gen: 9, address: 1234 }, tombstone: { state: "live", kind: "panic", phase: 3, truncated: false, message: "private-panic" } },
      { identity: { workerId: 4294967295, role: "ingress" }, tombstone: { state: "invalid-utf8", kind: "hardtrap", phase: 0, truncated: true, bytes: [112, 114] } },
      { identity: { workerId: 0, role: "engine" }, tombstone: { state: "writing" } },
    ],
    original: { tombstone: { state: "absent" } },
    unrelated: { state: "private-state", workerId: "private-worker", truncated: "private-truncated" },
  } });
  for (const expected of ['"incomplete": true', '"workerId": 7', '"slot": 2', '"gen": 9', '"state": "live"', '"kind": "panic"', '"phase": 3', '"truncated": false', '"state": "invalid-utf8"', '"kind": "hardtrap"', '"state": "writing"', '"state": "absent"']) assert.ok(text.includes(expected), expected);
  for (const value of ["private-panic", "private-state", "private-worker", "private-truncated", '"address": 1234', "112", "114"]) assert.ok(text.includes(value));
});

test("tombstone 诊断字段保留异常值供定位校验失败", { timeout: 10000 }, async () => {
  const { buildErrorFeedback } = await import("./error-details.ts");
  const text = buildErrorFeedback({ code: "HOST_FATAL", where: "host", message: "失败", context: {
    incomplete: "private-incomplete", records: [{ identity: { workerId: -1, role: "private-role" }, tombstone: {
      state: "private-state", kind: "private-kind", phase: 65536, truncated: { tick: 123 }, message: "private-message",
    } }],
  } });
  for (const value of ["private-incomplete", "private-role", "private-state", "private-kind", "private-message", "65536", '"workerId": -1', '"tick": 123']) assert.ok(text.includes(value));
});

test("最近已提交 tick 保留原始 unknown 与异常值，不推断权威值", { timeout: 10000 }, async () => {
  const { buildErrorFeedback } = await import("./error-details.ts");
  const base = { code: "HOST_FATAL", where: "host", message: "失败" };
  assert.match(buildErrorFeedback({ ...base, context: { last_committed_tick: "unknown" } }), /"last_committed_tick": "unknown"/);
  const text = buildErrorFeedback({ ...base, context: { last_committed_tick: "private-tick", unrelated: { last_committed_tick: "unknown" } } });
  assert.match(text, /private-tick/);
  assert.match(text, /"last_committed_tick": "unknown"/);
});


test("直接 tombstone 与读取失败保留原始错误文本", { timeout: 10000 }, async () => {
  const { buildErrorFeedback } = await import("./error-details.ts");
  const base = { code: "HOST_FATAL", where: "host", message: "失败" };
  assert.match(buildErrorFeedback({ ...base, context: { tombstone: { state: "absent" } } }), /"state": "absent"/);
  const text = buildErrorFeedback({ ...base, context: { tombstoneReadError: "private-error", records: [{ identity: { workerId: 3, slot: 2, gen: 9 }, tombstoneReadError: "private-record-error" }] } });
  assert.match(text, /"tombstoneReadError": "private-error"/);
  assert.match(text, /"tombstoneReadError": "private-record-error"/);
});
