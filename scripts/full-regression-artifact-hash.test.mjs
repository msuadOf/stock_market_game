import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { it } from "node:test";
import { Worker } from "node:worker_threads";
import { hashArtifactFiles } from "./full-regression-artifact-hash.mjs";

async function fixture(body) {
  const directory = path.resolve(".tmp", "artifact-hash-tests");
  await mkdir(directory, { recursive: true });
  const root = await mkdtemp(path.join(directory, "case-"));
  try { await body(root); } finally { await rm(root, { recursive: true, force: true }); }
}

function observedWorkers(factory = (url) => new Worker(url)) {
  const workers = [];
  return { workers, createWorker: (url) => { const worker = factory(url); workers.push(worker); return worker; } };
}

it("真实 worker 跨 chunk 流式 SHA 与 bytes 正确且保留重复项顺序", { timeout: 10000 }, async () => fixture(async (root) => {
  const bytes = Buffer.alloc(3 * 65536 + 17);
  for (let index = 0; index < bytes.length; index += 1) bytes[index] = index % 251;
  const filename = path.join(root, "binary");
  await writeFile(filename, bytes);
  const observed = observedWorkers();
  const result = await hashArtifactFiles([filename, filename], { cpuCount: 2, ...observed });
  assert.deepEqual(result, Array(2).fill({ bytes: bytes.length, sha256: createHash("sha256").update(bytes).digest("hex") }));
  assert.equal(observed.workers.length, 2);
  assert.ok(observed.workers.every((worker) => worker.threadId === -1), "返回前必须确认全部 worker 退出");
}));

it("两个真实 worker 同时处理文件且线程独立", { timeout: 10000 }, async () => fixture(async (root) => {
  const filename = path.join(root, "binary");
  await writeFile(filename, Buffer.alloc(1024 * 1024, 31));
  const gate = new SharedArrayBuffer(8);
  const threadIds = new Set();
  const observed = observedWorkers((url) => {
    // 同一 message 的第二个 listener 在首个异步 read 让出后等待两条线程到齐。
    const bootstrap = `import {parentPort, workerData, threadId} from 'node:worker_threads';
      await import(${JSON.stringify(url.href)});
      parentPort.on('message', () => { const gate = new Int32Array(workerData);
        Atomics.add(gate, 0, 1); Atomics.notify(gate, 0);
        while (Atomics.load(gate, 0) < 2) Atomics.wait(gate, 0, 1, 1000);
        Atomics.add(gate, 1, 1); });`;
    const worker = new Worker(new URL(`data:text/javascript,${encodeURIComponent(bootstrap)}`), { workerData: gate });
    worker.on("online", () => threadIds.add(worker.threadId));
    return worker;
  });
  await hashArtifactFiles([filename, filename], { cpuCount: 2, ...observed });
  assert.equal(threadIds.size, 2);
  assert.equal(new Int32Array(gate)[1], 2, "两个 worker 必须都越过并行 barrier");
  assert.ok(observed.workers.every((worker) => worker.threadId === -1));
}));

it("后项 prepare 先失败仍保留前项 SHA 错误并停止排队", { timeout: 10000 }, async () => fixture(async (root) => {
  const filename = path.join(root, "binary");
  await writeFile(filename, Buffer.alloc(65537, 1));
  const prepared = [];
  const observed = observedWorkers();
  await assert.rejects(hashArtifactFiles([filename, filename, filename], {
    cpuCount: 2, ...observed,
    prepare: async (entry, index) => { prepared.push(index); if (index === 1) throw new Error("后项 descriptor"); return entry; },
    verify: (_hash, _entry, index) => { if (index === 0) throw new Error("前项 SHA mismatch"); },
  }), /前项 SHA mismatch/);
  assert.deepEqual(prepared, [0, 1]);
  assert.ok(observed.workers.every((worker) => worker.threadId === -1));
}));

it("stream 错误保留路径且等待所有 worker 退出", { timeout: 10000 }, async () => fixture(async (root) => {
  const observed = observedWorkers();
  const missing = path.join(root, "missing");
  await assert.rejects(hashArtifactFiles([missing], { cpuCount: 1, ...observed }), /ENOENT.*missing/);
  assert.ok(observed.workers.every((worker) => worker.threadId === -1));
}));

for (const [label, source, expected] of [
  ["抛异常", "throw new Error('故意 worker 崩溃')", /故意 worker 崩溃/],
  ["异常退出", "process.exit(7)", /worker.*exit.*7/i],
]) it(`worker ${label}显式失败且等待全部退出`, { timeout: 10000 }, async () => fixture(async (root) => {
  const filename = path.join(root, "binary");
  await writeFile(filename, "small");
  const observed = observedWorkers(() => new Worker(new URL(`data:text/javascript,${encodeURIComponent(source)}`)));
  await assert.rejects(hashArtifactFiles([filename, filename], { cpuCount: 2, ...observed }), expected);
  assert.equal(observed.workers.length, 2);
  assert.ok(observed.workers.every((worker) => worker.threadId === -1));
}));

it("Abort 取消等待中的 worker 并确认退出", { timeout: 10000 }, async () => fixture(async (root) => {
  const filename = path.join(root, "binary");
  await writeFile(filename, "small");
  const controller = new AbortController();
  const observed = observedWorkers(() => {
    const worker = new Worker(new URL("data:text/javascript,setInterval(() => {}, 1000)"));
    worker.on("online", () => controller.abort(new Error("显式取消 hash")));
    return worker;
  });
  await assert.rejects(hashArtifactFiles([filename, filename], { cpuCount: 2, signal: controller.signal, ...observed }), /显式取消 hash/);
  assert.ok(observed.workers.every((worker) => worker.threadId === -1));
}));

for (const delayedPrepare of [false, true]) it(`首项 worker 崩溃会取消后项挂起 worker（prepare 延迟=${delayedPrepare}）`, { timeout: 10000 }, async () => fixture(async (root) => {
  const filename = path.join(root, "binary");
  await writeFile(filename, "small");
  const controller = new AbortController();
  const observed = observedWorkers(() => new Worker(new URL(observed.workers.length === 0
    ? "data:text/javascript,throw new Error('首项真实崩溃')"
    : "data:text/javascript,setInterval(() => {}, 1000)")));
  const fallback = setTimeout(() => controller.abort(new Error("取消清理未及时完成")), 750);
  try {
    await assert.rejects(hashArtifactFiles([filename, filename], { cpuCount: 2, signal: controller.signal, ...observed,
      prepare: async (entry, index) => {
        if (delayedPrepare && index === 1) await new Promise((resolve) => observed.workers[0].once("exit", resolve));
        return entry;
      },
    }), /首项真实崩溃/);
    assert.equal(controller.signal.aborted, false, "不能依赖外部取消才进入清理");
    assert.ok(observed.workers.every((worker) => worker.threadId === -1));
  } finally { clearTimeout(fallback); }
}));

it("prepare 跨过 worker error 与 exit 时保留首个原异常", { timeout: 10000 }, async () => fixture(async (root) => {
  const filename = path.join(root, "binary");
  await writeFile(filename, "small");
  const observed = observedWorkers(() => new Worker(new URL("data:text/javascript,throw new Error('原始 worker 异常')")));
  await assert.rejects(hashArtifactFiles([filename], { cpuCount: 1, ...observed,
    prepare: async (entry) => { await new Promise((resolve) => observed.workers[0].once("exit", resolve)); return entry; },
  }), /原始 worker 异常/);
  assert.ok(observed.workers.every((worker) => worker.threadId === -1));
}));
