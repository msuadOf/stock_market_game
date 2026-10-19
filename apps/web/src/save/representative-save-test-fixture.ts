import { readFileSync } from "node:fs"
import { currentSaveFixture } from "./current-save-fixture.ts"

const save = JSON.parse(readFileSync(new URL("./fixtures/current-schema-save.json", import.meta.url), "utf8")) as Record<string, unknown>

/** 含非空领域数据的 schema projection；可省略跨域引用，因此不声明 Rust restore 有效。 */
export function representativeCurrentSaveFixture(): Record<string, unknown> {
  // 正例 fixture 显式迁入当前单 book 结构；parser 仍严格拒绝旧字段。
  const { auction_orders: _removedAuctionOrders, ...currentBookSave } = save
  const snapshot = save.snapshot as Record<string, unknown>
  const { day: _day, phase: _phase, ...savedSnapshot } = snapshot
  const markets = snapshot.markets as Record<string, Record<string, unknown>>
  const accounts = snapshot.accounts as Record<string, Record<string, unknown>>
  const planBook = save.plans as { plans: Record<string, Record<string, unknown>> }
  // B2 保存域（W3 接入）迁入：旧 fixture 摄于 B2 之前，缺根级四字段。
  // 已结算日的日K stats 全零（零成交交易日）——保留链按日注入
  // {trading:true, bars:[]} 与既有事实逐位自洽（parser 的分钟/日K互检兜底：
  // 若未来 fixture 的已结算日出现真实成交，空 bars 会被显式拒绝而非静默通过）。
  const stockIds = Object.keys(markets)
  const clock = save.civil_clock as { settled_through: string | null }
  const startDate = (save.setup as { start_date: string }).start_date
  const retainedDays: unknown[] = []
  for (let cursor = new Date(`${startDate}T00:00:00Z`); clock.settled_through !== null && cursor <= new Date(`${clock.settled_through}T00:00:00Z`); cursor = new Date(cursor.getTime() + 86_400_000)) {
    retainedDays.push({ date: cursor.toISOString().slice(0, 10), securities: Object.fromEntries(stockIds.map((id) => [id, { trading: true, bars: [] }])) })
  }
  const b2SaveFields = {
    market_minute_bars: Object.fromEntries(stockIds.map((id) => [id, []])),
    retained_market_days: retainedDays,
    personal_trade_confirmations: {},
    receipt_watermark: 0,
  }
  return {
    ...currentBookSave,
    ...b2SaveFields,
    runtime_state: {
      ...(save.runtime_state as Record<string, unknown>),
      book_public_cutoff_sequences: Object.fromEntries(Object.keys(markets).map((code) => [code, "0"])),
    },
    urgency_policy: currentSaveFixture().urgency_policy,
    plans: {
      ...planBook,
      plans: Object.fromEntries(Object.entries(planBook.plans).map(([id, plan]) => [id, {
        ...plan,
        review: { ...(plan.review as Record<string, unknown>), last_review_resources: null },
      }])),
    },
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
