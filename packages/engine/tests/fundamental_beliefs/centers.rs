//! 映射金样：能力中心 / λ / 期限（K5a 行 154 + K5 行 133 的计划锁定值）
//! 与信念簿 serde 往返。

use crate::priors::{cash_flow_analysis, form_on, profile, total_of};
use crate::{assumptions_rng, market, scenario};
use engine::information::{NpcInformationState, NpcObservationContext};
use engine::orderbook::AccountId;
use engine::strategy::{
    BeliefBook, CapabilityCenter, HotStyle, InstitutionStyle, RetailStyle, StrategyProfile,
    belief_horizon_days, capability_center, revision_lambda_bp,
};

/// 能力中心 / λ / 期限映射（K5a 行 154 + K5 行 133 的计划锁定值）。
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
    let json = serde_json::to_string(&book).expect("serializes");
    let restored: BeliefBook = serde_json::from_str(&json).expect("restores");
    assert_eq!(restored, book);
    assert_eq!(total_of(&restored), total);
    let _ = NpcObservationContext::new(npc, &state, &sc.library, &market_view);
}
