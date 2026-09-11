//! 金样：same-news-different-priors（相反修订）、no-trigger-stable（字节不变）、
//! 经历信心调整（界限 + 同订单去重）、能力中心/λ/期限映射、serde 往返。
//!
//! 相反修订金样（两个受控参与者、同为现金流法、λ=2500bp）：
//! - 甲先读 FY2029（可比增长 +125% → clamp 2000bp）+ 偏差 +1000 ⇒ 预期 +30%；
//! - 乙先读 FY2028（可比增长 −20% = −2000bp）+ 偏差 −1000 ⇒ 预期 −30%；
//! - 同一条新闻：FY2030（收入/净利均 +10% ⇒ 观察 +1000bp）。
//!
//! 修订后：甲 3000→2500（下调），乙 −3000→−2000（上调）——方向相反。
//! 独立金样（分）：甲 546,615,121 → 503,863,836；乙 20,219,635 → 79,823,534。

use crate::{ISSUED_SHARES, assumptions_rng, hour_after, market, scenario};
use engine::account::StockCode;
use engine::company::CompanyKind;
use engine::information::{NpcInformationState, NpcObservationContext};
use engine::orderbook::{AccountId, OrderId};
use engine::strategy::{
    AnalysisProfile, AnalysisWeights, BeliefBook, BeliefCause, BeliefError, BeliefInputs,
    ForecastBasis, FundamentalMethod, RetailStyle, StrategyProfile, ValuationOutcome,
};

pub(crate) fn stock_code() -> StockCode {
    StockCode("600101".to_string())
}

pub(crate) fn cash_flow_analysis() -> AnalysisProfile {
    AnalysisProfile::new(
        AnalysisWeights::new(3_000, 2_000, 2_000, 1_500, 1_500).expect("weights sum 10000"),
        Some(FundamentalMethod::CashFlow),
    )
    .expect("valid slot")
}

pub(crate) fn profile() -> StrategyProfile {
    StrategyProfile::Retail(RetailStyle::Momentum)
}

pub(crate) fn form_on(
    sc: &crate::Scenario,
    npc: AccountId,
    state: &mut NpcInformationState,
    report_index: usize,
    book: &mut BeliefBook,
    day: u64,
    market_view: &crate::BeliefMarket,
) {
    state
        .record_acquisition(
            npc,
            &sc.library,
            sc.annual_ids[report_index],
            hour_after(sc.annual_instants[report_index]),
        )
        .expect("acquire");
    let ctx = NpcObservationContext::new(npc, state, &sc.library, market_view).expect("ctx");
    let inputs = BeliefInputs {
        ctx: &ctx,
        company: sc.company.clone(),
        kind: CompanyKind::Industrial,
        total_issued_shares: ISSUED_SHARES,
        as_of_trading_day: day,
    };
    book.apply_cause(
        &stock_code(),
        BeliefCause::NewMaterial {
            report: sc.annual_ids[report_index],
        },
        &inputs,
    )
    .expect("material applies");
}

pub(crate) fn total_of(book: &BeliefBook) -> i128 {
    let entry = book.entry(&stock_code()).expect("entry");
    let ValuationOutcome::Available {
        total_equity_estimate,
        ..
    } = &entry.valuation
    else {
        panic!("expected an available valuation");
    };
    total_equity_estimate.cents()
}

/// 金样：同一新闻、不同先验 ⇒ 相反修订（计划验收句）。
#[test]
fn same_news_different_priors_revise_oppositely() {
    let sc = scenario();
    let market_view = market(30_000);

    // 甲：先验 +30%（FY2029 爆发历史 clamp 至 2000bp + 一次性偏差 +1000bp）。
    let a = AccountId(3);
    let mut state_a = NpcInformationState::new(a);
    let mut book_a = BeliefBook::new(
        a,
        profile(),
        cash_flow_analysis(),
        &mut assumptions_rng(1.0),
    );
    form_on(&sc, a, &mut state_a, 2, &mut book_a, 600, &market_view);
    let a_growth_before = book_a.entry(&stock_code()).unwrap().forecast.growth_bp;
    let a_total_before = total_of(&book_a);
    assert_eq!(a_growth_before, Some(3_000), "prior +30% expectation");
    assert_eq!(a_total_before, 546_615_121);

    // 乙：先验 −30%（FY2028 下滑历史 −2000bp + 一次性偏差 −1000bp）。
    let b = AccountId(5);
    let mut state_b = NpcInformationState::new(b);
    let mut book_b = BeliefBook::new(
        b,
        profile(),
        cash_flow_analysis(),
        &mut assumptions_rng(0.0),
    );
    form_on(&sc, b, &mut state_b, 1, &mut book_b, 600, &market_view);
    let b_growth_before = book_b.entry(&stock_code()).unwrap().forecast.growth_bp;
    let b_total_before = total_of(&book_b);
    assert_eq!(b_growth_before, Some(-3_000), "declining expectation");
    assert_eq!(b_total_before, 20_219_635);

    // 同一条新闻：FY2030（利润 +10% ⇒ 观察增长 +1000bp）。
    form_on(&sc, a, &mut state_a, 3, &mut book_a, 850, &market_view);
    form_on(&sc, b, &mut state_b, 3, &mut book_b, 850, &market_view);

    let a_growth_after = book_a.entry(&stock_code()).unwrap().forecast.growth_bp;
    let b_growth_after = book_b.entry(&stock_code()).unwrap().forecast.growth_bp;
    assert_eq!(a_growth_after, Some(2_500));
    assert_eq!(b_growth_after, Some(-2_000));
    assert!(
        a_growth_after < a_growth_before && b_growth_after > b_growth_before,
        "the same +10% news must move the two priors in opposite directions"
    );
    assert_eq!(
        book_a.entry(&stock_code()).unwrap().forecast.basis,
        ForecastBasis::Revised { observed_bp: 1_000 }
    );

    // 估值随修订同向移动：甲下调、乙上调。
    let (a_total_after, b_total_after) = (total_of(&book_a), total_of(&book_b));
    assert_eq!(a_total_after, 503_863_836);
    assert_eq!(b_total_after, 79_823_534);
    assert!(a_total_after < a_total_before && b_total_after > b_total_before);

    // 信心不受材料修订影响（只有经历事件调整）；锚与所用报告更新。
    for book in [&book_a, &book_b] {
        let entry = book.entry(&stock_code()).unwrap();
        assert_eq!(entry.confidence_bp, 6_000);
        assert_eq!(entry.anchor_trading_day, 850);
        assert_eq!(entry.used_report_ids, vec![sc.annual_ids[3]]);
    }
}

/// 金样：无新信息/到期/经历触发 ⇒ 信念字节不变；未到期触发被类型化拒绝。
#[test]
fn no_trigger_keeps_belief_bytes_unchanged() {
    let sc = scenario();
    let npc = AccountId(3);
    let mut state = NpcInformationState::new(npc);
    let market_view = market(30_000);
    let mut book = BeliefBook::new(
        npc,
        profile(),
        cash_flow_analysis(),
        &mut assumptions_rng(0.0),
    );
    form_on(&sc, npc, &mut state, 3, &mut book, 1_000, &market_view);
    let frozen = serde_json::to_string(&book).expect("book serializes");

    // 无触发：只读访问不移动任何字节（更新绝不由观察/每 tick 驱动）。
    let _ = book.entry(&stock_code());
    assert_eq!(serde_json::to_string(&book).expect("serializes"), frozen);

    // 到期未至（horizon 5，anchor 1000，第 1004 日剩余 1 日）⇒ 类型化拒绝 + 字节不变。
    let ctx = NpcObservationContext::new(npc, &state, &sc.library, &market_view).expect("ctx");
    let inputs = BeliefInputs {
        ctx: &ctx,
        company: sc.company.clone(),
        kind: CompanyKind::Industrial,
        total_issued_shares: ISSUED_SHARES,
        as_of_trading_day: 1_004,
    };
    let err = book
        .apply_cause(&stock_code(), BeliefCause::HorizonExpired, &inputs)
        .expect_err("premature expiry must be rejected");
    assert!(matches!(
        err,
        BeliefError::HorizonNotElapsed {
            remaining_trading_days: 1
        }
    ));
    assert_eq!(serde_json::to_string(&book).expect("serializes"), frozen);

    // 真到期：同事实重估（数值不变），锚推进、原因记录。
    let inputs = BeliefInputs {
        ctx: &ctx,
        company: sc.company.clone(),
        kind: CompanyKind::Industrial,
        total_issued_shares: ISSUED_SHARES,
        as_of_trading_day: 1_005,
    };
    book.apply_cause(&stock_code(), BeliefCause::HorizonExpired, &inputs)
        .expect("expiry applies");
    let entry = book.entry(&stock_code()).unwrap();
    assert_eq!(entry.anchor_trading_day, 1_005);
    assert_eq!(entry.forecast.growth_bp, Some(0));
    assert_eq!(
        total_of(&book),
        191_648_179,
        "same facts re-derive the same value"
    );
    assert_eq!(
        entry.last_cause.as_ref().expect("cause recorded").cause,
        BeliefCause::HorizonExpired
    );
}

/// 金样：经历触发——失败 −1000、真实获利退出 +500、界限 0..=10000、
/// 同一订单只更新一次（重复触发字节不变）。
#[test]
fn experience_confidence_bounds_and_once_per_order() {
    let sc = scenario();
    let npc = AccountId(3);
    let mut state = NpcInformationState::new(npc);
    let market_view = market(30_000);
    let mut book = BeliefBook::new(
        npc,
        profile(),
        cash_flow_analysis(),
        &mut assumptions_rng(0.0),
    );
    form_on(&sc, npc, &mut state, 3, &mut book, 1_000, &market_view);
    let total = total_of(&book);
    let ctx = NpcObservationContext::new(npc, &state, &sc.library, &market_view).expect("ctx");
    let inputs = BeliefInputs {
        ctx: &ctx,
        company: sc.company.clone(),
        kind: CompanyKind::Industrial,
        total_issued_shares: ISSUED_SHARES,
        as_of_trading_day: 1_001,
    };

    let apply = |book: &mut BeliefBook, cause: BeliefCause| {
        book.apply_cause(&stock_code(), cause, &inputs)
            .expect("experience applies");
    };
    apply(
        &mut book,
        BeliefCause::ExperienceFailure { order: OrderId(7) },
    );
    assert_eq!(book.entry(&stock_code()).unwrap().confidence_bp, 5_000);

    let after_first = serde_json::to_string(&book).expect("serializes");
    apply(
        &mut book,
        BeliefCause::ExperienceFailure { order: OrderId(7) },
    );
    assert_eq!(
        serde_json::to_string(&book).expect("serializes"),
        after_first,
        "the same order must update confidence exactly once"
    );

    for order in 8u64..=13 {
        apply(
            &mut book,
            BeliefCause::ExperienceFailure {
                order: OrderId(order),
            },
        );
    }
    assert_eq!(
        book.entry(&stock_code()).unwrap().confidence_bp,
        0,
        "6000 - 7000 clamps at the zero floor"
    );

    apply(
        &mut book,
        BeliefCause::ProfitableExit { order: OrderId(30) },
    );
    let entry = book.entry(&stock_code()).unwrap();
    assert_eq!(
        entry.confidence_bp, 500,
        "profitable exit adds 500 from the floor"
    );
    assert!(entry.applied_experience_orders.contains(&30));
    assert_eq!(
        total_of(&book),
        total,
        "experience never re-prices the valuation"
    );
}
