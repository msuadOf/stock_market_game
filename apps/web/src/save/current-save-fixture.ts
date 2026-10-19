const ONE = "3ff0000000000000"
const SMALL = "3f847ae147ae147b"

type JsonObject = Record<string, unknown>

export function currentSaveFixture(): JsonObject {
  return {
    runtime_state: {
      poisoned: false,
      next_receipt_base: "0",
      live_envelopes: [],
      book_public_cutoff_sequences: { "0": "0" },
      retail_projection_seen: [],
      strategy_states: {
        "1": {
          Momentum: {
            style: "Momentum",
            lookback: 2,
            trend_threshold: SMALL,
            order_size: 100,
            volume_confirmation: ONE,
            base_observation_probability: ONE,
          },
        },
      },
    },
    setup: {
      stocks: [{
        code: "600101",
        exchange: "Shanghai",
        initial_price: "1120",
        category: "MainBoard",
        limit_pct: 0.1,
        tick: "1",
        total_shares: "8928571429",
        float_shares: 169318418,
      }],
      npcs: { retail_count: 1, inst_count: 0, hot_count: 0, retail_cash_median: "20000000" },
      config: {
        commission_rate: 0.00025,
        commission_min: "500",
        stamp_tax_rate: 0.0005,
        default_limit: 0.1,
        st_limit: 0.1,
        price_cage_enabled: true,
        lot_size: 100,
        starting_cash: "1000000000",
      },
      strategy_params: {
        retail: { arrival_rate: 0.3, order_size_mean: 300, chase_prob: 0.4 },
        inst: { margin: 0.02, order_size: 200_000 },
        hot: { lookback: 20, trend_threshold: 0.03, order_size: 100_000 },
      },
      ticks_per_day: 15_480,
      auction_ticks: 900,
      closing_auction_ticks: 180,
      history_len: 20,
      t1_enabled: true,
      float_allocation: {
        between_kinds: { Percentage: { retail: 1, inst: 0, hot: 0 } },
        within_kind: "Random",
      },
      start_date: "2030-01-01",
      simulation_policy_id: "a-share-simulation",
    },
    seed: "42",
    snapshot: {
      seq: 0,
      tick: 0,
      markets: {
        "0": {
          last_price: "1120",
          last_close: "1120",
        },
      },
      accounts: {
        "0": { cash: "1000000000", positions: {} },
        // B3：NPC 账户（retail_count=1）——成员关系恰好校验要求快照含 NPC。
        "1": { cash: "20000000", positions: {} },
      },
      daily_candles: {
        "0": [{ time: 1893369600, open: "1120", high: "1120", low: "1120", close: "1120", volume: 0 }],
      },
      active_daily_candles: {},
    },
    resting_orders: {},
    book_next_sequences: { "0": "0" },
    filled_orders: {},
    price_history: { "0": ["1120"] },
    market_minute_closes: { "0": [] },
    // B2 保存域（W3 接入）：tick 0 无成交——分钟桶空、保留链空（未日结）、
    // receipt 日志空、水位 0（next_receipt_base 初值）。
    market_minute_bars: { "0": [] },
    retained_market_days: [],
    personal_trade_confirmations: {},
    receipt_watermark: 0,
    rng_state: "0",
    npc_attention: {},
    retail_experience: {},
    npc_order_lifecycles: [],
    pending_player: [],
    ingress_receipt_cursors: { next_account_ordinal: {}, next_stock_ordinal: {} },
    next_order_id: 1,
    civil_clock: {
      current_date: "2030-01-01",
      settled_through: null,
      policy: {
        algorithm_version: 1,
        default_start: "2030-01-01",
        runtime_min_start: "2000-01-01",
        runtime_max_end: "2099-12-31",
        init_only_min_start: "1998-01-01",
        official_coverage: [],
        simulated_fallback: {
          version: 1,
          notice_unverified_year: 2026,
          lunar_facts: { facts: [], digest: "fixture" },
          digest: "fixture",
        },
      },
    },
    groups: [],
    company_operations: {
      payment_failures: {},
      seed: "0",
      shock_params: {
        version: 1,
        market_candidate_bp: 100,
        industry_candidate_bp: 100,
        company_candidate_bp: 100,
        duration_min_days: 1,
        duration_max_days: 1,
        market_demand_band_bp: 1,
        industry_cost_band_bp: 1,
        company_demand_band_bp: 1,
        credit_deterioration_add_bp: 0,
      },
      scheduler: { next_seq: 0, settled_through: null, pending: [] },
      market_rng: { state: "0" },
      industry_rngs: {},
      companies: {},
      next_expected: null,
      history: null,
    },
    closing_registry: { versions: [], restatements: [] },
    public_library: { next_seq: 0, reports: [], announcements: [] },
    disclosures: { published_through: null, announced_through: null },
    plans: {
      policy: {
        reverse_revision_threshold_bp: 2000,
        review_signal_delta_bp: 1000,
        review_price_change_bp: 200,
      },
      next_plan_seq: 0,
      plans: {},
    },
    urgency_policy: {
      policy_version: 1,
      drop_30min_threshold_bp: -300,
      drop_1min_threshold_bp: -75,
      urgent_drawdown_threshold_bp: 2000,
      patient_confidence_threshold_bp: 4000,
      urgent_remaining_trading_days: 1,
      resume_signal_threshold_bp: 2000,
    },
    information_states: {},
    belief_books: {},
    watchlists: {},
    price_memories: {},
    history_reads: { "0": { stocks: {} }, "1": { stocks: {} } },
    // B3 远程登录：单人档的 local-owner 成员关系（账户 0；retail NPC=1——
    // 账户集合恰好校验：members ∪ {1} == {0, 1}）。
    market_memberships: {
      members: {
        "local-owner": { account_id: "0", admission_funding: { external_cash: "1000000000" } },
      },
    },
  }
}


/** 测试用：把 fixture 推进为"已日结到 settled"的形态（B2 保存域）——
 *  保留链覆盖 [start_date, settled]、已结算日的日K补真实零成交 stats
 *  （零成交开市日语义：trading=true 且 bars=[]，与 stats 全零逐位一致）。
 *  该日缺日K时按引擎日终形态合成（o=h=l=c=昨收、量额笔全零——与
 *  representative fixture 已结算日一致）。原对象就地修改并返回。 */
export function withSettledDay(save: JsonObject, settled: string, currentDate: string): JsonObject {
  const clock = save.civil_clock as Record<string, unknown>
  clock.current_date = currentDate
  clock.settled_through = settled
  const setup = save.setup as Record<string, unknown>
  const snapshot = save.snapshot as Record<string, unknown>
  const markets = snapshot.markets as Record<string, unknown>
  const dailyCandles = snapshot.daily_candles as Record<string, Array<Record<string, unknown>>>
  const days: unknown[] = []
  for (let cursor = new Date(`${setup.start_date as string}T00:00:00Z`); cursor <= new Date(`${settled}T00:00:00Z`); cursor = new Date(cursor.getTime() + 86_400_000)) {
    const date = cursor.toISOString().slice(0, 10)
    const time = Date.parse(`${date}T00:00:00Z`) / 1000
    days.push({ date, securities: Object.fromEntries(Object.keys(markets).map((id) => [id, { trading: true, bars: [] }])) })
    for (const [id, candles] of Object.entries(dailyCandles)) {
      const candle = candles.find((candle) => candle.time === time)
      if (candle === undefined) {
        // 零成交开市日的真实日K形态（引擎日结对无成交日发 o=h=l=c=昨收、
        // volume 0、stats 全零——与 representative fixture 已结算日一致）。
        const previous = candles.at(-1)
        if (previous === undefined) throw new Error(`测试 fixture 缺 ${date}（stock ${id}）前史日K，不能标记为已日结`)
        candles.push({ time, open: previous.close, high: previous.close, low: previous.close, close: previous.close, volume: 0, trade_stats: { turnover_cents: "0", trade_count: 0 } })
      } else if (candle.trade_stats === undefined) {
        candle.trade_stats = { turnover_cents: "0", trade_count: 0 }
      }
    }
  }
  save.retained_market_days = days
  return save
}
