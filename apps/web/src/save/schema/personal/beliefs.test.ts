import assert from "node:assert/strict"
import test from "node:test"
import { parseBeliefBook } from "./beliefs.ts"
import { parseRawSaveJson } from "../raw-json.ts"
import { materializeSaveValue, SaveSchemaError } from "../primitives.ts"

const base = {
  npc: "1",
  institution_account_risk_paused: false,
  profile: { Institution: "Balanced" },
  analysis: {
    fundamental_bp: 0,
    trend_bp: 2500,
    price_volume_bp: 2500,
    technical_bp: 2500,
    experience_cost_bp: 2500,
    fundamental_method: null,
  },
  assumptions: {
    growth_deviation_bp: 0,
    quality_coefficient_bp: 0,
    pe_multiple: 0,
    equity_cost_bp: 0,
    terminal_growth_bp: 0,
    roe_deviation_bp: 0,
  },
  entries: {},
  experience: {
    reference_equity: null,
    peak_equity: null,
    consecutive_failed_buys: 0,
    stocks: {},
  },
  institution_policy: {
    policy_version: 1,
    loss_response: "HoldOrAdd",
    cost_loss_threshold_bp: 800,
    cost_profit_threshold_bp: 1200,
    risk_pause_drawdown_bp: 3000,
    risk_resume_drawdown_bp: 1500,
    risk_pause_failed_buys: 3,
    adverse_move_threshold_bp: 500,
  },
}

const moment = { civil_date: "2030-01-02", market_minute: "22", trading_day: "1" }

// synthetic：仅定义 Web 输入边界，不冒充 Rust producer 或真实 Calendar 场景。
const syntheticEntry = {
  company: 0, method: null,
  forecast: { growth_bp: null, basis: "InitialWithoutHistory" }, confidence_bp: 3000,
  valuation: { Unavailable: { reason: "MethodDisabled" } }, used_report_ids: [],
  anchor: { kind: "NoPriorSession" }, horizon_trading_days: 1,
  last_cause: { cause: "HorizonExpired", as_of_anchor: { kind: "NoPriorSession" } },
  applied_experience_orders: [],
}
const syntheticBook = (entry: unknown) => ({ ...base, entries: { "0": entry } })

test("typed INFO anchor 三态及完整 u64 字符串不合并、不丢精度（synthetic）", () => {
  for (const anchor of [
    { kind: "NoPriorSession" },
    ...["TradingDay", "DayBefore"].flatMap(kind => ["0", "9007199254740993", "18446744073709551615"].map(ordinal => ({ kind, ordinal }))),
  ]) {
    const book = syntheticBook({ ...syntheticEntry, anchor, last_cause: { cause: "HorizonExpired", as_of_anchor: anchor } })
    assert.deepEqual(parseBeliefBook(book, "book"), book)
    assert.deepEqual(materializeSaveValue(parseBeliefBook(parseRawSaveJson(JSON.stringify(book)), "book")), book)
  }
})

test("typed INFO anchor 旧 scalar 与旧字段分别显式拒绝，不建立 alias（synthetic）", () => {
  const { anchor: _anchor, last_cause: _cause, ...entry } = syntheticEntry
  assert.throws(() => parseBeliefBook(syntheticBook({ ...entry, anchor_trading_day: 0, last_cause: null }), "book"), /anchor/)
  assert.throws(() => parseBeliefBook(syntheticBook({ ...syntheticEntry, anchor: 0 }), "book"), /anchor/)
  assert.throws(() => parseBeliefBook(syntheticBook({ ...syntheticEntry, anchor_trading_day: 0 }), "book"), /anchor_trading_day/)
  assert.throws(() => parseBeliefBook(syntheticBook({ ...syntheticEntry, last_cause: { cause: "HorizonExpired", as_of_trading_day: 0 } }), "book"), /as_of_anchor/)
  assert.throws(() => parseBeliefBook(syntheticBook({ ...syntheticEntry, last_cause: { cause: "HorizonExpired", as_of_anchor: 0 } }), "book"), /as_of_anchor/)
  assert.throws(() => parseBeliefBook(syntheticBook({ ...syntheticEntry, last_cause: { ...syntheticEntry.last_cause, as_of_trading_day: 0 } }), "book"), /as_of_trading_day/)
})

test("typed INFO CauseRecord 的旧 scalar 完整条目独立拒绝（synthetic）", () => {
  const { anchor: _anchor, ...entry } = syntheticEntry
  const legacy = syntheticBook({ ...entry, anchor_trading_day: 0, last_cause: { cause: "HorizonExpired", as_of_trading_day: 0 } })
  assert.throws(() => parseBeliefBook(legacy, "book"), /anchor|as_of_anchor/)
})

test("typed INFO anchor 精确键集、kind 与 canonical ordinal 两字段均严格拒错（synthetic）", () => {
  const invalid = [null, false, true, 0, "NoPriorSession", [], {}, { kind: "Unknown" },
    { kind: "NoPriorSession", ordinal: "0" }, { kind: "NoPriorSession", extra: true },
    { kind: "TradingDay" }, { kind: "DayBefore", ordinal: "0", extra: true },
    ...[null, 0, -0, -1, "", "00", "01", "+0", "-0", "-1", "1.0", "1e0", " 0", "0\n", "０", "١", "18446744073709551616", "999999999999999999999"].flatMap(ordinal => ["TradingDay", "DayBefore"].map(kind => ({ kind, ordinal }))),
  ]
  for (const anchor of invalid) {
    for (const field of ["anchor", "as_of_anchor"] as const) {
      const entry = field === "anchor" ? { ...syntheticEntry, anchor } : { ...syntheticEntry, last_cause: { cause: "HorizonExpired", as_of_anchor: anchor } }
      const expected = field === "anchor" ? "book.entries.0.anchor" : "book.entries.0.last_cause.as_of_anchor"
      assert.throws(() => parseBeliefBook(syntheticBook(entry), "book"), error => {
        assert.ok(error instanceof Error)
        assert.ok(error.message.includes(expected), `必须定位当前字段 ${expected}：${error.message}`)
        if (error instanceof SaveSchemaError) assert.ok(error.path === expected || error.path.startsWith(`${expected}.`))
        return true
      })
    }
  }
})

test("typed INFO anchor 原始 JSON 的重复 kind/ordinal 不经覆盖后进入 decoder（synthetic）", () => {
  const raw = JSON.stringify(syntheticBook(syntheticEntry))
  for (const duplicate of [
    '{"kind":"NoPriorSession","kind":"TradingDay","ordinal":"0"}',
    '{"kind":"TradingDay","ordinal":"0","ordinal":"1"}',
  ]) {
    for (const field of ["anchor", "as_of_anchor"]) {
      const bad = raw.replace(`"${field}":{"kind":"NoPriorSession"}`, `"${field}":${duplicate}`)
      assert.notEqual(bad, raw)
      assert.throws(() => materializeSaveValue(parseBeliefBook(parseRawSaveJson(bad), "book")), /重复 JSON 键/)
    }
  }
})

test("Retail BeliefBook 不携带 Institution policy 或账户暂停状态", () => {
  const retail = { ...base, profile: { Retail: "Noise" }, institution_policy: null }
  assert.deepEqual(parseBeliefBook(retail, "book"), retail)
  assert.throws(() => parseBeliefBook({ ...retail, institution_policy: base.institution_policy }, "book"), /institution_policy/)
  assert.throws(() => parseBeliefBook({ ...retail, institution_account_risk_paused: true }, "book"), /institution_account_risk_paused/)
})

test("institution account risk pause is a required frozen boolean", () => {
  for (const paused of [false, true]) {
    const frozen = { ...base, institution_account_risk_paused: paused }
    assert.deepEqual(parseBeliefBook(frozen, "book"), frozen)
  }
  const { institution_account_risk_paused: removed, ...legacy } = base
  assert.equal(removed, false)
  assert.throws(() => parseBeliefBook(legacy, "book"), /institution_account_risk_paused/)
  for (const invalid of [null, 0, "false"]) {
    assert.throws(() => parseBeliefBook({ ...base, institution_account_risk_paused: invalid }, "book"), /institution_account_risk_paused/)
  }
})

test("belief confidence rejects values above 10000 at the save boundary", () => {
  const entry = {
    company: 0, method: null,
    forecast: { growth_bp: null, basis: "InitialWithoutHistory" },
    valuation: { Unavailable: { reason: "MethodDisabled" } },
    used_report_ids: [], anchor: { kind: "NoPriorSession" }, horizon_trading_days: 1,
    last_cause: null, applied_experience_orders: [],
  }
  const withConfidence = (confidence: number) => ({
    ...base, entries: { "0": { ...entry, confidence_bp: confidence } },
  })
  for (const confidence of [0, 10_000]) {
    assert.equal(parseBeliefBook(withConfidence(confidence), "book").entries["0"].confidence_bp, confidence)
  }
  for (const confidence of [10_001, 65_535]) {
    assert.throws(() => parseBeliefBook(withConfidence(confidence), "book"), /confidence_bp/)
  }
})
const withExitFact = {
  ...base,
  experience: {
    ...base.experience,
    feedback: {
      latest_moment: moment,
      failure_events: [],
      stocks: {},
      exit_records: [{
        code: 0,
        order_id: "9",
        cooldown_until_market_minute: null,
        realized_profit: true,
        moment,
      }],
    },
  },
}

test("holding fee history is a required nullable nonnegative fact", () => {
  const epoch = { entry_moment: moment, last_own_observation: null, institutional_fees_paid: "77" }
  const withEpoch = (value: unknown) => ({
    ...withExitFact,
    experience: { ...withExitFact.experience, feedback: {
      ...withExitFact.experience.feedback, stocks: { "600001": value },
    } },
  })
  assert.equal(parseBeliefBook(withEpoch(epoch), "book").experience.feedback!.stocks["600001"].institutional_fees_paid, "77")
  assert.equal(parseBeliefBook(withEpoch({ ...epoch, institutional_fees_paid: null }), "book").experience.feedback!.stocks["600001"].institutional_fees_paid, null)
  assert.throws(() => parseBeliefBook(withEpoch({ entry_moment: moment, last_own_observation: null }), "book"), /institutional_fees_paid/)
  assert.throws(() => parseBeliefBook(withEpoch({ ...epoch, institutional_fees_paid: "-1" }), "book"), /institutional_fees_paid/)
})

test("institution belief book requires its independent experience state", () => {
  assert.deepEqual(parseBeliefBook(base, "belief_book").experience, base.experience)
  const { experience: removedExperience, ...withoutExperience } = base
  assert.equal(removedExperience.reference_equity, null)
  assert.throws(() => parseBeliefBook(withoutExperience, "belief_book"), /experience/)
})

test("institution experience strictly parses dated receipt facts and exit order ids", () => {
  assert.deepEqual(
    parseBeliefBook(withExitFact, "belief_book").experience,
    withExitFact.experience,
  )

  const [exitFact] = withExitFact.experience.feedback.exit_records
  const { order_id: removedOrderId, ...exitWithoutOrderId } = exitFact
  assert.equal(removedOrderId, "9")
  const missingOrderId = {
    ...withExitFact,
    experience: {
      ...withExitFact.experience,
      feedback: {
        ...withExitFact.experience.feedback,
        exit_records: [exitWithoutOrderId],
      },
    },
  }
  assert.throws(() => parseBeliefBook(missingOrderId, "belief_book"), /order_id/)

  const invalidOrderId = {
    ...withExitFact,
    experience: {
      ...withExitFact.experience,
      feedback: {
        ...withExitFact.experience.feedback,
        exit_records: [{ ...exitFact, order_id: "not-an-order-id" }],
      },
    },
  }
  assert.throws(() => parseBeliefBook(invalidOrderId, "belief_book"), /order_id/)
})
