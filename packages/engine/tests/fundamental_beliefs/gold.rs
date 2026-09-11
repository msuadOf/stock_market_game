//! 金样 1–2（端到端）：盈利倍数/现金流两法经真实披露链路（账套 → 结账 →
//! 公开 → 获知 → 信念）的独立手算值。全部期望值由独立推导脚本预先算出
//! （rhe 链逐步半偶舍入），与 Rust 实现零共享。
//!
//! 独立金样速查（分）：
//! - 盈利倍数（NI 17,820,000 × quality 10000bp × PE 10）：中央 178,200,000；
//!   悲观 q−1000 → 160,380,000；乐观 q+1000 → 196,020,000；每股 1782/1604/1960。
//! - 现金流（FCFE 17,820,000，g=0/r=1000/gt=0）：中央 191,648,179；
//!   悲观 g−300,r+200 → 145,033,183；乐观 g+300,r−200 → 265,974,604；每股 1916/1450/2660。

use crate::{ISSUED_SHARES, assumptions_rng, d, hour_after, market, scenario};
use engine::Money;
use engine::account::StockCode;
use engine::accounting::AccountingAmount;
use engine::calendar::CivilInstant;
use engine::company::CompanyKind;
use engine::information::PublicationId;
use engine::information::{NpcInformationState, NpcObservationContext};
use engine::orderbook::AccountId;
use engine::strategy::{
    AnalysisProfile, AnalysisWeights, AnnualFacts, BeliefBook, BeliefCause, BeliefInputs,
    ForecastBasis, FundamentalMethod, PersonalAssumptions, PriorRevenue, StrategyProfile,
    ValuationOutcome,
};

pub(crate) fn stock_code() -> StockCode {
    StockCode("600101".to_string())
}

pub(crate) fn momentum_profile() -> StrategyProfile {
    StrategyProfile::Retail(engine::strategy::RetailStyle::Momentum)
}

pub(crate) fn momentum_analysis(slot: FundamentalMethod) -> AnalysisProfile {
    AnalysisProfile::new(
        AnalysisWeights::new(3_000, 2_000, 2_000, 1_500, 1_500).expect("weights sum 10000"),
        Some(slot),
    )
    .expect("valid slot")
}

pub(crate) fn neutral_assumptions() -> PersonalAssumptions {
    PersonalAssumptions {
        growth_deviation_bp: 0,
        quality_coefficient_bp: 10_000,
        pe_multiple: 10,
        equity_cost_bp: 1_000,
        terminal_growth_bp: 0,
        roe_deviation_bp: 0,
    }
}

/// 手工 AnnualFacts（方法层直测；分）。
pub(crate) fn hand_facts(equity_cents: i128, opening_cents: i128, ni_cents: i128) -> AnnualFacts {
    AnnualFacts {
        report_id: PublicationId::new(901),
        published_at: CivilInstant::from_hms(d("2031-03-20"), 18, 0, 0).expect("phase"),
        revenue: AccountingAmount::from_cents(200_000_000),
        prior_revenue: PriorRevenue::Comparative(AccountingAmount::from_cents(160_000_000)),
        net_income_to_parent: AccountingAmount::from_cents(ni_cents),
        equity_to_parent: AccountingAmount::from_cents(equity_cents),
        opening_equity_to_parent: AccountingAmount::from_cents(opening_cents),
        operating_cf: AccountingAmount::from_cents(ni_cents),
        investing_cf: AccountingAmount::ZERO,
        financing_cf: AccountingAmount::ZERO,
        net_new_borrowings: AccountingAmount::ZERO,
    }
}

/// 金样 1：盈利倍数法端到端（偶数账户 → 档案槽位 = 盈利倍数）。
/// dev +1000 只影响增长预测（2000bp），不进入盈利倍数估值。
#[test]
fn earnings_multiple_end_to_end_gold() {
    let sc = scenario();
    let npc = AccountId(2);
    let mut state = NpcInformationState::new(npc);
    state
        .record_acquisition(
            npc,
            &sc.library,
            sc.annual_ids[3],
            hour_after(sc.annual_instants[3]),
        )
        .expect("acquire FY2030");
    let market_view = market(40_000);
    let mut rng = assumptions_rng(1.0);
    let mut book = BeliefBook::new(
        npc,
        momentum_profile(),
        momentum_analysis(FundamentalMethod::EarningsMultiple),
        &mut rng,
    );
    assert_eq!(
        rng.draws(),
        6,
        "assumptions drawn exactly once at construction"
    );

    let ctx =
        NpcObservationContext::new(npc, &state, &sc.library, &market_view).expect("ctx builds");
    let inputs = BeliefInputs {
        ctx: &ctx,
        company: sc.company.clone(),
        kind: CompanyKind::Industrial,
        total_issued_shares: ISSUED_SHARES,
        as_of_trading_day: 1_000,
    };
    book.apply_cause(
        &stock_code(),
        BeliefCause::NewMaterial {
            report: sc.annual_ids[3],
        },
        &inputs,
    )
    .expect("formation succeeds");

    let entry = book.entry(&stock_code()).expect("entry exists");
    assert_eq!(entry.method, Some(FundamentalMethod::EarningsMultiple));
    assert_eq!(
        entry.forecast.basis,
        ForecastBasis::InitialTwoYear { observed_bp: 1_000 }
    );
    assert_eq!(entry.forecast.growth_bp, Some(2_000));
    assert_eq!(entry.confidence_bp, 6_000);
    assert_eq!(entry.horizon_trading_days, 5);
    assert_eq!(entry.anchor_trading_day, 1_000);
    assert_eq!(entry.used_report_ids, vec![sc.annual_ids[3]]);
    let ValuationOutcome::Available {
        total_equity_estimate,
        per_share,
    } = &entry.valuation
    else {
        panic!("earnings multiple must be available for profitable FY2030");
    };
    assert_eq!(total_equity_estimate.cents(), 178_200_000);
    assert_eq!(per_share.pessimistic, Money::from_cents(1_604));
    assert_eq!(per_share.optimistic, Money::from_cents(1_960));
}

/// 金样 2：现金流法端到端（奇数账户 → 档案槽位 = 现金流；dev −1000 抵消
/// +1000bp 观察 ⇒ 个人增长恰 0bp）。
#[test]
fn cash_flow_end_to_end_gold() {
    let sc = scenario();
    let npc = AccountId(3);
    let mut state = NpcInformationState::new(npc);
    state
        .record_acquisition(
            npc,
            &sc.library,
            sc.annual_ids[3],
            hour_after(sc.annual_instants[3]),
        )
        .expect("acquire FY2030");
    let market_view = market(30_000);
    let mut book = BeliefBook::new(
        npc,
        momentum_profile(),
        momentum_analysis(FundamentalMethod::CashFlow),
        &mut assumptions_rng(0.0),
    );
    let ctx =
        NpcObservationContext::new(npc, &state, &sc.library, &market_view).expect("ctx builds");
    let inputs = BeliefInputs {
        ctx: &ctx,
        company: sc.company.clone(),
        kind: CompanyKind::Industrial,
        total_issued_shares: ISSUED_SHARES,
        as_of_trading_day: 1_000,
    };
    book.apply_cause(
        &stock_code(),
        BeliefCause::NewMaterial {
            report: sc.annual_ids[3],
        },
        &inputs,
    )
    .expect("formation succeeds");

    let entry = book.entry(&stock_code()).expect("entry exists");
    assert_eq!(entry.method, Some(FundamentalMethod::CashFlow));
    assert_eq!(entry.forecast.growth_bp, Some(0));
    let ValuationOutcome::Available {
        total_equity_estimate,
        per_share,
    } = &entry.valuation
    else {
        panic!("cash flow must be available for FY2030");
    };
    assert_eq!(total_equity_estimate.cents(), 191_648_179);
    assert_eq!(per_share.pessimistic, Money::from_cents(1_450));
    assert_eq!(per_share.optimistic, Money::from_cents(2_660));
}
