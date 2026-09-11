//! 金样 3–5（方法层/口径）：权益 ROE 独立金样 + 每股 10 元 mandate、
//! 流通股不变性 + 行情 spy、合并范围少数股东剔除。

use crate::gold::{
    hand_facts, momentum_analysis, momentum_profile, neutral_assumptions, stock_code,
};
use crate::hand::{HandReportSpec, hand_report};
use crate::{COMPANY, ISSUED_SHARES, assumptions_rng, d, hour_after, market, scenario};
use engine::Money;
use engine::calendar::CivilInstant;
use engine::company::{CompanyId, CompanyKind};
use engine::information::{NpcInformationState, NpcObservationContext};
use engine::orderbook::AccountId;
use engine::strategy::{
    BeliefBook, BeliefCause, BeliefInputs, FundamentalMethod, ValuationOutcome, equity_roe,
    estimate_by_method, extract_annual_facts, per_share_price, to_per_share_range,
};

/// 金样 3：权益 ROE 法（方法层）+ 每股 mandate 金样：
/// 归母权益估值 100 万元 / 总股本 10 万股 = 每股 10.00 元。
#[test]
fn equity_roe_method_gold_and_per_share_mandate() {
    let facts = hand_facts(80_000_000, 70_000_000, 9_375_000);
    let assumptions = neutral_assumptions();
    let estimates = equity_roe(&facts, &assumptions).expect("equity roe available");
    assert_eq!(estimates.central, 100_000_000);
    assert_eq!(estimates.pessimistic, 70_000_000);
    assert_eq!(estimates.optimistic, 145_000_000);

    // mandate：整体归母权益估值（分）÷ 总股本（半偶舍入）= 每股价格。
    let per_share = per_share_price(100_000_000, 100_000).expect("per-share converts");
    assert_eq!(
        per_share,
        Money::from_cents(1_000),
        "100 万元 / 10 万股 = 10 元"
    );
    let range = to_per_share_range(estimates, 100_000).expect("range converts");
    assert_eq!(range.pessimistic, Money::from_cents(700));
    assert_eq!(range.optimistic, Money::from_cents(1_450));
}

/// 金样 4：估值只除以发行人固定的已发行总股本——仅改变流通股数（可见行情
/// 快照字段）不改变任何信念字节；信念操作期间行情快照零变化（spy）。
#[test]
fn float_shares_never_enter_valuation_and_market_untouched() {
    let sc = scenario();
    let mut books = Vec::new();
    for float in [40_000u64, 25_000] {
        let npc = AccountId(3);
        let mut state = NpcInformationState::new(npc);
        state
            .record_acquisition(
                npc,
                &sc.library,
                sc.annual_ids[3],
                hour_after(sc.annual_instants[3]),
            )
            .expect("acquire");
        let market_view = market(float);
        let market_before = market_view.clone();
        let mut book = BeliefBook::new(
            npc,
            momentum_profile(),
            momentum_analysis(FundamentalMethod::CashFlow),
            &mut assumptions_rng(0.0),
        );
        let ctx = NpcObservationContext::new(npc, &state, &sc.library, &market_view).expect("ctx");
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
        .expect("formation");
        assert_eq!(
            market_view, market_before,
            "belief ops never touch market state"
        );
        books.push(serde_json::to_string(&book).expect("book serializes"));
    }
    assert_eq!(
        books[0], books[1],
        "changing only float shares must leave the belief byte-identical"
    );
}

/// 金样 5：合并范围少数股东剔除——归母权益/归母净利被采用，集团总量与
/// 少数行不泄漏进估值；与等额单体口径产出完全一致。
#[test]
fn minority_equity_excluded_in_consolidated_scope() {
    let company = CompanyId(COMPANY.to_string());
    let as_of = CivilInstant::from_hms(d("2031-03-21"), 12, 0, 0).expect("as_of");
    let consolidated = extract_annual_facts(
        &hand_report(HandReportSpec {
            consolidated: true,
            equity_to_parent_cents: 80_000_000,
            total_equity_cents: 100_000_000,
            opening_parent_cents: 70_000_000,
            ni_total_cents: 12_000_000,
            ni_to_parent_cents: Some(9_375_000),
            revenue_cents: 200_000_000,
            prior_revenue_cents: Some(160_000_000),
            operating_cf_cents: 9_375_000,
            ..HandReportSpec::default()
        }),
        &company,
        as_of,
    )
    .expect("consolidated facts extract");
    assert_eq!(consolidated.equity_to_parent.cents(), 80_000_000);
    assert_eq!(consolidated.net_income_to_parent.cents(), 9_375_000);

    let standalone = extract_annual_facts(
        &hand_report(HandReportSpec {
            equity_to_parent_cents: 80_000_000,
            total_equity_cents: 80_000_000,
            opening_parent_cents: 70_000_000,
            ni_total_cents: 9_375_000,
            revenue_cents: 200_000_000,
            prior_revenue_cents: Some(160_000_000),
            operating_cf_cents: 9_375_000,
            ..HandReportSpec::default()
        }),
        &company,
        as_of,
    )
    .expect("standalone facts extract");

    let assumptions = neutral_assumptions();
    let consolidated_estimate = estimate_by_method(
        FundamentalMethod::EquityRoe,
        &consolidated,
        &assumptions,
        None,
        100_000,
    );
    let standalone_estimate = estimate_by_method(
        FundamentalMethod::EquityRoe,
        &standalone,
        &assumptions,
        None,
        100_000,
    );
    let (
        ValuationOutcome::Available {
            total_equity_estimate: total,
            ..
        },
        ValuationOutcome::Available {
            total_equity_estimate: twin,
            ..
        },
    ) = (&consolidated_estimate, &standalone_estimate)
    else {
        panic!("both scopes must stay available");
    };
    assert_eq!(total.cents(), 100_000_000, "minority must not leak in");
    assert_eq!(total, twin, "attributable split equals the standalone twin");
    // 少数行确实存在于合并报表（防夹具退化成伪合并：总量 1 亿 ≠ 归母 8 千万）。
    assert_ne!(consolidated.equity_to_parent.cents(), 100_000_000);
}
