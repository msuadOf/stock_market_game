import { readFileSync } from "node:fs"
import { projectHistoricalCompanyOperations } from "./company-slice-test-fixture.ts"
import { currentSaveFixture } from "./current-save-fixture.ts"

const save = JSON.parse(readFileSync(new URL("./fixtures/current-schema-save.json", import.meta.url), "utf8")) as Record<string, unknown>

/** 仅供测试的历史语料 schema projection；可省略跨域引用，不是 Rust save/restore 见证。 */
export function representativeCurrentSaveFixture(): Record<string, unknown> {
  // 正例 fixture 显式迁入当前单 book 结构；parser 仍严格拒绝旧字段。
  const { auction_orders: _removedAuctionOrders, groups, company_operations: _oldOperations, closing_registry: _oldClosing, ...currentBookSave } = save
  const snapshot = save.snapshot as Record<string, unknown>
  const { day: _day, phase: _phase, ...savedSnapshot } = snapshot
  const markets = snapshot.markets as Record<string, Record<string, unknown>>
  const accounts = snapshot.accounts as Record<string, Record<string, unknown>>
  // 原 JSON 是历史语料，保留旧字节；这里只投影测试输入，不提供生产旧档迁移。
  const planBook = save.plans as { policy: unknown; next_plan_seq: number; plans: Record<string, Record<string, unknown>> }
  const historicalPlans = Object.values(planBook.plans)
  const owner_sequences = [...new Set(historicalPlans.map((plan) => String(plan.account)))].map((owner) => ({ owner, next_sequence: String(planBook.next_plan_seq) }))
  const companyCodes = Object.keys((save.company_operations as { companies: Record<string, unknown> }).companies).sort()
  const companyId = (code: unknown): number => {
    const id = companyCodes.indexOf(String(code))
    if (id < 0) throw new Error(`历史测试语料公司代码不存在：${String(code)}`)
    return id
  }
  const unavailableRoe = { basis: { Unsupported: { reason: "MissingNonRecurringIncomeFacts" } }, ordinary_roe: { Unavailable: { reason: "MissingNonRecurringIncomeFacts" } }, adjusted_roe: { Unavailable: { reason: "MissingNonRecurringIncomeFacts" } }, weighted_average_parent_equity_cents: { Unavailable: { reason: "MissingNonRecurringIncomeFacts" } }, non_recurring: { Unavailable: { reason: "MissingNonRecurringIncomeFacts" } }, equity_events: { attributable_to_parent: { Unavailable: { reason: "MissingNonRecurringIncomeFacts" } }, minority_interest: { Unavailable: { reason: "MissingNonRecurringIncomeFacts" } } } }
  const reportSet = (report: Record<string, unknown>) => {
    const income = report.income as Record<string, unknown>
    const scope = Object.fromEntries(Object.entries(report.scope as Record<string, unknown>).map(([kind, code]) => [kind, companyId(code)]))
    return { ...report, scope, roe: unavailableRoe, income: { ...income, report_period: report.kind === "Quarter" ? income.quarter : income.cumulative, report_period_net_income_to_parent: income.net_income_to_parent } }
  }
  const library = save.public_library as { reports: Record<string, unknown>[]; announcements: Record<string, unknown>[] }
  const public_library = { ...library, reports: library.reports.map((report) => ({ ...report, company: companyId(report.company), source: "SimulationAccounting", reports: reportSet(report.reports as Record<string, unknown>) })), announcements: library.announcements.map((announcement) => {
    const { event, ...rest } = announcement
    return { ...rest, company: companyId(announcement.company), content: { kind: "Shock", value: event } }
  }) }
  const registry = save.closing_registry as { versions: [Record<string, unknown>, string, string, Record<string, unknown>[]][]; restatements: [Record<string, unknown>, unknown][] }
  const closing_registry = { versions: registry.versions.map(([scope, period, kind, reports]) => [Object.fromEntries(Object.entries(scope).map(([kind, code]) => [kind, companyId(code)])), period, kind, reports.map(reportSet)]), restatements: registry.restatements.map(([scope, entries]) => [Object.fromEntries(Object.entries(scope).map(([kind, code]) => [kind, companyId(code)])), entries]) }
  const information_states = Object.fromEntries(Object.entries(save.information_states as Record<string, { owner: number; companies: Record<string, unknown> }>).map(([owner, state]) => [owner, {
    ...state, owner: String(state.owner),
    companies: Object.fromEntries(Object.entries(state.companies).map(([code, records]) => [String(companyId(code)), records])),
  }]))
  const belief_books = Object.fromEntries(Object.entries(save.belief_books as Record<string, { npc: number; entries: Record<string, Record<string, unknown>> }>).map(([owner, book]) => [owner, {
    ...book, npc: String(book.npc),
    entries: Object.fromEntries(Object.entries(book.entries).map(([stock, entry]) => {
      const { anchor_trading_day: _historicalAnchor, last_cause, ...facts } = entry
      // synthetic：固定无此前会话的 typed 锚，仅保持非空 schema 覆盖，不解释旧 scalar 的日历含义。
      const anchor = { kind: "NoPriorSession" }
      let cause: Record<string, unknown> | null = null
      if (last_cause !== null) {
        const { as_of_trading_day: _historicalCauseAnchor, ...record } = last_cause as Record<string, unknown>
        cause = { ...record, as_of_anchor: anchor }
      }
      return [stock, { ...facts, company: companyId(entry.company), anchor, last_cause: cause }]
    })),
  }]))
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
    // synthetic：仅为当前 schema 测试显式选择策略，不改写历史原件或冒充 Rust 恢复证据。
    setup: { ...(save.setup as Record<string, unknown>), runtime_sync_policy: "every_tick" },
    desk_mirrored_resting: false,
    company_state: { LegacyOperations: { operations: projectHistoricalCompanyOperations(save.company_operations as Record<string, unknown>), closing: closing_registry, groups } },
    report_correction_operations: {},
    public_library,
    information_states,
    belief_books,
    ...b2SaveFields,
    runtime_state: {
      ...(save.runtime_state as Record<string, unknown>),
      book_public_cutoff_sequences: Object.fromEntries(Object.keys(markets).map((code) => [code, "0"])),
      retail_projection_seen: ((save.runtime_state as { retail_projection_seen: Record<string, unknown>[] }).retail_projection_seen).map((receipt) => {
        const localKey = receipt.local_key as Record<string, unknown>
        const transition = localKey.transition as Record<string, unknown>
        const envelope = transition.envelope as Record<string, unknown>
        return { ...receipt, local_key: { ...localKey, transition: { ...transition, envelope: { ...envelope, account: String(envelope.account) } } } }
      }),
    },
    urgency_policy: currentSaveFixture().urgency_policy,
    plans: {
      policy: planBook.policy,
      owner_sequences,
      plans: historicalPlans.map((plan) => ({ Investor: {
        ...plan,
        account: String(plan.account),
        plan_id: { owner: String(plan.account), sequence: String(plan.plan_id) },
        review: { ...(plan.review as Record<string, unknown>), last_review_resources: null },
      } })),
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
