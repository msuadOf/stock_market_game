import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import { createHash } from "node:crypto"
import test from "node:test"
import { currentSaveFixture } from "./current-save-fixture.ts"
import { parseSetup } from "./schema/market.ts"
import { parseSaveSlot } from "./save-schema.ts"
import { parseInformationStates } from "./schema/personal/information.ts"
import { parseBeliefBooks, type BeliefBook } from "./schema/personal/beliefs.ts"
import { parseRawSaveJson } from "./schema/raw-json.ts"
import { materializeSaveValue } from "./schema/primitives.ts"

// 从真实 Rust producer 原档提取，字段和值均未投影；来源与原档哈希见工作记录。
const fragments = JSON.parse(readFileSync(new URL("./fixtures/current-producer-fragments.json", import.meta.url), "utf8")) as Record<string, unknown>

// synthetic：当前身份 codec 的独立正控，不改写历史 Rust fragment 冒充新 producer。
const syntheticBelief = {
  npc: "3", profile: { Retail: "Noise" }, institution_account_risk_paused: false,
  analysis: { fundamental_bp: 0, trend_bp: 2500, price_volume_bp: 2500, technical_bp: 2500, experience_cost_bp: 2500, fundamental_method: null },
  assumptions: { growth_deviation_bp: 0, quality_coefficient_bp: 0, pe_multiple: 0, equity_cost_bp: 0, terminal_growth_bp: 0, roe_deviation_bp: 0 },
  entries: { "0": { company: 0, method: null, forecast: { growth_bp: null, basis: "InitialWithoutHistory" }, confidence_bp: 3000, valuation: { Unavailable: { reason: "MethodDisabled" } }, used_report_ids: [], anchor: { kind: "NoPriorSession" }, horizon_trading_days: 1, last_cause: null, applied_experience_orders: [] } },
  experience: { reference_equity: null, peak_equity: null, consecutive_failed_buys: 0, stocks: {} }, institution_policy: null,
} satisfies BeliefBook

test("当前存档严格保留必填 desk_mirrored_resting，不混入默认值", { timeout: 10_000 }, () => {
  const current = currentSaveFixture()
  for (const desk_mirrored_resting of [false, true]) {
    const saved = { ...current, desk_mirrored_resting }
    assert.deepEqual(parseSaveSlot(saved), saved)
  }
  const { desk_mirrored_resting: _removed, ...missing } = current
  assert.throws(() => parseSaveSlot(missing), /desk_mirrored_resting.*必填/)
  for (const invalid of [0, "false", null, undefined]) {
    assert.throws(() => parseSaveSlot({ ...current, desk_mirrored_resting: invalid }), /desk_mirrored_resting/)
  }
})

test("真实 InformationState 保留身份；历史 Rust BeliefBook scalar 明确拒绝", { timeout: 10_000 }, () => {
  assert.deepEqual(parseInformationStates({ "1": fragments.information_state }), { "1": fragments.information_state })
  assert.throws(() => parseBeliefBooks({ "3": fragments.belief_book }), /belief_books\.3\.entries\.0\.anchor/)
  assert.deepEqual(parseBeliefBooks({ "3": syntheticBelief }), { "3": syntheticBelief })
})

test("真实 Rust 三场景完整非空 Book 保留 typed INFO anchor 与 Cause", { timeout: 10_000 }, () => {
  const bytes = readFileSync(new URL("./fixtures/typed-info/rust-three-belief-books.json", import.meta.url))
  assert.equal(createHash("sha256").update(bytes).digest("hex"), "2351428e962a56c9becadc2b0e4817d809ec9597df8c89220c32b63ecf9a5ec6")
  const rows = JSON.parse(bytes.toString("utf8")) as { scenario: string; book: BeliefBook; anchor: unknown }[]
  assert.deepEqual(rows.map(row => row.scenario), ["no-prior", "trading-zero", "day-before-zero"])
  assert.deepEqual(rows.map(row => row.anchor), [{ kind: "NoPriorSession" }, { kind: "TradingDay", ordinal: "0" }, { kind: "DayBefore", ordinal: "0" }])
  for (const { book, anchor } of rows) {
    assert.deepEqual(parseBeliefBooks({ [book.npc]: book }), { [book.npc]: book })
    assert.deepEqual(materializeSaveValue(parseBeliefBooks(parseRawSaveJson(JSON.stringify({ [book.npc]: book })))), { [book.npc]: book })
    const entries = Object.values(book.entries)
    assert.ok(entries.length > 0)
    for (const entry of entries) {
      assert.deepEqual(entry.anchor, anchor)
      assert.ok(entry.last_cause !== null)
      assert.deepEqual(entry.last_cause.as_of_anchor, anchor)
      assert.ok(entry.used_report_ids.length > 0)
    }
  }
})

test("个人存档拒绝非规范 AccountId、未知字段、错误 owner 和非数值 CompanyId", { timeout: 10_000 }, () => {
  const information = fragments.information_state as Record<string, unknown>
  const belief = syntheticBelief
  for (const invalid of [1, "01", "+1", "-1", "18446744073709551616"]) {
    assert.throws(() => parseInformationStates({ "1": { ...information, owner: invalid } }), /owner/)
    assert.throws(() => parseBeliefBooks({ "3": { ...belief, npc: invalid } }), /npc/)
  }
  assert.throws(() => parseInformationStates({ "1": { ...information, owner: "2" } }), /owner/)
  assert.throws(() => parseBeliefBooks({ "3": { ...belief, npc: "2" } }), /npc/)
  assert.throws(() => parseInformationStates({ "1": { ...information, extra: true } }), /extra/)
  const entries = belief.entries
  for (const company of ["0", -1, 4_294_967_296, null]) {
    assert.throws(() => parseBeliefBooks({ "3": { ...belief, entries: { "0": { ...entries["0"], company } } } }), /company/)
  }
})


test("parseSetup 接入完整公司选项且保留独立 report_frequency", { timeout: 10_000 }, () => {
  const base = currentSaveFixture().setup as Record<string, unknown>
  const setup = {
    ...base,
    company_system: { mode: "Simulation" }, dividend_tax_mode: "FlatWithholding", flat_withholding_bp: 1_000,
    rights_offering_enabled: true, issuer_repurchase_enabled: true, auto_corporate_foundation: true,
    par_value_per_share: "9007199254740993", report_frequency: { Monthly: { schedule: { Custom: { day: 15, second_of_day: 64_800, delay: "None" } } } },
  }
  assert.deepEqual(parseSetup(setup, "setup"), setup)
  assert.deepEqual(parseSetup({ ...base, report_frequency: "Quarterly" }, "setup"), { ...base, report_frequency: "Quarterly" })
  assert.throws(() => parseSetup({ ...setup, company_system: { mode: "Simulation", extra: true } }, "setup"), /extra/)
  assert.throws(() => parseSetup({ ...setup, dividend_tax_mode: "Legacy" }, "setup"), /dividend_tax_mode/)
  assert.throws(() => parseSetup({ ...setup, extra: true }, "setup"), /extra/)
})
