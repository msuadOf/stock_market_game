//! C2-C 的合法初始 mixed fixture；只在首次 restore 前冻结 synthetic policy。

use crate::calendar::{CalendarExchange, CivilDate, OfficialCoverageEntry};
use crate::{
    FloatAllocation, GameConfig, GameSession, HotParams, InstParams, Money, NpcSetup, RetailParams,
    SaveSlot, SecurityCategory, SessionSetup, StockCode, StockExchange, StockId, StockSpec,
    StrategyParams, SIMULATION_POLICY_ID,
};

pub const START_DATE: &str = "2030-01-02";
pub const TICKS_PER_DAY: u64 = 2;
pub const PRESET_HISTORY_DAYS: usize = 360;

pub fn date(iso: &str) -> CivilDate {
    CivilDate::from_iso(iso).expect("fixture 日期必须合法")
}

pub struct MixedFixture {
    pub session: GameSession,
    pub closed: StockId,
    pub open: StockId,
}

/// 每次重新 new/save，避免对已经运行的 Session 更换 source、policy 或 catalog。
pub fn initial_save(closed: CalendarExchange, first: StockExchange, auction: bool) -> SaveSlot {
    let shanghai = StockSpec {
        code: StockCode("600101".into()),
        exchange: StockExchange::Shanghai,
        initial_price: Money::from_cents(1_000),
        category: SecurityCategory::MainBoard,
        limit_pct: 0.10,
        tick: Money::from_cents(1),
        total_shares: 10_000_000,
        float_shares: 0,
    };
    let mut shenzhen = shanghai.clone();
    shenzhen.code = StockCode("000101".into());
    shenzhen.exchange = StockExchange::Shenzhen;
    let mut stocks = vec![shanghai, shenzhen];
    if first == StockExchange::Shenzhen {
        stocks.reverse();
    }
    let setup = SessionSetup {
        stocks,
        company_system: None,
        dividend_tax_mode: None,
        flat_withholding_bp: None,
        rights_offering_enabled: false,
        issuer_repurchase_enabled: false,
        par_value_per_share: None,
        auto_corporate_foundation: false,
        report_frequency: None,
        company_operations: None,
        groups: Vec::new(),
        npcs: NpcSetup {
            retail_count: 0,
            inst_count: 0,
            hot_count: 0,
            retail_cash_median: Money::from_cents(10_000_000),
        },
        config: GameConfig::proposed_defaults(),
        strategy_params: StrategyParams {
            retail: RetailParams {
                arrival_rate: 1.0,
                order_size_mean: 100,
                chase_prob: 0.0,
            },
            inst: InstParams {
                margin: 0.05,
                order_size: 200,
            },
            hot: HotParams {
                lookback: 3,
                trend_threshold: 0.02,
                order_size: 200,
            },
        },
        ticks_per_day: if auction { 12 } else { TICKS_PER_DAY },
        auction_ticks: if auction { 3 } else { 0 },
        closing_auction_ticks: if auction { 3 } else { 0 },
        history_len: 5,
        t1_enabled: true,
        float_allocation: FloatAllocation::random(),
        start_date: date(START_DATE),
        simulation_policy_id: SIMULATION_POLICY_ID.into(),
    };
    let session = GameSession::new(setup, 73).expect("合法两股零 NPC 初始局");
    assert_eq!((session.tick(), session.day()), (0, 0));
    let mut save = session.save().expect("合法初始 SaveSlot");
    // 年覆盖替代全年 fallback，必须保留 Jan1 休市，初始化前史才保持一致。
    save.civil_clock
        .policy
        .official_coverage
        .push(OfficialCoverageEntry::new(
            closed,
            2030,
            vec![(date("2030-01-01"), date(START_DATE))],
            "synthetic-test-c2c-mixed-calendar".into(),
            "synthetic-test-c2c-source-digest".into(),
        ));
    save
}

pub fn restore(closed: CalendarExchange, first: StockExchange, auction: bool) -> MixedFixture {
    let save = initial_save(closed, first, auction);
    let session = GameSession::restore(&save)
        .unwrap_or_else(|error| panic!("合法初始 mixed fixture restore 拒绝：{error:?}"));
    let (closed_code, open_code) = match closed {
        CalendarExchange::Sse => ("600101", "000101"),
        CalendarExchange::Szse => ("000101", "600101"),
    };
    let closed = session
        .stock_id(closed_code)
        .expect("closed 股必须在 catalog");
    let open = session.stock_id(open_code).expect("open 股必须在 catalog");
    MixedFixture {
        session,
        closed,
        open,
    }
}
