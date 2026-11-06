import assert from "node:assert/strict";
import test from "node:test";
import { fixture, drain } from "../../../apps/web/src/host/worker-host-w1.test-support.ts";

test("独审：长save在dispose/换代时立即拒绝，晚到响应不恢复", { timeout: 10000 }, async () => {
  for (const transition of ["dispose", "generation"]) {
    const f = fixture();
    try {
      f.launch(); await drain(); f.ingress(); f.baseline(); await drain(); f.ack(f.workers[1], 1); await drain();
      const result = f.readOutcome();
      assert.equal(result.status, "fulfilled");
      if (result.status !== "fulfilled") throw new Error("真实host fixture未完成初始化");
      const saving = result.host.save({ seq: 0, settledDate: "2030-01-01" });
      const rejected = assert.rejects(saving, /过期|关闭|dispose|销毁|generation|取消/);
      const request = f.workers[0].sent.at(-1);
      assert.equal(request.type, "save");
      if (transition === "dispose") await result.host.dispose();
      else f.baseline(2);
      await rejected;
      f.workers[0].emit({ type: "saved", requestId: request.requestId, generation: 1, slot: { late: true } });
      await drain();
    } finally { await f.cleanup(); }
  }
});
