import assert from "node:assert/strict";
import test from "node:test";
import { markSharedControl, requireSharedControlWord, retractSharedControl, sharedControlWord, SharedSubstratePublisher } from "../../../../packages/libos/browser/shared-substrate-publisher.ts";

function fixture() {
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true });
  const messages: string[] = [];
  const failures: unknown[] = [];
  const values: number[] = [];
  const signal = new Int32Array(memory.buffer, 4, 1);
  let resume: (() => void) | undefined;
  let injectGap = false;
  let takes = 0;
  const publisher = new SharedSubstratePublisher<number>({
    memory,
    signal: () => 4,
    take() {
      takes += 1;
      if (injectGap) { injectGap = false; Atomics.add(signal, 0, 1); values.push(9); return null; }
      return values.shift() ?? null;
    },
    deliver(value, generation) { messages.push(`${generation}:${value}`); return "continue"; },
    failure(error) { failures.push(error); },
    wait: async () => { await new Promise<void>((resolve) => { resume = resolve; }); },
    notify() { const done = resume; resume = undefined; done?.(); },
    yieldTask: () => Promise.resolve(),
  });
  return { publisher, messages, failures, values, signal, bump() { Atomics.add(signal, 0, 1); }, wake() { resume?.(); resume = undefined; }, race() { injectGap = true; }, takes: () => takes };
}

test("发布端在第一次绑定后处理已有数据；停止时唤醒并等待读者退出", async () => {
  const f = fixture();
  f.values.push(1, 2);
  await f.publisher.bind(1, 7, () => f.messages.push("bound"));
  assert.deepEqual(f.messages, ["bound", "1:1", "1:2"]);
  await f.publisher.close();
  const takes = f.takes();
  f.values.push(3);
  f.bump(); f.wake();
  await Promise.resolve();
  assert.equal(f.takes(), takes);
  assert.deepEqual(f.failures, []);
});

test("空读后信号已变化时重读，不丢失竞态中的唤醒", async () => {
  const f = fixture();
  f.race();
  await f.publisher.bind(1, 7, () => undefined);
  assert.deepEqual(f.messages, ["1:9"]);
  await f.publisher.close();
});

test("换代拒绝旧代，且新数据只带新 generation", async () => {
  const f = fixture();
  await f.publisher.bind(1, 7, () => undefined);
  await assert.rejects(f.publisher.bind(1, 8, () => undefined), /generation/);
  f.values.push(2);
  await f.publisher.bind(2, 8, () => undefined);
  assert.deepEqual(f.messages, ["2:2"]);
  await f.publisher.close();
});

test("同代刷新停止旧读者，并在恢复确认之后才交付新数据", async () => {
  const f = fixture();
  await f.publisher.bind(1, 7, () => undefined);
  await f.publisher.resume(1, (token) => { assert.equal(token, 7); f.values.push(2); return true; },
    (accepted) => f.messages.push(`resumed:${accepted}`));
  assert.deepEqual(f.messages, ["resumed:true", "1:2"]);
  await f.publisher.close();
});

test("共享控制字只绑定对齐的一字宽视图，并可撤回未发出的命令", () => {
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true });
  for (const address of [0, 3, memory.buffer.byteLength]) assert.throws(() => sharedControlWord(memory, address), /地址/);
  const word = requireSharedControlWord(sharedControlWord(memory, 4));
  assert.throws(() => requireSharedControlWord(new Int32Array(new SharedArrayBuffer(16), 4, 2)), /控制/);
  markSharedControl(word);
  assert.equal(Atomics.load(word, 0), 1);
  retractSharedControl(word);
  assert.equal(Atomics.load(word, 0), 0);
});

test("两阶段续接先停车，确认游标后仍不发送，直到显式启动", async () => {
  const f = fixture();
  await f.publisher.bind(1, 7, () => undefined);
  await f.publisher.pause(1);
  f.values.push(3);
  f.bump();
  await f.publisher.resume(1, () => false, (accepted) => f.messages.push(`resume:${accepted}`), false);
  await f.publisher.resume(1, () => true, (accepted) => f.messages.push(`resume:${accepted}`), false);
  assert.deepEqual(f.messages, ["resume:false", "resume:true"]);
  f.publisher.start(1, () => f.messages.push("start"));
  assert.deepEqual(f.messages, ["resume:false", "resume:true", "start", "1:3"]);
  await f.publisher.close();
});
