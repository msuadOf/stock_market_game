import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { createWorkerLifecycle } from "./worker-lifecycle.ts";

describe("worker lifecycle", () => {
  it("keeps pause reusable but terminates the worker exactly once on dispose", () => {
    const messages: unknown[] = [];
    let terminations = 0;
    const lifecycle = createWorkerLifecycle({
      postMessage(message: unknown) { messages.push(message); },
      terminate() { terminations += 1; },
    });

    lifecycle.pause();
    lifecycle.pause();
    assert.deepEqual(messages, [{ type: "stop" }, { type: "stop" }]);
    assert.equal(terminations, 0);

    lifecycle.dispose();
    lifecycle.dispose();
    assert.equal(terminations, 1);
    assert.throws(() => lifecycle.pause(), /已经销毁/);
  });
});
