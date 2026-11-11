import assert from "node:assert/strict"
import test from "node:test"
import { currentSaveFixture } from "../current-save-fixture.ts"
import { parseSaveJson } from "../save-schema.ts"
import { scanArchiveRoot } from "./archive-root-scan.ts"
import { SchemaFailureLedger } from "./shard-errors.ts"
import { PersonalShardValidation } from "./personal-shard-validation.ts"
import { validateRootMetadata } from "./root-metadata-validation.ts"
import { SessionShardValidation } from "./session-shard-validation.ts"

function fixture() {
  return currentSaveFixture() as Record<string, unknown> & {
    snapshot: Record<string, unknown> & { accounts: Record<string, unknown> }
    runtime_state: Record<string, unknown> & { strategy_states: Record<string, unknown> }
  }
}

async function* chunks(text: string): AsyncIterable<string> { for (let index = 0; index < text.length; index += 23) yield text.slice(index, index + 23) }
async function grouped(text: string) {
  const ledger = new SchemaFailureLedger()
  const session = new SessionShardValidation(ledger)
  const scan = await scanArchiveRoot(chunks(text), session.options())
  const summary = session.finish(scan)
  ledger.throwIfAny()
  return summary
}
function errorOf(operation: () => unknown): string {
  try { operation() } catch (error) { assert.ok(error instanceof Error); return error.message }
  return "ACCEPTED"
}

test("snapshot/account 与 runtime/strategy 逐行验证后只保留计数", { timeout: 10000 }, async () => {
  const root = fixture()
  const result = await grouped(JSON.stringify(root))
  assert.equal(result.accounts, Object.keys(root.snapshot.accounts).length)
  assert.equal(result.strategies, Object.keys(root.runtime_state.strategy_states).length)
  assert.equal(result.schemaValidated, false)
})

test("snapshot 形状、字段及账户 leaf 首错与完整 codec 相同", { timeout: 10000 }, async () => {
  const valid = fixture()
  for (const value of [null, [], false, 1, {}, { ...valid.snapshot, extra: true }, { ...valid.snapshot, markets: null }, { ...valid.snapshot, accounts: null }, { ...valid.snapshot, accounts: { "2": {} } }, { ...valid.snapshot, seq: -1 }, { ...valid.snapshot, daily_candles: null }, { ...valid.snapshot, active_daily_candles: null }]) {
    const text = JSON.stringify({ ...valid, snapshot: value })
    const expected = errorOf(() => parseSaveJson(text))
    assert.notEqual(expected, "ACCEPTED")
    await assert.rejects(grouped(text), error => error instanceof Error && error.message === expected)
  }
})

test("snapshot accounts 在 seq/tick 前校验，markets 在全部账户前校验", { timeout: 10000 }, async () => {
  const valid = fixture()
  for (const snapshot of [
    { ...valid.snapshot, seq: -1, accounts: { "2": {} } },
    { ...valid.snapshot, markets: null, accounts: { "2": {} } },
    { ...valid.snapshot, accounts: { "20": {}, "2": { cash: "-1", positions: {} } } },
  ]) {
    const text = JSON.stringify({ ...valid, snapshot })
    await assert.rejects(grouped(text), error => error instanceof Error && error.message === errorOf(() => parseSaveJson(text)))
  }
})

test("runtime 头、空容器和 strategy leaf 的严格首错与完整 codec 相同", { timeout: 10000 }, async () => {
  const valid = fixture()
  for (const value of [null, [], false, 1, {}, { ...valid.runtime_state, extra: true }, { ...valid.runtime_state, poisoned: 1 }, { ...valid.runtime_state, next_receipt_base: false }, { ...valid.runtime_state, live_envelopes: [{}] }, { ...valid.runtime_state, book_public_cutoff_sequences: null }, { ...valid.runtime_state, retail_projection_seen: [{}] }, { ...valid.runtime_state, strategy_states: null }, { ...valid.runtime_state, strategy_states: { "2": {} } }]) {
    const text = JSON.stringify({ ...valid, runtime_state: value })
    const expected = errorOf(() => parseSaveJson(text))
    assert.notEqual(expected, "ACCEPTED")
    await assert.rejects(grouped(text), error => error instanceof Error && error.message === expected)
  }
})

test("runtime 内前置字段错误优先于全部 strategy leaf", { timeout: 10000 }, async () => {
  const valid = fixture()
  const text = JSON.stringify({ ...valid, runtime_state: { ...valid.runtime_state, poisoned: 1, strategy_states: { "2": {} } } })
  await assert.rejects(grouped(text), error => error instanceof Error && error.message === errorOf(() => parseSaveJson(text)))
})

test("数字 raw 账户/策略表保留原 map 键错误；严格持仓与精确浮点检查不能漏过", { timeout: 10000 }, async () => {
  const valid = fixture()
  const strategy = valid.runtime_state.strategy_states["1"] as { Momentum: Record<string, unknown> }
  for (const root of [
    { ...valid, snapshot: { ...valid.snapshot, accounts: 1 } },
    { ...valid, runtime_state: { ...valid.runtime_state, strategy_states: 1 } },
    { ...valid, snapshot: { ...valid.snapshot, accounts: { "2": { cash: "1", positions: { "0": { qty: 1, t1_locked: 2, invested_cents: "0", recovered_cents: "0" } } } } } },
    { ...valid, runtime_state: { ...valid.runtime_state, strategy_states: { "1": { Momentum: { ...strategy.Momentum, trend_threshold: "7ff0000000000000" } } } } },
  ]) {
    const text = JSON.stringify(root)
    await assert.rejects(grouped(text), error => error instanceof Error && error.message === errorOf(() => parseSaveJson(text)))
  }
})


test("组合个人表、snapshot/runtime 与 root metadata 仍按完整 root 阶段报错", { timeout: 10000 }, async () => {
  const valid = fixture()
  for (const root of [
    { ...valid, snapshot: { ...valid.snapshot, seq: -1 }, npc_attention: { "1": {} } },
    { ...valid, snapshot: { ...valid.snapshot, accounts: { "2": {} } }, seed: null },
    { ...valid, runtime_state: { ...valid.runtime_state, strategy_states: { "2": {} } }, information_states: { "2": {} } },
    { ...valid, company_state: null, runtime_state: { ...valid.runtime_state, strategy_states: { "2": {} } } },
  ]) {
    const ledger = new SchemaFailureLedger()
    const session = new SessionShardValidation(ledger), personal = new PersonalShardValidation(ledger)
    const sessionOptions = session.options(), personalOptions = personal.options()
    const options = {
      records: [...sessionOptions.records, ...personalOptions.records],
      onRecord: (row: Parameters<typeof sessionOptions.onRecord>[0]) => (row.path[0] === "snapshot" || row.path[0] === "runtime_state" ? sessionOptions : personalOptions).onRecord(row),
      onShape: (shape: Parameters<NonNullable<typeof sessionOptions.onShape>>[0]) => (shape.path[0] === "snapshot" || shape.path[0] === "runtime_state" ? sessionOptions : personalOptions).onShape!(shape),
    }
    const text = JSON.stringify(root)
    const scan = await scanArchiveRoot(chunks(text), options)
    session.finish(scan); personal.finish(); validateRootMetadata(scan, ledger)
    const expected = errorOf(() => parseSaveJson(text))
    assert.throws(() => ledger.throwIfAny(), error => error instanceof Error && error.message === expected)
  }
})
