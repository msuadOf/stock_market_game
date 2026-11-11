import assert from "node:assert/strict";
import test from "node:test";
import { createBrowserChunkedArchiveRestore } from "./chunked-archive-restore.ts";

test("正式分块能力使用真实包的 chunked 模式，并将租约与取消信号传给 Worker", async () => {
  const calls: unknown[] = [];
  const owner = {};
  const adapter = createBrowserChunkedArchiveRestore({
    documentKey: owner,
    runtimeOptions: () => ({ runtimeOverride: true }),
    prepare: async (key, options) => { calls.push([key, options]); },
    create: async (setup, seed, options) => { calls.push([setup, seed, options]); return {} as never; },
    e2e: false,
  });
  await adapter.prepareCapability({} as never);
  const abort = new AbortController();
  const ticket = {} as never, lease = {} as never, setup = {} as never;
  await adapter.createHost({ setup, seed: 5n, ticket, lease, signal: abort.signal, onSpawnFailure() {}, onInitializationProgress() {} });
  assert.deepEqual(calls[0], [owner, { runtimeOverride: true, restoreMode: "chunked" }]);
  const options = (calls[1] as unknown[])[2] as Record<string, unknown>;
  assert.equal(options.initialChunkedLease, lease);
  assert.equal(options.initialChunkedSignal, abort.signal);
  assert.equal(options.memoryAdmissionTicket, ticket);
});
