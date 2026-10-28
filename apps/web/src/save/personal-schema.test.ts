import assert from "node:assert/strict"
import test from "node:test"
import { parseBeliefBooks } from "./schema/personal/beliefs.ts"
import { parseRetailExperienceStates } from "./schema/personal/experience.ts"
import { parseInformationStates } from "./schema/personal/information.ts"
import { parseHistoryReads, parsePriceMemories, parseWatchlists } from "./schema/personal/memory.ts"
import { parsePlanBook } from "./schema/personal/plans.ts"

const information = { "1": { owner: "1", companies: { "0": [{ id: 7, observed_at: { date: "2030-02-01", second_of_day: 64_800 }, kind: "Report" }] } } }
const belief = { "1": { npc: "1", institution_account_risk_paused: false, profile: { Institution: "Growth" }, analysis: { fundamental_bp: 4_000, trend_bp: 2_000, price_volume_bp: 1_000, technical_bp: 2_000, experience_cost_bp: 1_000, fundamental_method: "cash_flow" }, assumptions: { growth_deviation_bp: 10, quality_coefficient_bp: 10_000, pe_multiple: 14, equity_cost_bp: 1_200, terminal_growth_bp: 200, roe_deviation_bp: 20 }, entries: { "3": { company: 0, method: "cash_flow", forecast: { growth_bp: 500, basis: { Revised: { observed_bp: 450 } } }, confidence_bp: 6_000, valuation: { Available: { total_equity_estimate: "1234.56", per_share: { pessimistic: "900", optimistic: "1100" } } }, used_report_ids: [7], anchor: { kind: "TradingDay", ordinal: "3" }, horizon_trading_days: 20, last_cause: { cause: { NewMaterial: { report: 7 } }, as_of_anchor: { kind: "TradingDay", ordinal: "3" } }, applied_experience_orders: [17] } }, experience: { reference_equity: null, peak_equity: null, consecutive_failed_buys: 0, stocks: {} }, institution_policy: { policy_version: 1, loss_response: "HoldOrAdd", cost_loss_threshold_bp: 800, cost_profit_threshold_bp: 1200, risk_pause_drawdown_bp: 3000, risk_resume_drawdown_bp: 1500, risk_pause_failed_buys: 3, adverse_move_threshold_bp: 500 } } }
const planBook = { policy: { reverse_revision_threshold_bp: 2_000, review_signal_delta_bp: 1_000, review_price_change_bp: 200 }, owner_sequences: [{ owner: "1", next_sequence: "2" }], plans: [{ Investor: { plan_id: { owner: "1", sequence: "1" }, account: "1", code: 3, direction: "Buy", target: { ShareCount: 500 }, filled_qty: 100, opinion: { signal_score_bp: 2_500, source: "Blended" }, confidence_bp: 6_000, urgency: "Normal", status: { Paused: { reason: "RiskPressure" } }, version: 2, last_revision: { version: 2, reason: "NewInformation", trading_day: 3 }, last_resume: "OpinionReaffirmed", created_trading_day: 1, horizon_trading_days: 20, review: { min_signal_delta_bp: 1_000, min_price_change_bp: 200, last_review_signal_score_bp: 2_500, last_review_trading_day: 3, last_review_price: "1000", last_review_acquired_count: 0, last_review_resources: null }, active_child_order_id: 4, execution: { child_qty: 200, limit_price: "1000", expires_market_minute: 500, active_child_remaining_qty: 100 }, last_event_trading_day: 3 } }] }
const watchlists = { "1": { stocks: { "3": { last_observed_market_minute: "50" } }, latest_attention_minute: "50" } }
const memories = { "1": { stocks: { "3": { first_observed_minute: "10", first_observed_price: "1000", last_observed_minute: "40", last_observed_price: "1100", observed_high: "1200", observed_low: "900", last_touched_minute: "50" } } } }
const historyReads = { "0": { stocks: { "3": { last_read_market_minute: "50", read_count: 2 } } }, "1": { stocks: { "3": { last_read_market_minute: "50", read_count: 1 } } } }
const experience = { "1": { reference_equity: "100000", peak_equity: "120000", consecutive_failed_buys: 1, stocks: { "3": { entry_reference_price: "1000", peak_price_since_entry: "1200", last_buy_price: "1000", adverse_move_recorded: false, last_buy_order_id: "9", last_sell_order_id: null, last_trade_market_minute: "10", last_observed_market_minute: "20", cooldown_until_market_minute: null } }, feedback: { latest_moment: { civil_date: "2030-02-01", market_minute: "20", trading_day: "3" }, failure_events: [{ code: 3, order_id: "9", moment: { civil_date: "2030-02-01", market_minute: "20", trading_day: "3" } }], stocks: { "3": { entry_moment: { civil_date: "2030-01-30", market_minute: "10", trading_day: "1" }, institutional_fees_paid: null, last_own_observation: { price: "1100", moment: { civil_date: "2030-02-01", market_minute: "20", trading_day: "3" } } } }, exit_records: [{ code: 600102, order_id: "10", cooldown_until_market_minute: "50", realized_profit: true, moment: { civil_date: "2030-02-01", market_minute: "30", trading_day: "3" } }] } } }

test("plan review preserves historical resources and rejects missing or impossible facts", () => {
  const populated = {
    ...planBook,
    plans: [{ Investor: { ...planBook.plans[0]!.Investor, review: {
      ...planBook.plans[0]!.Investor.review,
      last_review_resources: { cash: "10000", frozen_cash: "1000", held_qty: 200, t1_locked: 100 },
    } } }],
  }
  assert.deepEqual(parsePlanBook(populated), populated)
  const missing = structuredClone(populated)
  delete (missing.plans[0]!.Investor.review as Record<string, unknown>).last_review_resources
  assert.throws(() => parsePlanBook(missing), /last_review_resources/)
  for (const resources of [
    { cash: "1000", frozen_cash: "1001", held_qty: 100, t1_locked: 0 },
    { cash: "1000", frozen_cash: "0", held_qty: 100, t1_locked: 101 },
  ]) {
    const invalid = structuredClone(populated)
    invalid.plans[0]!.Investor.review.last_review_resources = resources
    assert.throws(() => parsePlanBook(invalid), /资源事实/)
  }
})

test("personal parsers preserve populated simulation acceptance state exactly", () => {
  assert.deepEqual(parseInformationStates(information), information)
  assert.deepEqual(parseBeliefBooks(belief), belief)
  assert.deepEqual(parsePlanBook(planBook), planBook)
  assert.deepEqual(parseWatchlists(watchlists), watchlists)
  assert.deepEqual(parsePriceMemories(memories), memories)
  assert.deepEqual(parseHistoryReads(historyReads), historyReads)
  assert.deepEqual(parseRetailExperienceStates(experience), experience)
})

test("BeliefBook 保存合并现金流归母归属不足的显式原因", { timeout: 10_000 }, () => {
  const saved = { "1": { ...belief["1"], entries: { "3": {
    ...belief["1"].entries["3"],
    valuation: { Unavailable: { reason: "ConsolidatedCashFlowAttributionUnavailable" } },
  } } } }
  assert.deepEqual(parseBeliefBooks(saved), saved)
  const missingIncome = { "1": { ...saved["1"], entries: { "3": {
    ...saved["1"].entries["3"], valuation: { Unavailable: { reason: "ConsolidatedNetIncomeAttributionUnavailable" } },
  } } } }
  assert.deepEqual(parseBeliefBooks(missingIncome), missingIncome)
  assert.throws(() => parseBeliefBooks({ "1": { ...saved["1"], entries: { "3": {
    ...saved["1"].entries["3"], valuation: { Unavailable: { reason: "UnattributedDefault" } },
  } } } }), /valuation\.Unavailable\.reason/)
})

test("information and belief parsers reject malformed exact fields and tags", () => {
  assert.throws(() => parseInformationStates({ "1": { owner: "1", companies: {}, extra: true } }), /information_states\.1\.extra/)
  assert.throws(() => parseInformationStates({ account: information["1"] }), /information_states\.account/)
  assert.throws(() => parseBeliefBooks({ "1": { ...belief["1"], entries: { "3": { ...belief["1"].entries["3"], forecast: { growth_bp: 1, basis: "Unknown" } } } } }), /forecast\.basis/)
  assert.throws(() => parseBeliefBooks({ "1": { ...belief["1"], entries: { "3": { ...belief["1"].entries["3"], valuation: { Unavailable: { reason: "Unknown" } } } } } }), /valuation\.Unavailable\.reason/)
  assert.throws(() => parseBeliefBooks({ "1": { ...belief["1"], entries: { "3": { ...belief["1"].entries["3"], last_cause: { cause: { Unknown: {} }, as_of_anchor: { kind: "TradingDay", ordinal: "3" } } } } } }), /last_cause\.cause\.Unknown/)
})

test("plan parsers reject bad keys and non-canonical execution legs", () => {
  assert.throws(() => parsePlanBook({ ...planBook, plans: { bad: planBook.plans[0]!.Investor } }), /plans\.plans/)
  assert.throws(() => parsePlanBook({ ...planBook, plans: [{ Investor: { ...planBook.plans[0]!.Investor, target: { Unknown: 1 } } }] }), /target\.Unknown/)
  assert.throws(() => parsePlanBook({ ...planBook, plans: [{ Investor: { ...planBook.plans[0]!.Investor, status: { Paused: { reason: "Unknown" } } } }] }), /status\.Paused\.reason/)
  // 执行腿是可选键：显式 null 或注入未知字段都不是 Rust 序列化会产出的形态。
  assert.throws(() => parsePlanBook({ ...planBook, plans: [{ Investor: { ...planBook.plans[0]!.Investor, execution: null } }] }), /execution/)
  assert.throws(() => parsePlanBook({ ...planBook, plans: [{ Investor: { ...planBook.plans[0]!.Investor, execution: { ...planBook.plans[0]!.Investor.execution, extra: true } } }] }), /execution\.extra/)
  assert.throws(() => parsePlanBook({ ...planBook, plans: [{ Investor: { ...planBook.plans[0]!.Investor, execution: { ...planBook.plans[0]!.Investor.execution, child_qty: 0 } } }] }), /execution\.child_qty/)
  assert.throws(() => parsePlanBook({ ...planBook, plans: [{ Investor: { ...planBook.plans[0]!.Investor, execution: { ...planBook.plans[0]!.Investor.execution, limit_price: 1000 } } }] }), /execution\.limit_price/)
  assert.throws(() => parsePlanBook({ ...planBook, plans: [{ Investor: { ...planBook.plans[0]!.Investor, execution: { ...planBook.plans[0]!.Investor.execution, expires_market_minute: "500" } } }] }), /execution\.expires_market_minute/)
  assert.throws(() => parsePlanBook({ ...planBook, plans: [{ Investor: { ...planBook.plans[0]!.Investor, execution: { ...planBook.plans[0]!.Investor.execution, active_child_remaining_qty: "100" } } }] }), /execution\.active_child_remaining_qty/)
})

test("personal memory and feedback reject malformed identifiers, dates, and decimal minutes", () => {
  assert.throws(() => parseWatchlists({ "1": { ...watchlists["1"], stocks: { BAD: watchlists["1"].stocks["3"] } } }), /watchlists\.1\.stocks\.BAD/)
  assert.throws(() => parsePriceMemories({ "1": { stocks: { "3": { ...memories["1"].stocks["3"], last_touched_minute: 50 } } } }), /last_touched_minute/)
  assert.throws(() => parseRetailExperienceStates({ "1": { ...experience["1"], feedback: { ...experience["1"].feedback, latest_moment: { civil_date: "2030-02-30", market_minute: "20", trading_day: "3" } } } }), /civil_date/)
  assert.throws(() => parseRetailExperienceStates({ "1": { ...experience["1"], feedback: { ...experience["1"].feedback, exit_records: [{ ...experience["1"].feedback.exit_records[0], realized_profit: "yes" }] } } }), /realized_profit/)
  assert.throws(() => parsePriceMemories({ "1": { stocks: { "3": { ...memories["1"].stocks["3"], last_touched_minute: "18446744073709551616" } } } }), /last_touched_minute/)
  assert.throws(() => parseInformationStates({ "1": { ...information["1"], companies: { "0": [{ ...information["1"].companies["0"][0], observed_at: { date: "2030-02-01", second_of_day: 86_400 } }] } } }), /second_of_day/)
})


test("PlanBook 无损保留各 owner 的 sequence、历史游标与耗尽状态", { timeout: 10_000 }, () => {
  const original = planBook.plans[0]!.Investor
  const saved = {
    policy: planBook.policy,
    owner_sequences: [
      { owner: "1", next_sequence: "9007199254740994" },
      { owner: "18446744073709551615", next_sequence: null },
      { owner: "2", next_sequence: "0" },
    ],
    plans: [
      { Investor: { ...original, plan_id: { owner: "1", sequence: "9007199254740993" } } },
      { Investor: { ...original, account: "18446744073709551615", plan_id: { owner: "18446744073709551615", sequence: "18446744073709551615" } } },
    ],
  }
  assert.deepEqual(parsePlanBook(saved), saved)
  const sharedSequence = {
    ...planBook,
    owner_sequences: [{ owner: "1", next_sequence: "1" }, { owner: "2", next_sequence: "1" }],
    plans: [
      { Investor: { ...original, plan_id: { owner: "1", sequence: "0" } } },
      { Investor: { ...original, account: "2", plan_id: { owner: "2", sequence: "0" } } },
    ],
  }
  assert.deepEqual(parsePlanBook(sharedSequence), sharedSequence)
})

test("PlanBook 严格拒绝旧格式、非规范身份与缺省游标字段", { timeout: 10_000 }, () => {
  const original = planBook.plans[0]!.Investor
  for (const invalid of [0, "", "01", "-0", "+1", "-1", "1.0", "1e3", " 1", "1\n", "18446744073709551616"]) {
    for (const field of ["owner", "sequence"] as const) {
      assert.throws(() => parsePlanBook({ ...planBook, plans: [{ Investor: { ...original, plan_id: { ...original.plan_id, [field]: invalid } } }] }), new RegExp(`plan_id\\.${field}`))
    }
    assert.throws(() => parsePlanBook({ ...planBook, owner_sequences: [{ owner: invalid, next_sequence: "2" }] }), /owner_sequences\[0\]\.owner/)
    assert.throws(() => parsePlanBook({ ...planBook, owner_sequences: [{ owner: "1", next_sequence: invalid }] }), /next_sequence/)
    assert.throws(() => parsePlanBook({ ...planBook, plans: [{ Investor: { ...original, account: invalid } }] }), /account/)
  }
  for (const id of [1, "1", {}, { owner: "1" }, { sequence: "1" }, { ...original.plan_id, extra: true }]) {
    assert.throws(() => parsePlanBook({ ...planBook, plans: [{ Investor: { ...original, plan_id: id } }] }), /plan_id/)
  }
  for (const owner_sequences of [undefined, {}, [{ owner: "1" }], [{ owner: "1", next_sequence: "2", extra: true }]]) {
    assert.throws(() => parsePlanBook({ ...planBook, owner_sequences }), /owner_sequences/)
  }
  const { owner_sequences: _removed, ...missing } = planBook
  assert.throws(() => parsePlanBook(missing), /owner_sequences.*必填/)
  assert.throws(() => parsePlanBook({ policy: planBook.policy, next_plan_seq: 2, plans: { "1": original } }), /owner_sequences/)
  assert.throws(() => parsePlanBook({ ...planBook, next_plan_seq: 2 }), /next_plan_seq/)
})

test("PlanBook 拒绝重复身份、重复 owner、owner 漂移与回退游标", { timeout: 10_000 }, () => {
  const original = planBook.plans[0]!.Investor
  assert.throws(() => parsePlanBook({ ...planBook, owner_sequences: [...planBook.owner_sequences, ...planBook.owner_sequences] }), /重复.*owner/)
  assert.throws(() => parsePlanBook({ ...planBook, plans: [{ Investor: original }, { Investor: original }] }), /重复.*PlanId/)
  assert.throws(() => parsePlanBook({ ...planBook, plans: [{ Investor: { ...original, account: "2" } }] }), /owner.*account/)
  assert.throws(() => parsePlanBook({ ...planBook, owner_sequences: [] }), /owner_sequences.*缺少/)
  for (const next_sequence of ["0", "1"]) {
    assert.throws(() => parsePlanBook({ ...planBook, owner_sequences: [{ owner: "1", next_sequence }] }), /next_sequence.*大于/)
  }
  const nextPlan = { ...original, plan_id: { owner: "1", sequence: "2" } }
  assert.throws(() => parsePlanBook({ ...planBook, owner_sequences: [{ owner: "1", next_sequence: "3" }], plans: [{ Investor: original }, { Investor: nextPlan }] }), /非终止计划/)
  const terminal = { ...planBook, owner_sequences: [{ owner: "1", next_sequence: "10" }], plans: [{ Investor: { ...original, status: "Completed" } }, { Investor: nextPlan }] }
  assert.deepEqual(parsePlanBook(terminal), terminal)
})
