import assert from "node:assert/strict";
import test from "node:test";
import { observedNpcGroups, verifyAuthoritySpan } from "./libos-product-scenario.mjs";

const slot = {
  snapshot: { accounts: { "0": {}, "1": {}, "12": {}, "3": {}, "7": {}, "5": {} } },
  npc_attention: { "1": {}, "12": {}, "3": {}, "5": {} },
};

test("真实成交见证按整数 AccountId 名册切片；排除玩家和没有 NPC 策略的账户", { timeout: 10000 }, () => {
  assert.deepEqual(observedNpcGroups(slot, new Set(["0", "1", "3", "12", "7"]), 2), [2, 1]);
  assert.deepEqual(observedNpcGroups(slot, new Set(["0", "1", "3", "7"]), 2), [2, 0]);
  assert.deepEqual(observedNpcGroups(slot, new Set(["12", "3"]), 1), [2]);
});

test("无效拓扑或无日终账户事实的成交 NPC 不能伪造组活动", { timeout: 10000 }, () => {
  assert.throws(() => observedNpcGroups(slot, new Set(), 3), /已验证的宿主拓扑/);
  assert.throws(
    () => observedNpcGroups({ ...slot, npc_attention: { "100": {} } }, new Set(["100"]), 2),
    /成交 NPC 100 缺少日终账户事实/,
  );
});

test("自由运行可以跨 Desk 乱序交付，但不可跳 seq、丢事实或重用稳定身份", { timeout: 10000 }, () => {
  const fact = (seq, code) => ({
    key: { phase_rank: 4, entity: { Stock: code }, source: "PriceTick", local_event_index: 0 },
    event: { AuctionTick: { seq, code } },
  });
  const valid = { from: 10, to: 12, facts: [fact(12, 1), fact(11, 0)] };
  assert.doesNotThrow(() => verifyAuthoritySpan(valid, 10));
  assert.throws(() => verifyAuthoritySpan(valid, 9), /seq 覆盖不连续/);
  assert.throws(() => verifyAuthoritySpan({ ...valid, facts: [fact(11, 0)] }, 10), /seq 覆盖不连续/);
  assert.throws(() => verifyAuthoritySpan({ ...valid, facts: [fact(11, 0), fact(11, 1)] }, 10), /seq 缺失或重复/);
  assert.throws(() => verifyAuthoritySpan({ ...valid, facts: [fact(11, 0), fact(12, 0)] }, 10), /重复 EventStableKey/);
});
