const ONE = "3ff0000000000000"
const SMALL = "3f847ae147ae147b"

type JsonObject = Record<string, unknown>

function object(value: unknown, label: string): JsonObject {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${label} must be an object`)
  }
  return value as JsonObject
}

function strategyState(profileValue: unknown): JsonObject {
  const profile = object(profileValue, "strategy profile")
  if (typeof profile.Retail === "string") {
    return {
      ZiNoise: {
        retail_style: profile.Retail,
        arrival_rate: ONE,
        order_size_mean: 100,
        chase_prob: SMALL,
        tick_cents: 1,
        dip_threshold: SMALL,
        stop_loss_threshold: SMALL,
        take_profit_threshold: SMALL,
        volume_confirmation: ONE,
        max_stock_fraction: ONE,
        base_observation_probability: ONE,
      },
    }
  }
  if (typeof profile.Institution === "string") {
    return {
      BeliefInstitution: {
        style: profile.Institution,
        margin: SMALL,
        order_size: 100,
        max_stock_fraction: ONE,
        base_observation_probability: ONE,
      },
    }
  }
  if (typeof profile.Hot === "string") {
    return {
      Momentum: {
        style: profile.Hot,
        lookback: 2,
        trend_threshold: SMALL,
        order_size: 100,
        volume_confirmation: ONE,
        max_stock_fraction: ONE,
        base_observation_probability: ONE,
      },
    }
  }
  throw new Error("strategy profile has no supported variant")
}

export function upgradeLegacySaveFixture(legacyValue: unknown): JsonObject {
  const legacy = object(legacyValue, "mature save")
  const profiles = object(legacy.strategy_profiles, "strategy_profiles")
  const strategy_states = Object.fromEntries(
    Object.entries(profiles).map(([account, profile]) => [account, strategyState(profile)]),
  )
  const { strategy_profiles: _removed, ...save } = legacy
  const setup = object(save.setup, "setup")
  return {
    ...save,
    schema_version: 2,
    runtime_v2: {
      poisoned: false,
      next_receipt_base: "0",
      live_envelopes: [],
      retail_projection_seen: [],
      strategy_states,
    },
    setup: { ...setup, simulation_policy_id: "a-share-simulation-v2" },
  }
}

export function currentSaveFixture(): JsonObject {
  return {
    schema_version: 2,
    runtime_v2: {
      poisoned: false,
      next_receipt_base: "0",
      live_envelopes: [],
      retail_projection_seen: [],
      strategy_states: {
        "1": {
          Momentum: {
            style: "Momentum",
            lookback: 2,
            trend_threshold: SMALL,
            order_size: 100,
            volume_confirmation: ONE,
            max_stock_fraction: ONE,
            base_observation_probability: ONE,
          },
        },
      },
    },
    setup: {
      stocks: [{
        code: "600101",
        exchange: "Shanghai",
        initial_price: 1120,
        category: "MainBoard",
        limit_pct: 0.1,
        tick: 1,
        total_shares: "8928571429",
        float_shares: 169318418,
      }],
      npcs: { retail_count: 1, inst_count: 0, hot_count: 0, retail_cash_median: 20_000_000 },
      config: {
        commission_rate: 0.00025,
        commission_min: 500,
        stamp_tax_rate: 0.0005,
        default_limit: 0.1,
        st_limit: 0.1,
        lot_size: 100,
        starting_cash: 1_000_000_000,
      },
      strategy_params: {
        retail: { arrival_rate: 0.3, order_size_mean: 300, chase_prob: 0.4, tick_cents: 1 },
        inst: { margin: 0.02, order_size: 200_000 },
        hot: { lookback: 20, trend_threshold: 0.03, order_size: 100_000 },
      },
      ticks_per_day: 15_480,
      auction_ticks: 900,
      closing_auction_ticks: 180,
      history_len: 20,
      t1_enabled: true,
      float_allocation: { ByKind: { retail: 1, inst: 0, hot: 0 } },
      start_date: "2030-01-01",
      simulation_policy_id: "a-share-simulation-v2",
    },
    seed: "42",
    snapshot: {
      seq: 0,
      tick: 0,
      day: 0,
      phase: "CallAuction",
      markets: {
        "600101": {
          last_price: 1120,
          last_close: 1120,
          best_bid: null,
          best_ask: null,
          bids: [],
          asks: [],
        },
      },
      accounts: {
        "0": { cash: 1_000_000_000, positions: {}, reserved_cash: 0, reserved_sell_qty: {} },
      },
      daily_candles: {
        "600101": [{ time: 0, open: 1120, high: 1120, low: 1120, close: 1120, volume: 0 }],
      },
      active_daily_candles: {},
    },
    auction_orders: {},
    resting_orders: {},
    filled_orders: {},
    price_history: { "600101": [1120] },
    market_minute_closes: { "600101": [] },
    rng_state: "0",
    npc_attention: {},
    retail_experience: {},
    parent_orders: {},
    npc_order_lifecycles: [],
    pending_player: [],
    pending_npc: { observed_tick: 0, observed_accounts: [], intents: [], dependencies: [] },
    next_order_id: 1,
    civil_clock: {
      current_date: "2030-01-01",
      settled_through: null,
      next_due_seq: 0,
      pending_due: [],
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
    company_operations: {
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
    ops_wiring: { mirrored: [] },
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
    information_states: {},
    belief_books: {},
    watchlists: {},
    price_memories: {},
    pending_plan_events: [],
  }
}
