import assert from "node:assert/strict";
import test from "node:test";
import { instrumentWindow, measuredAuthority } from "./libos-performance-window.mjs";

test("测量窗口只含预热之后的第一交易日至日结，不含存档恢复", async () => {
  let tick = 0;
  const calls = [];
  const host = {
    async step() { return ++tick; },
    async civil() { calls.push("civil"); return tick; },
  };
  let now = 0;
  const recorder = instrumentWindow(host, {
    warmupTicks: 2, measuredTicks: 3, now: () => ++now,
    start: async () => { calls.push("start"); },
    end: async () => { calls.push("end"); return { memoryPeak: 42 }; },
  });
  for (let i = 0; i < 5; i++) await recorder.host.step();
  await recorder.host.civil();
  await recorder.host.step();
  await recorder.host.civil();
  assert.deepEqual(calls, ["start", "civil", "end", "civil"]);
  assert.deepEqual(recorder.results(), { elapsedNs: 7, tickNs: [1, 1, 1], memoryPeak: 42 });
});

test("空跑、错窗和假受理缺少权威凭证时拒绝", () => {
  assert.throws(() => measuredAuthority({ trace: [], inputs: [], checkpoint: {} }, {
    warmupTicks: 2, measuredTicks: 3, activeGroups: 1,
  }), /测量窗/);
});

test("玩家和 NPC 的交易与双 Desk 必须来自窗口本身，不能借用预热期见证", () => {
  const event = (item) => ({ key: { source: "Sealed" }, event: item });
  const witness = {
    // 整场计数不得用于通过稳态窗。
    playerTradeCount: 10, npcTradeCount: 10, groups: 1, desks: 2, inputs: [],
    trace: [
      { kind: "tick", tick: 3, to: 31, facts: [
        event({ OrderAccepted: { account: "0" } }), event({ OrderAccepted: { account: "1" } }),
        event({ Trade: { code: 10, maker: "0", taker: "1" } }),
      ] },
      { kind: "tick", tick: 4, to: 44, facts: [] },
      { kind: "tick", tick: 5, to: 50, facts: [] },
      { kind: "civil", tick: 5, to: 51, facts: [] },
    ],
    checkpoint: { ingress_receipt_cursors: { next_account_ordinal: {}, next_stock_ordinal: {} } },
  };
  const config = { warmupTicks: 2, measuredTicks: 3, activeGroups: 1 };
  assert.throws(() => measuredAuthority(witness, config), /双 Desk/);
  witness.trace[1].facts.push(event({ Trade: { code: 11, maker: "1", taker: "0" } }));
  assert.deepEqual(measuredAuthority(witness, config).summary, {
    firstCommittedTick: 3, lastCommittedTick: 5, lastCommittedSeq: 51,
    activeGroups: 1, deskExchanges: ["Shanghai", "Shenzhen"], playerAccepts: 1, npcOrders: 1,
  });
});
