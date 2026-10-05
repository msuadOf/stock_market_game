import assert from "node:assert/strict";
import test from "node:test";
import { parseEngineUpdate } from "./protocol/parse.ts";
import { canonicalJson, eventStableKey } from "./protocol/canonical.ts";
import { effectsFromFacts } from "./protocol/effects.ts";
import { normalizeEngineUpdate } from "./protocol/normalize.ts";
import { reduceEngineUpdate } from "./protocol/reduce.ts";
import { createProtocolState } from "./protocol/types.ts";
import { parseProtocolSnapshot } from "./protocol/parse.ts";
import { snapshot } from "./protocol-test-fixtures.ts";
import { civilUpdate, isJsonRecord } from "./protocol-test-fixtures.ts";
import { frame, tickBatch, type JsonRecord } from "./protocol-test-fixtures.ts";

test("公开成交与私有省略覆盖全部seq，匿名量价进入trade effects而省略不生成通知", { timeout: 10000 }, () => {
  const events: JsonRecord[] = [{ PublicTrade: { seq: 1, code: "600000", price: "1000", qty: 100 } }, { PrivateEventOmitted: { seq: 2 } }];
  const facts: JsonRecord[] = events.map((event, index) => ({ event, key: index === 0 ? { phase_rank: 4, entity: { Stock: "600000" }, source: "Sealed", local_event_index: 0 } : { phase_rank: 4, entity: "Session", source: "Sealed", local_event_index: 2 }, canonical_payload: canonicalJson(event) }));
  const parsed = parseEngineUpdate(tickBatch([{ ...frame(1, 0, []), events, facts, seq_to: 2 }], null));
  assert.ok("TickBatch" in parsed);
  const normalized = normalizeEngineUpdate(parsed);
  assert.equal(normalized.kind, "tick-batch");
  const effects = effectsFromFacts(parsed.TickBatch.frames[0]!.facts, {}, 1);
  assert.deepEqual(effects, [{ kind: "trade", event: { seq: 1, code: "600000", price: "1000", qty: 100, tick: 1 } }]);
  assert.deepEqual(eventStableKey(parsed.TickBatch.frames[0]!.events[1]!, 99), facts[1]!.key);
  for (const privateField of ["maker", "taker", "account", "reason"]) {
    const corrupted: JsonRecord[] = events.map((event, index) => index === 0 ? { PublicTrade: { seq: 1, code: "600000", price: "1000", qty: 100, [privateField]: "18446744073709551615" } } : event);
    assert.throws(() => parseEngineUpdate(tickBatch([{ ...frame(1, 0, []), events: corrupted, facts, seq_to: 2 }], null)), /字段/);
  }
});

test("当前金融AccountId与实体key保留MAX u64字符串，拒绝数字和非规范值", { timeout: 10000 }, () => {
  const maximum = "18446744073709551615";
  const event = { OrderAccepted: { seq: 1, account: maximum, code: "600000", id: 3, side: "Buy", price: "1000", remaining_qty: 100 } };
  const key = { phase_rank: 4, entity: { Account: maximum }, source: "Sealed", local_event_index: 0 };
  const input = { ...frame(1, 0, []), events: [event], facts: [{ event, key, canonical_payload: canonicalJson(event) }], seq_to: 1 };
  assert.ok("TickBatch" in parseEngineUpdate(tickBatch([input], null)));
  for (const account of [0, "00", "18446744073709551616", "-1"]) {
    assert.throws(() => parseEngineUpdate(tickBatch([{ ...input, events: [{ OrderAccepted: { ...event.OrderAccepted, account } }] }], null)), /u64|字符串|规范/);
  }
});

test("MAX本人snapshot与runtime delta无Number舍入，非本人owner不能混入委托", { timeout: 10000 }, () => {
  const maximum = "18446744073709551615";
  const account = { cash: "100000", reserved_cash: "0", positions: {}, reserved_sell_qty: {} };
  const baseline = parseProtocolSnapshot({ ...snapshot(0, 0), accounts: { [maximum]: account } });
  const state = createProtocolState(baseline, "1");
  const delta = { seq_from: 0, seq_to: 0, tick: 1, day: 0, phase: "Continuous", accounts: { [maximum]: { ...account, cash: "90000" } }, working_orders: { reset: true, upserts: [], removed: [] } };
  const result = reduceEngineUpdate(state, "1", tickBatch([frame(1, 0, [])], null, delta));
  assert.equal(result.state.snapshot.accounts[maximum]!.cash, "90000");
  assert.equal(result.state.snapshot.accounts["18446744073709552000"], undefined);
  assert.throws(() => parseProtocolSnapshot({ ...snapshot(0, 0), accounts: { "00": account } }), /规范/);
});

test("CivilUpdate refresh的匿名成交与省略保留覆盖，不重放历史trade或通知", { timeout: 10000 }, () => {
  const event: JsonRecord = { PublicTrade: { seq: 1, code: "600000", price: "1000", qty: 100 } };
  const fact: JsonRecord = { event, key: { phase_rank: 4, entity: { Stock: "600000" }, source: "Sealed", local_event_index: 0 }, canonical_payload: canonicalJson(event) };
  const history = { ...frame(1, 0, []), events: [event], facts: [fact], seq_to: 1 };
  const state = reduceEngineUpdate(createProtocolState(parseProtocolSnapshot(snapshot(0, 0)), "1"), "1", tickBatch([history], snapshot(1, 1))).state;
  const raw = civilUpdate();
  const civil = raw.CivilUpdate;
  if (!isJsonRecord(civil) || !isJsonRecord(civil.refresh)) throw new Error("日终 fixture 必须完整");
  const result = reduceEngineUpdate(state, "1", { CivilUpdate: { ...civil, refresh: { ...civil.refresh, intraday: [history] } } });
  assert.equal(result.state.cursor.seq, 2);
  assert.deepEqual(result.effects.map((effect) => effect.kind), ["civil-barrier", "civil-barrier"]);
  assert.deepEqual(result.state.intraday[0]!.events, [event]);
});
