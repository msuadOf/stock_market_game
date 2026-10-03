//! 映射金样：能力中心 / λ / 期限的固定游戏参数。
//! 与信念簿 serde 往返。

use crate::priors::{cash_flow_analysis, profile, total_of};
use crate::{
    assumptions_rng, hour_after, market, scenario, BeliefIssuerInputs, FundamentalBeliefCase,
    ISSUED_SHARES,
};
use engine::company::CompanyKind;
use engine::orderbook::AccountId;
use engine::strategy::BeliefCause;
use engine::strategy::{
    belief_horizon_days, capability_center, revision_lambda_bp, BeliefBook, CapabilityCenter,
    HotStyle, InstitutionStyle, RetailStyle, StrategyProfile,
};

/// 能力中心 / λ / 期限映射的独立期望值。
#[test]
fn capability_centers_lambda_and_horizons() {
    use CapabilityCenter::*;
    assert_eq!(
        capability_center(&StrategyProfile::Retail(RetailStyle::LongTerm)),
        LongTerm
    );
    assert_eq!(
        capability_center(&StrategyProfile::Institution(InstitutionStyle::DeepValue)),
        Value
    );
    assert_eq!(
        capability_center(&StrategyProfile::Institution(InstitutionStyle::Defensive)),
        Value
    );
    assert_eq!(
        capability_center(&StrategyProfile::Institution(InstitutionStyle::Growth)),
        Growth
    );
    assert_eq!(
        capability_center(&StrategyProfile::Institution(InstitutionStyle::Balanced)),
        Other
    );
    assert_eq!(
        capability_center(&StrategyProfile::Hot(HotStyle::Reversal)),
        Other
    );

    assert_eq!(revision_lambda_bp(LongTerm), 4_000);
    assert_eq!(revision_lambda_bp(Value), 4_000);
    assert_eq!(revision_lambda_bp(Growth), 6_000);
    assert_eq!(revision_lambda_bp(Other), 2_500);

    assert_eq!(
        belief_horizon_days(&StrategyProfile::Retail(RetailStyle::LongTerm)),
        60
    );
    assert_eq!(
        belief_horizon_days(&StrategyProfile::Institution(InstitutionStyle::DeepValue)),
        60
    );
    assert_eq!(
        belief_horizon_days(&StrategyProfile::Institution(InstitutionStyle::Defensive)),
        60
    );
    assert_eq!(
        belief_horizon_days(&StrategyProfile::Institution(InstitutionStyle::Balanced)),
        20
    );
    assert_eq!(
        belief_horizon_days(&StrategyProfile::Institution(InstitutionStyle::Growth)),
        20
    );
    assert_eq!(
        belief_horizon_days(&StrategyProfile::Retail(RetailStyle::Momentum)),
        5
    );
    assert_eq!(
        belief_horizon_days(&StrategyProfile::Hot(HotStyle::Reversal)),
        5
    );
}

/// 信念簿可序列化往返（存档面）。
#[test]
fn belief_book_serde_round_trip() {
    let sc = scenario();
    let npc = AccountId(3);
    let mut case = FundamentalBeliefCase::new(
        sc,
        npc,
        market(30_000),
        profile(),
        cash_flow_analysis(),
        BeliefIssuerInputs {
            kind: CompanyKind::Industrial,
            total_issued_shares: ISSUED_SHARES,
        },
        &mut assumptions_rng(0.0),
    );
    let report = case.scenario.annual_ids[3];
    case.acquire(report, hour_after(case.scenario.annual_instants[3]))
        .expect("acquire");
    case.apply_cause(
        &crate::priors::stock_code(),
        BeliefCause::NewMaterial { report },
        1_000,
    )
    .expect("material applies");
    let total = total_of(&case.book);
    let json = serde_json::to_string(&case.book).expect("serializes");
    let restored: BeliefBook = serde_json::from_str(&json).expect("restores");
    assert_eq!(restored, case.book);
    assert_eq!(total_of(&restored), total);
}
