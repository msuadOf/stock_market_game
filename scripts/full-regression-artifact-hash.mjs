import os from "node:os";
import { Worker } from "node:worker_threads";

const WORKER_URL = new URL("./full-regression-artifact-hash-worker.mjs", import.meta.url);

export async function hashArtifactFiles(entries, {
  cpuCount = os.availableParallelism(), signal,
  prepare = async (entry) => entry,
  verify = () => undefined,
  createWorker = (url) => new Worker(url),
} = {}) {
  if (!Number.isInteger(cpuCount) || cpuCount <= 0) throw new Error("artifact hash requires a positive CPU count");
  if (entries.length === 0) return [];
  signal?.throwIfAborted();
  const workers = [];
  const failures = new Map();
  const results = new Array(entries.length);
  let cursor = 0;
  let stop = false;
  let failureIndex = Infinity;
  let closing = false;
  function recordFailure(index, error) {
    failures.set(index, error);
    stop = true;
    failureIndex = Math.min(failureIndex, index);
    // 只取消已确定不会成为首错的后项；先前项继续完整验证。
    for (const slot of workers) {
      if (slot.activeIndex > failureIndex) slot.pending?.reject(error);
    }
  }
  function abort() {
    stop = true;
    for (const slot of workers) slot.pending?.reject(signal.reason);
  }
  signal?.addEventListener("abort", abort, { once: true });
  try {
    // 每条线程同一时间只持有一个流；CPU 预算和 artifact 数共同限制并发。
    for (let index = 0; index < Math.min(cpuCount, entries.length); index += 1) {
      const worker = createWorker(WORKER_URL);
      const slot = { worker, pending: undefined, failure: undefined, activeIndex: index };
      workers.push(slot);
      function fail(error) {
        if (closing) return;
        if (slot.failure === undefined) slot.failure = error;
        recordFailure(slot.activeIndex, slot.failure);
        slot.pending?.reject(slot.failure);
      }
      worker.on("error", fail);
      worker.on("exit", (code) => fail(new Error(`artifact hash worker unexpected exit ${code}`)));
      worker.on("message", (message) => {
        if (slot.pending === undefined) { fail(new Error("artifact hash worker returned an unsolicited result")); return; }
        if (message?.error) {
          const error = new Error(message.error.message);
          error.code = message.error.code;
          slot.pending.reject(error);
        } else if (!Number.isSafeInteger(message?.bytes) || message.bytes < 0 || !/^[a-f0-9]{64}$/.test(message?.sha256)) {
          fail(new Error("artifact hash worker returned an invalid digest result"));
        } else slot.pending.resolve(message);
      });
    }
    async function consume(slot) {
      while (!stop && cursor < entries.length) {
        const index = cursor++;
        slot.activeIndex = index;
        try {
          const executable = await prepare(entries[index], index);
          signal?.throwIfAborted();
          if (slot.failure) throw slot.failure;
          if (index > failureIndex) return;
          const digest = await new Promise((resolve, reject) => {
            slot.pending = { resolve, reject };
            slot.worker.postMessage({ executable });
          });
          slot.pending = undefined;
          await verify(digest, entries[index], index);
          results[index] = digest;
        } catch (error) {
          recordFailure(index, error);
        }
      }
    }
    await Promise.all(workers.map(consume));
    signal?.throwIfAborted();
    // 后项先失败不能取消早项；早项的 filesystem 或 SHA 错误仍然优先。
    if (failures.size > 0) throw failures.get(Math.min(...failures.keys()));
    for (const slot of workers) if (slot.failure) throw slot.failure;
    return results;
  } finally {
    closing = true;
    signal?.removeEventListener("abort", abort);
    const cleanup = await Promise.allSettled(workers.map(({ worker }) => worker.terminate()));
    const failed = cleanup.filter((result) => result.status === "rejected").map((result) => result.reason);
    if (failed.length > 0) throw new AggregateError(failed, "artifact hash worker termination failed");
  }
}
