import assert from "node:assert/strict"
import test from "node:test"
import { currentSaveFixture } from "./current-save-fixture.ts"
import { parseSaveSlot } from "./save-schema.ts"
import { parseInformationStates } from "./schema/personal/information.ts"
import { parseNpcDecisionDiagnostics } from "../host/npc-decision-trace.ts"
import { parseSaveRuntime } from "./schema/runtime-state.ts"
import { parseOrderState } from "./schema/orders.ts"

const maximum = "18446744073709551615"

test("Runtime envelopes 与 NPC 队列不缩窄账户，order 和 seq 仍为 number", { timeout: 10000 }, () => {
  const save = currentSaveFixture()
  const runtime = { ...(save.runtime_state as object), live_envelopes: [{ key: { account: maximum, stock: "600101", order: 1, side: "Buy" }, charged: { commission: "0", stamp_tax: "0", transfer_fee: "0" } }] }
  assert.equal(parseSaveRuntime(runtime).live_envelopes[0]?.key.account, maximum)
  const batch = { observed_tick: 0, observed_accounts: [maximum], intents: [], dependencies: [] }
  assert.deepEqual(parseOrderState({ ...save, pending_npc: batch }).pending_npc, batch)
  assert.throws(() => parseOrderState({ ...save, pending_npc: { ...batch, observed_accounts: ["0"] } }), /NPC/)
  assert.throws(() => parseSaveRuntime({ ...runtime, live_envelopes: [{ ...runtime.live_envelopes[0], key: { account: maximum, stock: "600101", order: "1", side: "Buy" } }] }), /order/)
})

test("当前存档账户键与本人金融 owner 无损支持完整 u64", { timeout: 10000 }, () => {
  const save = currentSaveFixture()
  save.market_memberships = { members: { owner: { account_id: "0", admission_funding: { external_cash: "0" } }, remote: { account_id: maximum, admission_funding: { external_cash: "1000" } } } }
  save.snapshot = { ...(save.snapshot as object), accounts: { "0": { cash: "0", positions: {} }, "1": { cash: "0", positions: {} }, [maximum]: { cash: "1000", positions: {} } } }
  save.history_reads = { "0": { stocks: {} }, "1": { stocks: {} }, [maximum]: { stocks: {} } }
  // 本测试改写账户键并清空持仓而专注账户键编码不变量；N2a 起 fixture 携带
  // 开局自动名册，持仓勾稽会因清空持仓误拒——名册与目标不变量正交，显式
  // 移除后再断言。
  ;(save.corporate_actions as Record<string, unknown>).registries = []
  save.pending_player = [{ owner: maximum, intent: { PlaceMarket: { code: "600101", side: "Buy", qty: 100 } }, account_ordinal: "0", stock_ordinal: "0" }]
  assert.equal(parseSaveSlot(save).pending_player[0]?.owner, maximum)
  for (const invalid of [0, "01", "-1", "18446744073709551616"]) {
    save.pending_player = [{ owner: invalid, intent: { PlaceMarket: { code: "600101", side: "Buy", qty: 100 } }, account_ordinal: "0", stock_ordinal: "0" }]
    assert.throws(() => parseSaveSlot(save), /owner/)
  }
})

test("个人认识 owner 使用严格规范 u64 字符串", { timeout: 10000 }, () => {
  assert.equal(parseInformationStates({ [maximum]: { owner: maximum, companies: {} } })[maximum]?.owner, maximum)
  for (const owner of [1, "01", "18446744073709551616"]) assert.throws(() => parseInformationStates({ "1": { owner, companies: {} } }), /owner/)
})

test("NPC diagnostics 不把完整 u64 account 转换为 Number", { timeout: 10000 }, () => {
  const record = { account: maximum, tick: "0", source_report_ids: [], expectation_method: null, plan_ids: [], plan_changes: [], budget_constraints: [], order_ids: [], codes: [] }
  assert.deepEqual(parseNpcDecisionDiagnostics({ kind: "supported", records: [record] }), { kind: "supported", records: [record] })
  for (const account of [1, "01", "18446744073709551616"]) assert.throws(() => parseNpcDecisionDiagnostics({ kind: "supported", records: [{ ...record, account }] }), /NPC/)
})
