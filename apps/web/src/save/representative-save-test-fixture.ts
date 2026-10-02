import { readFileSync } from "node:fs"
import { currentSaveFixture } from "./save-v2-test-fixture.ts"

const save = JSON.parse(readFileSync(new URL("./fixtures/current-schema-save.json", import.meta.url), "utf8")) as Record<string, unknown>

/** A populated schema projection; cross-domain references may be omitted, so Rust restore validity is not asserted. */
export function representativeCurrentSaveFixture(): Record<string, unknown> {
  const snapshot = save.snapshot as Record<string, unknown>
  const { day: _day, phase: _phase, ...savedSnapshot } = snapshot
  const markets = snapshot.markets as Record<string, Record<string, unknown>>
  const accounts = snapshot.accounts as Record<string, Record<string, unknown>>
  const planBook = save.plans as { plans: Record<string, Record<string, unknown>> }
  const beliefs = save.belief_books as Record<string, Record<string, unknown>>
  return {
    ...save,
    urgency_policy: currentSaveFixture().urgency_policy,
    plans: {
      ...planBook,
      plans: Object.fromEntries(Object.entries(planBook.plans).map(([id, plan]) => [id, {
        ...plan,
        review: { ...(plan.review as Record<string, unknown>), last_review_resources: null },
      }])),
    },
    // Explicit current-schema examples, matching the DeepValue midpoint preset.
    // This fixture projects schema branches; it does not migrate historical saves.
    belief_books: Object.fromEntries(Object.entries(beliefs).map(([id, book]) => [id, {
      ...book,
      experience: {
        reference_equity: null, peak_equity: null, consecutive_failed_buys: 0, stocks: {},
        feedback: { latest_moment: null, failure_events: [], stocks: {}, exit_records: [] },
      },
      institution_policy: {
        policy_version: 1, loss_response: "HoldOrAdd",
        cost_loss_threshold_bp: 1200, cost_profit_threshold_bp: 3000,
        risk_pause_drawdown_bp: 4000, risk_resume_drawdown_bp: 2000,
        risk_pause_failed_buys: 4, adverse_move_threshold_bp: 650,
      },
      institution_account_risk_paused: false,
    }])),
    snapshot: {
      ...savedSnapshot,
      markets: Object.fromEntries(Object.entries(markets).map(([code, market]) => {
        const { best_bid: _bestBid, best_ask: _bestAsk, bids: _bids, asks: _asks, ...raw } = market
        return [code, raw]
      })),
      accounts: Object.fromEntries(Object.entries(accounts).map(([id, account]) => {
        const { reserved_cash: _reservedCash, reserved_sell_qty: _reservedSellQty, ...raw } = account
        return [id, raw]
      })),
    },
  }
}
