import assert from "node:assert/strict";
import test from "node:test";
import { postWorkerProtocol } from "./worker-flush.ts";

test("Given a Worker EngineUpdate, when posted, then it remains one generation-tagged protocol envelope", () => {
  const messages: unknown[] = [];
  postWorkerProtocol({ postMessage: (value) => messages.push(value) }, "1", { TickBatch: { frames: [], runtime_snapshot: null } });
  assert.deepEqual(messages, [{ type: "protocol", generation: "1", update: { TickBatch: { frames: [], runtime_snapshot: null } }, civilDate: null, revision: null }]);
});
