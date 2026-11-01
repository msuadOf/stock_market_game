import assert from "node:assert/strict";
import test from "node:test";
import { createRightsSubscription } from "./rights-subscription.ts";
const receipt = { event_id: "r1", requested_shares: "5", submitted_on: "2030-01-02" };
test("认购入口验证请求和本人回执", { timeout: 10000 }, async () => {
  const sent: unknown[] = [];
  const subscribe = createRightsSubscription(async (event_id, requested_shares) => { sent.push({event_id, requested_shares}); return receipt; });
  assert.deepEqual(await subscribe("r1", "5"), receipt);
  assert.deepEqual(sent, [{ event_id: "r1", requested_shares: "5" }]);
  for (const shares of ["0", "01", "18446744073709551616"]) await assert.rejects(subscribe("r1", shares));
  await assert.rejects(subscribe("", "1"));
  assert.equal(sent.length, 1);
});
test("拒绝不匹配事件、股数及额外字段的响应", { timeout: 10000 }, async () => {
  for (const patch of [{event_id:"r2"}, {requested_shares:"6"}, {extra:true}]) {
    const subscribe = createRightsSubscription(async () => ({ ...receipt, ...patch }));
    await assert.rejects(subscribe("r1", "5"));
  }
});
