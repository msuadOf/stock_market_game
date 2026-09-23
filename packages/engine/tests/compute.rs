//! ComputeBackend 契约测试（任务 26 后）：纯批量输入、稳定顺序输出、
//! 无按账户身份授予的信息面。共同 V 及其演化接口已删除。
use engine::{
    create_backend, decide_data, ComputeBackend, ComputeError, ComputeMode, CpuBackend, MarketView,
    Money, SelfView, StockCode, StockView, StrategyData, StrategyError, TargetPolicy,
};
use std::collections::BTreeMap;

fn view(last: i64) -> MarketView {
    MarketView {
        stocks: BTreeMap::from([(
            StockCode("600101".to_string()),
            StockView {
                best_bid: Some(Money::from_cents(last - 1)),
                best_ask: Some(Money::from_cents(last + 1)),
                last_price: Money::from_cents(last),
                recent_prices: vec![Money::from_cents(last)],
                recent_market_minute_prices: vec![],
                relative_volume: 1.0,
                order_book_imbalance: 0.0,
            },
        )]),
        tick: 0,
        market_minute: 0,
    }
}

fn self_view() -> SelfView {
    SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    }
}

struct FixedRng(u64);

impl engine::Rng for FixedRng {
    fn next_f64(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(1);
        0.5
    }
    fn next_range_u32(&mut self, lo: u32, _hi: u32) -> u32 {
        lo
    }
}

#[test]
fn cpu_decide_all_validates_parallel_input_lengths() {
    let strategies = [StrategyData::inst(
        TargetPolicy::Fixed(Money::from_cents(1_000)),
        0.05,
        100,
    )];
    let error = CpuBackend
        .decide_all(&strategies, &view(900), &[], &[])
        .unwrap_err();
    assert!(matches!(error, ComputeError::InvalidInput(_)));
}

#[test]
fn cpu_decide_all_returns_outputs_in_input_order() {
    // 三种 kind 混合批量：输出索引与 strategies 对齐（稳定顺序），
    // 且所有决策者消费同一份公共市场视图（无身份差异信息面）。
    let strategies = [
        StrategyData::inst(TargetPolicy::Fixed(Money::from_cents(1_000)), 0.05, 100),
        StrategyData::inst(
            TargetPolicy::DriftUp {
                rate: 0.01,
                base: Money::from_cents(1_000),
            },
            0.05,
            100,
        ),
        StrategyData::hot(3, 0.02, 100),
    ];
    let selves = [self_view(), self_view(), self_view()];
    let seeds = [11_u64, 22, 33];
    let outputs = CpuBackend
        .decide_all(&strategies, &view(900), &selves, &seeds)
        .unwrap();
    assert_eq!(outputs.len(), strategies.len());
    // 两个机构目标价都在带下方 → 各产买入意图；索引对齐。
    assert!(!outputs[0].is_empty());
    assert!(!outputs[1].is_empty());
    // 同输入（同种子）→ 同输出：确定性。
    let repeat = CpuBackend
        .decide_all(&strategies, &view(900), &selves, &seeds)
        .unwrap();
    assert_eq!(
        serde_json::to_string(&outputs).unwrap(),
        serde_json::to_string(&repeat).unwrap()
    );
}

#[test]
fn cpu_decide_all_matches_direct_kernel_path() {
    let data = StrategyData::inst(TargetPolicy::Fixed(Money::from_cents(1_000)), 0.05, 100);
    let market = view(900);
    let own = self_view();
    let via_backend = CpuBackend
        .decide_all(
            std::slice::from_ref(&data),
            &market,
            std::slice::from_ref(&own),
            &[7],
        )
        .unwrap()
        .remove(0);
    let mut rng = FixedRng(7);
    let direct = decide_data(&data, &market, &own, &mut rng);
    assert_eq!(
        serde_json::to_string(&via_backend).unwrap(),
        serde_json::to_string(&direct).unwrap()
    );
}

#[test]
fn backend_selection_never_silently_falls_back_from_explicit_gpu() {
    assert!(create_backend(&ComputeMode::Cpu).is_ok());
    assert!(create_backend(&ComputeMode::Auto).is_ok());
    assert!(matches!(
        create_backend(&ComputeMode::Gpu),
        Err(ComputeError::BackendUnavailable(_))
    ));
}

/// StrategyError 可达性（保持原 use 面可检查）。
#[test]
fn strategy_error_variant_is_reachable() {
    let error: StrategyError = StrategyError::InvalidParam {
        param: "margin",
        reason: "not in [0,1)".to_string(),
    };
    assert!(error.to_string().contains("margin"));
}
