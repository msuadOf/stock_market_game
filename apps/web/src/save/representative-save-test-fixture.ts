import { readFileSync } from "node:fs"
import { currentSaveFixture } from "./save-v2-test-fixture.ts"

const save = JSON.parse(readFileSync(new URL("./fixtures/current-schema-save.json", import.meta.url), "utf8")) as Record<string, unknown>

/** A populated schema projection; cross-domain references may be omitted, so Rust restore validity is not asserted. */
export function representativeCurrentSaveFixture(): Record<string, unknown> {
  const snapshot = save.snapshot as Record<string, unknown>
  const { day: _day, phase: _phase, ...savedSnapshot } = snapshot
  const markets = snapshot.markets as Record<string, Record<string, unknown>>
  const accounts = snapshot.accounts as Record<string, Record<string, unknown>>
  return {
    ...save,
    urgency_policy: currentSaveFixture().urgency_policy,
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
