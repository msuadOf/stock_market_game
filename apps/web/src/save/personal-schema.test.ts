import assert from "node:assert/strict"
import test from "node:test"
import { parseBeliefBooks } from "./schema/personal/beliefs.ts"
import { parseRetailExperienceStates } from "./schema/personal/experience.ts"
import { parseInformationStates } from "./schema/personal/information.ts"
import { parsePriceMemories, parseWatchlists } from "./schema/personal/memory.ts"
import { parsePendingPlanEvents, parsePlanBook } from "./schema/personal/plans.ts"

const information = { "1": { owner: 1, companies: { "C-600101": [{ id: 7, observed_at: { date: "2030-02-01", second_of_day: 64_800 }, kind: "Report" }] } } }
const belief = { "1": { npc: 1, profile: { Institution: "Growth" }, analysis: { fundamental_bp: 4_000, trend_bp: 2_000, price_volume_bp: 1_000, technical_bp: 2_000, experience_cost_bp: 1_000, fundamental_method: "cash_flow" }, assumptions: { growth_deviation_bp: 10, quality_coefficient_bp: 10_000, pe_multiple: 14, equity_cost_bp: 1_200, terminal_growth_bp: 200, roe_deviation_bp: 20 }, entries: { "600101": { company: "C-600101", method: "cash_flow", forecast: { growth_bp: 500, basis: { Revised: { observed_bp: 450 } } }, confidence_bp: 6_000, valuation: { Available: { total_equity_estimate: "1234.56", per_share: { pessimistic: 900, optimistic: 1_100 } } }, used_report_ids: [7], anchor_trading_day: 3, horizon_trading_days: 20, last_cause: { cause: { NewMaterial: { report: 7 } }, as_of_trading_day: 3 }, applied_experience_orders: [17] } } } }
const planBook = { policy: { reverse_revision_threshold_bp: 2_000, review_signal_delta_bp: 1_000, review_price_change_bp: 200 }, next_plan_seq: 2, plans: { "1": { plan_id: 1, account: 1, code: "600101", direction: "Buy", target: { ShareCount: 500 }, filled_qty: 100, opinion: { signal_score_bp: 2_500, source: "Blended" }, confidence_bp: 6_000, urgency: "Normal", status: { Paused: { reason: "RiskPressure" } }, version: 2, last_revision: { version: 2, reason: "NewInformation", trading_day: 3 }, last_resume: "OpinionReaffirmed", created_trading_day: 1, horizon_trading_days: 20, review: { min_signal_delta_bp: 1_000, min_price_change_bp: 200, last_review_signal_score_bp: 2_500, last_review_trading_day: 3 }, active_child_order_id: 4, last_event_trading_day: 3 } } }
const watchlists = { "1": { stocks: { "600101": { last_observed_market_minute: "50" } }, latest_attention_minute: "50" } }
const memories = { "1": { stocks: { "600101": { first_observed_minute: "10", first_observed_price: 1_000, last_observed_minute: "40", last_observed_price: 1_100, observed_high: 1_200, observed_low: 900, last_public_history_read_minute: "50", public_history_read_count: 2, last_touched_minute: "50" } } } }
const experience = { "1": { reference_equity: 100_000, peak_equity: 120_000, consecutive_failed_buys: 1, stocks: { "600101": { entry_reference_price: 1_000, peak_price_since_entry: 1_200, last_buy_price: 1_000, adverse_move_recorded: false, last_buy_order_id: "9", last_sell_order_id: null, last_trade_market_minute: "10", last_observed_market_minute: "20", cooldown_until_market_minute: null } }, feedback: { latest_moment: { civil_date: "2030-02-01", market_minute: "20", trading_day: "3" }, failure_events: [{ code: "600101", order_id: "9", moment: { civil_date: "2030-02-01", market_minute: "20", trading_day: "3" } }], stocks: { "600101": { entry_moment: { civil_date: "2030-01-30", market_minute: "10", trading_day: "1" }, last_own_observation: { price: 1_100, moment: { civil_date: "2030-02-01", market_minute: "20", trading_day: "3" } } } }, exit_records: [{ code: "600102", cooldown_until_market_minute: "50", realized_profit: true, moment: { civil_date: "2030-02-01", market_minute: "30", trading_day: "3" } }] } } }
const pending = [{ Accepted: { plan_id: 1, order_id: 4, trading_day: 3 } }, { Filled: { plan_id: 1, order_id: 4, qty: 100, trading_day: 3 } }, { DayEnded: { plan_id: 1, trading_day: 3 } }]

test("personal parsers preserve populated K7 state exactly", () => {
  assert.deepEqual(parseInformationStates(information), information)
  assert.deepEqual(parseBeliefBooks(belief), belief)
  assert.deepEqual(parsePlanBook(planBook), planBook)
  assert.deepEqual(parseWatchlists(watchlists), watchlists)
  assert.deepEqual(parsePriceMemories(memories), memories)
  assert.deepEqual(parseRetailExperienceStates(experience), experience)
  assert.deepEqual(parsePendingPlanEvents(pending), pending)
})

test("information and belief parsers reject malformed exact fields and tags", () => {
  assert.throws(() => parseInformationStates({ "1": { owner: 1, companies: {}, extra: true } }), /information_states\.1\.extra/)
  assert.throws(() => parseInformationStates({ account: information["1"] }), /information_states\.account/)
  assert.throws(() => parseBeliefBooks({ "1": { ...belief["1"], entries: { "600101": { ...belief["1"].entries["600101"], forecast: { growth_bp: 1, basis: "Unknown" } } } } }), /forecast\.basis/)
  assert.throws(() => parseBeliefBooks({ "1": { ...belief["1"], entries: { "600101": { ...belief["1"].entries["600101"], valuation: { Unavailable: { reason: "Unknown" } } } } } }), /valuation\.Unavailable\.reason/)
  assert.throws(() => parseBeliefBooks({ "1": { ...belief["1"], entries: { "600101": { ...belief["1"].entries["600101"], last_cause: { cause: { Unknown: {} }, as_of_trading_day: "3" } } } } }), /last_cause\.cause\.Unknown/)
})

test("plan parsers reject bad keys and every pending-event discriminant", () => {
  assert.throws(() => parsePlanBook({ ...planBook, plans: { bad: planBook.plans["1"] } }), /plans\.plans\.bad/)
  assert.throws(() => parsePlanBook({ ...planBook, plans: { "1": { ...planBook.plans["1"], target: { Unknown: 1 } } } }), /target\.Unknown/)
  assert.throws(() => parsePlanBook({ ...planBook, plans: { "1": { ...planBook.plans["1"], status: { Paused: { reason: "Unknown" } } } } }), /status\.Paused\.reason/)
  assert.throws(() => parsePendingPlanEvents([{ Accepted: { plan_id: 1, order_id: 4, trading_day: 3, extra: true } }]), /Accepted\.extra/)
  assert.throws(() => parsePendingPlanEvents([{ Filled: { plan_id: 1, order_id: 4, qty: "100", trading_day: 3 } }]), /Filled\.qty/)
  assert.throws(() => parsePendingPlanEvents([{ DayEnded: { plan_id: 1, trading_day: -1 } }]), /DayEnded\.trading_day/)
  assert.throws(() => parsePendingPlanEvents([{ Rejected: {} }]), /Rejected/)
})

test("personal memory and feedback reject malformed identifiers, dates, and decimal minutes", () => {
  assert.throws(() => parseWatchlists({ "1": { ...watchlists["1"], stocks: { BAD: watchlists["1"].stocks["600101"] } } }), /watchlists\.1\.stocks\.BAD/)
  assert.throws(() => parsePriceMemories({ "1": { stocks: { "600101": { ...memories["1"].stocks["600101"], last_touched_minute: 50 } } } }), /last_touched_minute/)
  assert.throws(() => parseRetailExperienceStates({ "1": { ...experience["1"], feedback: { ...experience["1"].feedback, latest_moment: { civil_date: "2030-02-30", market_minute: "20", trading_day: "3" } } } }), /civil_date/)
  assert.throws(() => parseRetailExperienceStates({ "1": { ...experience["1"], feedback: { ...experience["1"].feedback, exit_records: [{ ...experience["1"].feedback.exit_records[0], realized_profit: "yes" }] } } }), /realized_profit/)
  assert.throws(() => parsePriceMemories({ "1": { stocks: { "600101": { ...memories["1"].stocks["600101"], last_touched_minute: "18446744073709551616" } } } }), /last_touched_minute/)
  assert.throws(() => parseInformationStates({ "1": { ...information["1"], companies: { "C-600101": [{ ...information["1"].companies["C-600101"][0], observed_at: { date: "2030-02-01", second_of_day: 86_400 } }] } } }), /second_of_day/)
})
