import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { runObserverResync } from "./observer-resync.ts";

describe("shared observer resynchronization", () => {
  it("retries stale cuts and installs only a fenced cut before releasing delivery", async () => {
    const log: string[] = [];
    await runObserverResync({
      assertCurrent: () => {},
      hold: () => { log.push("hold"); },
      capture: (attempt) => ({ ordinal: ["1", "3", "4"][attempt], value: attempt }),
      floor: () => "2",
      fence: (cut) => { log.push("fence " + cut.ordinal); return cut.ordinal === "4"; },
      install: (cut) => { log.push("install " + cut.value); },
      release: () => { log.push("release"); },
    });
    assert.deepEqual(log, ["hold", "fence 3", "fence 4", "install 2", "release"]);
  });

  it("never installs or releases after an ownership change during fencing", async () => {
    let current = true;
    const log: string[] = [];
    await assert.rejects(runObserverResync({
      assertCurrent: () => { if (!current) throw new Error("old generation"); },
      hold: () => {},
      capture: () => ({ ordinal: "1", value: null }),
      floor: () => null,
      fence: () => { current = false; return true; },
      install: () => { log.push("install"); },
      release: () => { log.push("release"); },
    }), /old generation/);
    assert.deepEqual(log, []);
  });
});

it("同 generation 新 lease 使等待中的旧 release 与 cut 失效", { timeout: 10000 }, async () => {
  const { ObserverLease } = await import("./observer-resync.ts");
  const owner = new ObserverLease();
  const first = owner.replace("7");
  let installed = false;
  await assert.rejects(runObserverResync({
    lease: first,
    assertCurrent() {}, hold() {}, floor: () => null,
    capture: () => ({ ordinal: "1", value: null }),
    fence: () => { owner.replace("7"); return true; },
    install: () => { installed = true; }, release() { throw new Error("旧 release 不应执行"); },
  }), /lease/);
  assert.equal(installed, false);
});
