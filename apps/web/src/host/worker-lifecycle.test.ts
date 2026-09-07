import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { createWorkerLifecycle, routeWorkerFailure } from "./worker-lifecycle.ts";

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

describe("worker failure routing", () => {
  it("routes post-ready failures to the fatal runtime channel", () => {
    const routed: string[] = [];
    routeWorkerFailure(true, "rayon worker crashed", {
      initialization: (message) => routed.push(`init:${message}`),
      runtime: (message) => routed.push(`runtime:${message}`),
    });
    assert.deepEqual(routed, ["runtime:rayon worker crashed"]);
  });
});
