import assert from "node:assert/strict";
import test from "node:test";
import { drain, fixture } from "./worker-host-w1.test-support.ts";

for (const invalid of [0, -1, 0x100000000, 1.5, "1", undefined]) {
  test(`W1 ingress 在构造 Worker 前拒绝非法 token ${String(invalid)}`, { timeout: 10000 }, async () => {
    const f = fixture();
    try {
      f.launch(); await drain();
      f.workers[0].emit({ type: "ingress", generation: 1, token: invalid, shared: true });
      f.baseline(); await drain();
      if (f.workers[1] !== undefined) { f.ack(f.workers[1], 1); await drain(); }
      assert.equal(f.outcome.status, "rejected");
      assert.equal(f.workers.length, 1, "非法 token 不得分配 ingress Worker");
      assert.equal(f.workers[0].terminations, 1);
    } finally { await f.cleanup(); }
  });
}

for (const extra of [{ shared: false }, { module: "伪造 substrate" }, { generation: 2 }]) {
  test(`W1 ingress 显式拒绝非共享、额外 substrate 或非当前代 ${JSON.stringify(extra)}`, { timeout: 10000 }, async () => {
    const f = fixture();
    try {
      f.launch(); await drain(); f.ingress(1, 1, extra); f.baseline(); await drain();
      if (f.workers[1] !== undefined) { f.ack(f.workers[1], extra.generation === 2 ? 2 : 1); await drain(); }
      assert.equal(f.outcome.status, "rejected", "不能成功初始化或退回 engine enqueue");
      assert.equal(f.workers.length, 1);
      assert.equal(f.workers[0].sent.some((message) => message.type === "enqueue"), false);
    } finally { await f.cleanup(); }
  });
}

test("W1 首次 ingress 借用 supervisor 已配置 substrate，换代只传 token", { timeout: 10000 }, async () => {
  const f = fixture();
  try {
    f.launch(); await drain(); f.ingress(); f.baseline(); await drain();
    const intake = f.workers[1]; const first = intake.sent[0];
    assert.ok(f.configuredSubstrate, "真实控制端必须已配置 substrate");
    assert.equal(first.memory, f.configuredSubstrate.memory);
    assert.equal(first.module, f.configuredSubstrate.module);
    assert.deepEqual(first.layout, f.substrate.layout);
    assert.equal(first.diagnostics, false);
    assert.equal(Object.hasOwn(first, "controlPort"), false);
    f.ack(intake, 1); await drain();
    assert.equal(f.outcome.status, "fulfilled");
    f.baseline(2); f.ingress(2, 99); await drain();
    const next = intake.sent.at(-1)!;
    assert.deepEqual(Object.keys(next).sort(), ["generation", "requestId", "shared", "token", "type"]);
    assert.equal(next.token, 99); assert.equal(f.workers.length, 2);
    f.ack(intake, 2); await drain();
  } finally { await f.cleanup(); }
});

for (const obsoleteOutcome of ["resolve", "reject"] as const) {
  test(`W1 初始化中旧 ready ${obsoleteOutcome} 不得完成或杀死新代`, { timeout: 10000 }, async () => {
    const f = fixture();
    try {
      f.launch(); await drain(); f.ingress(); f.baseline(); await drain();
      const intake = f.workers[1];
      // 同轮事件中先使旧 Promise settle，再推进 generation；then/catch 执行时旧引用已失效。
      f.ack(intake, 1, obsoleteOutcome === "reject" ? "旧 bind 失败" : undefined);
      f.baseline(2); f.ingress(2, 2); await drain();
      assert.equal(f.outcome.status, "pending", "旧 ready 的迟到 continuation 不得裁决新代初始化");
      assert.equal(f.workers[0].terminations, 0); assert.equal(intake.terminations, 0);
      f.ack(intake, 2); await drain();
      const current = f.readOutcome();
      assert.equal(current.status, "fulfilled");
      if (current.status === "fulfilled") assert.equal(current.host.tick(), 2);
    } finally { await f.cleanup(); }
  });
}

test("W1 当前 bind 失败仍 fatal 且关闭 engine/ingress 和所有 timer", { timeout: 10000 }, async () => {
  const f = fixture();
  try {
    f.launch(); await drain(); f.ingress(); f.baseline(); await drain();
    f.ack(f.workers[1], 1, "当前 bind 真实失败"); await drain();
    assert.equal(f.outcome.status, "rejected");
    if (f.outcome.status === "rejected") assert.match(String(f.outcome.error), /当前 bind 真实失败/);
    assert.ok(f.workers.every((worker) => worker.terminations === 1));
    assert.equal(f.clock.tasks.size, 0);
  } finally { await f.cleanup(); }
});

test("W1 初始化换代不能重置 main 10000ms 总 deadline", { timeout: 10000 }, async () => {
  const f = fixture();
  try {
    f.launch(); await drain(); f.ingress(); f.baseline(); await drain();
    f.clock.advance(9000); f.baseline(2); f.ingress(2, 2); await drain();
    f.clock.advance(1000); await drain();
    assert.equal(f.outcome.status, "rejected");
    if (f.outcome.status === "rejected") assert.match(String(f.outcome.error), /超时|deadline|TIMEOUT/);
    assert.ok(f.workers.every((worker) => worker.terminations === 1));
    assert.equal(f.clock.tasks.size, 0);
  } finally { await f.cleanup(); }
});

test("W1 submitIntent 等待 bind 时换代，旧 ready 不能投递到新 token", { timeout: 10000 }, async () => {
  const f = fixture();
  try {
    f.launch(); await drain(); f.ingress(); f.baseline(); await drain(); f.ack(f.workers[1], 1); await drain();
    const result = f.readOutcome(); assert.equal(result.status, "fulfilled");
    if (result.status !== "fulfilled") throw new Error("fixture 初始化失败");
    f.baseline(2); f.ingress(2, 2); await drain();
    const submitting = result.host.submitIntent({ Cancel: { code: "000812", id: 7 } } as Parameters<typeof result.host.submitIntent>[0]);
    const rejected = assert.rejects(submitting, /过期|generation|关闭|取消/);
    f.baseline(3); f.ingress(3, 3); await drain();
    f.ack(f.workers[1], 2); await drain(); f.ack(f.workers[1], 3); await drain();
    await rejected;
    assert.equal(f.workers[1].sent.some((message) => message.type === "enqueue"), false, "旧请求不能借新 token 进队");
    assert.equal(f.workers[0].terminations, 0, "过期请求属于显式取消，不能误杀新代");
  } finally { await f.cleanup(); }
});
