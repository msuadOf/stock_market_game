import assert from "node:assert/strict";
import test from "node:test";
import { sendRestoreChunks, receiveRestoreChunks, RestoreSourceCleanupTimeoutError } from "./restore-chunk-port.ts";

const timeout = { timeout: 10000 };
async function* source(bytes: Uint8Array): AsyncIterable<Uint8Array> { yield bytes; }
test("分块传输按消费完成确认，并保留字节与长度", timeout, async () => {
  const channel = new MessageChannel();
  const bytes = new Uint8Array(150000).map((_, index) => index % 251);
  const chunks: Uint8Array[] = [];
  let release!: () => void;
  const blocked = new Promise<void>((resolve) => { release = resolve; });
  const received = receiveRestoreChunks(channel.port2, 7, bytes.length, async (chunk) => {
    chunks.push(chunk);
    if (chunks.length === 1) await blocked;
  });
  const sent = sendRestoreChunks(channel.port1, 7, bytes.length, source(bytes));
  await new Promise((resolve) => setTimeout(resolve, 30));
  assert.equal(chunks.length, 1);
  release();
  await Promise.all([sent, received]);
  assert.deepEqual(chunks.map((chunk) => chunk.length), [65536, 65536, 18928]);
  assert.deepEqual(Buffer.concat(chunks), Buffer.from(bytes));
});

test("完成钩子结束前不得回发 done；钩子失败向发送端报告错误", timeout, async () => {
  for (const shouldFail of [false, true]) {
    const channel = new MessageChannel(); let release!: () => void; let started!: () => void;
    const gate = new Promise<void>(resolve => { release = resolve; });
    const entering = new Promise<void>(resolve => { started = resolve; });
    const receiver = receiveRestoreChunks(channel.port2, 8, 1, async () => {}, undefined, async () => {
      started(); await gate; if (shouldFail) throw new Error("最终校验失败");
    });
    let senderDone = false;
    const sender = sendRestoreChunks(channel.port1, 8, 1, source(Uint8Array.of(65))).finally(() => { senderDone = true; });
    const settledSender = shouldFail ? assert.rejects(sender, /最终校验失败/) : sender;
    const settledReceiver = shouldFail ? assert.rejects(receiver, /最终校验失败/) : receiver;
    await entering; assert.equal(senderDone, false); release();
    await Promise.all([settledSender, settledReceiver]);
  }
});

test("接收端拒绝错误序号、长度和超大分块", timeout, async () => {
  for (const message of [
    { type: "chunk", generation: 1, seq: 1, bytes: new Uint8Array([1]) },
    { type: "chunk", generation: 1, seq: 0, bytes: new Uint8Array(65537) },
    { type: "eof", generation: 1, seq: 0, length: 1 },
  ]) {
    const channel = new MessageChannel();
    const received = receiveRestoreChunks(channel.port2, 1, 2, async () => {});
    channel.port1.postMessage(message);
    await assert.rejects(received);
    channel.port1.close();
  }
});

test("旧 generation 不影响当前会话，取消显式失败", timeout, async () => {
  const channel = new MessageChannel();
  const abort = new AbortController();
  const received = receiveRestoreChunks(channel.port2, 2, 1, async () => {}, abort.signal);
  channel.port1.postMessage({ type: "chunk", generation: 1, seq: 99, bytes: new Uint8Array([1]) });
  channel.port1.postMessage({ type: "chunk", generation: 2, seq: 0, bytes: new Uint8Array([9]) });
  const ack = await new Promise<MessageEvent>((resolve) => { channel.port1.onmessage = resolve; });
  assert.equal(ack.data.seq, 0);
  abort.abort();
  await assert.rejects(received, /取消/);
  channel.port1.close();
});

test("消费异常传递给发送端", timeout, async () => {
  const channel = new MessageChannel();
  const received = receiveRestoreChunks(channel.port2, 1, 1, async () => { throw new Error("消费失败"); });
  const sent = sendRestoreChunks(channel.port1, 1, 1, source(new Uint8Array([1])));
  await assert.rejects(received, /消费失败/);
  await assert.rejects(sent, /消费失败/);
});

test("consumer 转移 buffer 后仍以原始字节数校验", timeout, async () => {
  const channel = new MessageChannel();
  const received = receiveRestoreChunks(channel.port2, 1, 2, async (bytes) => { structuredClone(bytes, { transfer: [bytes.buffer] }); });
  await Promise.all([sendRestoreChunks(channel.port1, 1, 2, source(new Uint8Array([1, 2]))), received]);
});

test("非法参数关闭独占端口", timeout, async () => {
  const channel = new MessageChannel();
  const closed = new Promise<void>((resolve) => channel.port1.addEventListener("close", () => resolve()));
  assert.throws(() => receiveRestoreChunks(channel.port2, 0, 0, async () => {}));
  await closed;
  channel.port1.close();
});

test("空内容可完成 EOF，等待 ACK 时取消会通知对端", timeout, async () => {
  const empty = new MessageChannel();
  await Promise.all([sendRestoreChunks(empty.port1, 1, 0, (async function* () {})()), receiveRestoreChunks(empty.port2, 1, 0, async () => { throw new Error("不应收到分块"); })]);
  const channel = new MessageChannel();
  const abort = new AbortController();
  const sent = sendRestoreChunks(channel.port1, 1, 1, source(new Uint8Array([1])), abort.signal);
  const first = await new Promise<MessageEvent>((resolve) => { channel.port2.onmessage = resolve; });
  assert.equal(first.data.type, "chunk");
  const notified = new Promise<MessageEvent>((resolve) => { channel.port2.onmessage = resolve; });
  abort.abort();
  await assert.rejects(sent, /取消/);
  assert.equal((await notified).data.type, "error");
  channel.port2.close();
});

test("超出声明长度及 credit 未归还时重复分块均被拒绝", timeout, async () => {
  const excessive = new MessageChannel();
  const tooLong = receiveRestoreChunks(excessive.port2, 1, 1, async () => {});
  excessive.port1.postMessage({ type: "chunk", generation: 1, seq: 0, bytes: new Uint8Array(2) });
  await assert.rejects(tooLong, /长度/); excessive.port1.close();
  const channel = new MessageChannel();
  let release!: () => void;
  const blocked = new Promise<void>((resolve) => { release = resolve; });
  const received = receiveRestoreChunks(channel.port2, 1, 2, async () => blocked);
  channel.port1.postMessage({ type: "chunk", generation: 1, seq: 0, bytes: new Uint8Array(1) });
  channel.port1.postMessage({ type: "chunk", generation: 1, seq: 0, bytes: new Uint8Array(1) });
  setTimeout(release, 20); await assert.rejects(received, /credit/); channel.port1.close();
});

test("对端意外关闭会结束等待", timeout, async () => {
  const channel = new MessageChannel();
  const sent = sendRestoreChunks(channel.port1, 1, 1, source(new Uint8Array([1])));
  channel.port2.close();
  await assert.rejects(sent, /关闭/);
});

test("流读取受 ACK 背压约束且不物化完整存档", timeout, async () => {
  const channel = new MessageChannel();
  let reads = 0; let release!: () => void;
  const blocked = new Promise<void>((resolve) => { release = resolve; });
  async function* chunks() { reads++; yield new Uint8Array(65537); reads++; yield new Uint8Array([3]); }
  let consumed = 0;
  const receiver = receiveRestoreChunks(channel.port2, 1, 65538, async () => { if (++consumed === 1) await blocked; });
  const sender = sendRestoreChunks(channel.port1, 1, 65538, chunks());
  await new Promise((resolve) => setTimeout(resolve, 30));
  assert.equal(reads, 1); assert.equal(consumed, 1); release();
  await Promise.all([sender, receiver]); assert.equal(reads, 2);
});

test("超长、短流和源异常均显式失败并通知接收端", timeout, async () => {
  for (const input of [source(new Uint8Array(2)), (async function* () {})(), (async function* () { if (Date.now() > 0) throw new Error("源失败"); yield new Uint8Array(); })()]) {
    const channel = new MessageChannel();
    const receiver = receiveRestoreChunks(channel.port2, 1, 1, async () => {});
    const sender = sendRestoreChunks(channel.port1, 1, 1, input);
    const results = await Promise.allSettled([sender, receiver]);
    assert.equal(results[0].status, "rejected"); assert.equal(results[1].status, "rejected");
  }
});

test("接收端取消会终止源迭代", timeout, async () => {
  const channel = new MessageChannel(); const abort = new AbortController();
  let closed = false;
  async function* chunks() { try { yield new Uint8Array([1]); yield new Uint8Array([2]); } finally { closed = true; } }
  const receiver = receiveRestoreChunks(channel.port2, 1, 2, async () => { abort.abort(); }, abort.signal);
  const sender = sendRestoreChunks(channel.port1, 1, 2, chunks());
  await Promise.allSettled([sender, receiver]); assert.equal(closed, true);
});

test("源清理失败仍关闭端口并显式报告", timeout, async () => {
  const channel = new MessageChannel();
  const input: AsyncIterable<Uint8Array> = { [Symbol.asyncIterator]: () => ({ next: async () => ({ done: false, value: new Uint8Array([1]) }), return: () => { throw new Error("清理失败"); } }) };
  const closed = new Promise<void>((resolve) => channel.port2.addEventListener("close", () => resolve()));
  const abort = new AbortController();
  const sender = sendRestoreChunks(channel.port1, 1, 1, input, abort.signal);
  await new Promise<MessageEvent>((resolve) => { channel.port2.onmessage = resolve; });
  abort.abort();
  await assert.rejects(sender, /源清理失败/); await closed; channel.port2.close();
});

test("空源块被拒绝", timeout, async () => {
  const channel = new MessageChannel();
  await assert.rejects(sendRestoreChunks(channel.port1, 1, 0, source(new Uint8Array())), /非空/);
  channel.port2.close();
});

test("接收端取消会调用源取消钩子以中断等待中的 next", timeout, async () => {
  const channel = new MessageChannel(); let cancel!: () => void; let cancelled = false;
  const pending = new Promise<IteratorResult<Uint8Array>>((resolve) => { cancel = () => { cancelled = true; resolve({ done: true, value: undefined }); }; });
  const input: AsyncIterable<Uint8Array> = { [Symbol.asyncIterator]: () => ({ next: () => pending }) };
  const sender = sendRestoreChunks(channel.port1, 1, 0, input, undefined, () => cancel());
  channel.port2.postMessage({ type: "error", generation: 1, reason: "取消" });
  await assert.rejects(sender, /取消/); assert.equal(cancelled, true); channel.port2.close();
});

test("阻塞的源不能使取消无限等待，取消钩子失败也会报告", timeout, async () => {
  for (const failCancel of [false, true]) {
    const channel = new MessageChannel(); const abort = new AbortController();
    async function* stalled() { await new Promise(() => {}); yield new Uint8Array([1]); }
    const sent = sendRestoreChunks(channel.port1, 1, 1, stalled(), abort.signal, failCancel ? () => { throw new Error("取消源失败"); } : undefined);
    abort.abort();
    await assert.rejects(sent, (error: unknown) => error instanceof RestoreSourceCleanupTimeoutError && error.sourceMayStillBeRunning && error.errors.some((item) => item instanceof Error && /超时/.test(item.message)) && (!failCancel || error.errors.some((item) => item instanceof Error && /取消源失败/.test(item.message))));
    channel.port2.close();
  }
});

test("接收端取消须等待正在执行的 consumer 结束", timeout, async () => {
  const channel = new MessageChannel(); const abort = new AbortController();
  let release!: () => void; let started!: () => void;
  const gate = new Promise<void>(resolve => { release = resolve; });
  const active = new Promise<void>(resolve => { started = resolve; });
  let finished = false;
  const result = receiveRestoreChunks(channel.port2, 1, 1, async () => { started(); await gate; }, abort.signal).finally(() => { finished = true; });
  channel.port1.postMessage({ type: "chunk", generation: 1, seq: 0, bytes: new Uint8Array([1]) });
  await active; abort.abort(); await new Promise(resolve => setTimeout(resolve, 30));
  assert.equal(finished, false); release(); await assert.rejects(result, /取消/); channel.port1.close();
});

test("consumer 无法结束时明确报告状态未知", timeout, async () => {
  const channel = new MessageChannel(); const abort = new AbortController(); let started!: () => void;
  const active = new Promise<void>(resolve => { started = resolve; });
  const result = receiveRestoreChunks(channel.port2, 1, 1, async () => { started(); await new Promise(() => {}); }, abort.signal);
  channel.port1.postMessage({ type: "chunk", generation: 1, seq: 0, bytes: new Uint8Array([1]) });
  await active; abort.abort();
  await assert.rejects(result, error => error instanceof Error && error.name === "RestoreConsumerCleanupTimeoutError" && (error as Error & { consumerMayStillBeRunning: boolean }).consumerMayStillBeRunning);
  channel.port1.close();
});
