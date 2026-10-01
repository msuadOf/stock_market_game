import assert from "node:assert/strict"
import test from "node:test"
import { parseBeliefBook } from "./beliefs.ts"

const base = {
  npc: 1,
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
const withExitFact = {
  ...base,
  experience: {
    ...base.experience,
    feedback: {
      latest_moment: moment,
      failure_events: [],
      stocks: {},
      exit_records: [{
        code: "600001",
        order_id: "9",
        cooldown_until_market_minute: null,
        realized_profit: true,
        moment,
      }],
    },
  },
}

test("holding fee history is a required nullable nonnegative fact", () => {
  const epoch = { entry_moment: moment, last_own_observation: null, institutional_fees_paid: 77 }
  const withEpoch = (value: unknown) => ({
    ...withExitFact,
    experience: { ...withExitFact.experience, feedback: {
      ...withExitFact.experience.feedback, stocks: { "600001": value },
    } },
  })
  assert.equal(parseBeliefBook(withEpoch(epoch), "book").experience.feedback!.stocks["600001"].institutional_fees_paid, 77)
  assert.equal(parseBeliefBook(withEpoch({ ...epoch, institutional_fees_paid: null }), "book").experience.feedback!.stocks["600001"].institutional_fees_paid, null)
  assert.throws(() => parseBeliefBook(withEpoch({ entry_moment: moment, last_own_observation: null }), "book"), /institutional_fees_paid/)
  assert.throws(() => parseBeliefBook(withEpoch({ ...epoch, institutional_fees_paid: -1 }), "book"), /institutional_fees_paid/)
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
