//! 失败路径（类型化不可用/错误，绝不静默 fallback、绝不偷偷改股价）。

mod flows;
mod guards;

use crate::scenario;
use crate::{ISSUED_SHARES, assumptions_rng, hour_after, market};
use engine::account::StockCode;
use engine::company::CompanyKind;
use engine::information::{AcquisitionError, NpcInformationState, NpcObservationContext};
use engine::orderbook::AccountId;
use engine::strategy::{
    AnalysisProfile, AnalysisWeights, BeliefBook, BeliefCause, BeliefError, BeliefInputs,
    FundamentalMethod, RetailStyle, StrategyProfile,
};

pub(crate) fn stock_code() -> StockCode {
    StockCode("600101".to_string())
}

/// 无任何已获知年报的参与者 + CreditDefault 重估 ⇒ 类型化 NoOwnAnnualMaterial。
#[test]
fn credit_default_without_own_annual_material_is_typed_error() {
    let sc = scenario();
    let npc = AccountId(3);
    let mut state = NpcInformationState::new(npc);
    state
        .record_acquisition(
            npc,
            &sc.library,
            sc.announcement_id,
            hour_after(
                engine::calendar::CivilInstant::from_hms(
                    engine::calendar::CivilDate::from_iso("2031-04-01").expect("date"),
                    18,
                    0,
                    0,
                )
                .expect("phase"),
            ),
        )
        .expect("acquire announcement");
    let market_view = market(30_000);
    let mut book = BeliefBook::new(
        npc,
        StrategyProfile::Retail(RetailStyle::Momentum),
        AnalysisProfile::new(
            AnalysisWeights::new(3_000, 2_000, 2_000, 1_500, 1_500).expect("weights"),
            Some(FundamentalMethod::CashFlow),
        )
        .expect("slot"),
        &mut assumptions_rng(0.0),
    );
    let ctx = NpcObservationContext::new(npc, &state, &sc.library, &market_view).expect("ctx");
    let inputs = BeliefInputs {
        ctx: &ctx,
        company: sc.company.clone(),
        kind: CompanyKind::Industrial,
        total_issued_shares: ISSUED_SHARES,
        as_of_trading_day: 1_100,
    };
    let err = book
        .apply_cause(
            &stock_code(),
            BeliefCause::CreditDefault {
                announcement: sc.announcement_id,
            },
            &inputs,
        )
        .expect_err("no own annual material");
    assert!(matches!(err, BeliefError::NoOwnAnnualMaterial));
}

/// 未获知的材料 id ⇒ 任务 16 NotAcquired 守卫透传（无前视）。
#[test]
fn unacquired_material_rejects_via_task16_guard() {
    let sc = scenario();
    let npc = AccountId(3);
    let state = NpcInformationState::new(npc);
    let market_view = market(30_000);
    let mut book = BeliefBook::new(
        npc,
        StrategyProfile::Retail(RetailStyle::Momentum),
        AnalysisProfile::new(
            AnalysisWeights::new(3_000, 2_000, 2_000, 1_500, 1_500).expect("weights"),
            Some(FundamentalMethod::CashFlow),
        )
        .expect("slot"),
        &mut assumptions_rng(0.0),
    );
    let ctx = NpcObservationContext::new(npc, &state, &sc.library, &market_view).expect("ctx");
    let inputs = BeliefInputs {
        ctx: &ctx,
        company: sc.company.clone(),
        kind: CompanyKind::Industrial,
        total_issued_shares: ISSUED_SHARES,
        as_of_trading_day: 1_100,
    };
    let err = book
        .apply_cause(
            &stock_code(),
            BeliefCause::NewMaterial {
                report: sc.annual_ids[3],
            },
            &inputs,
        )
        .expect_err("material not acquired");
    assert!(matches!(
        err,
        BeliefError::Acquisition(AcquisitionError::NotAcquired { .. })
    ));
}

/// 经历/到期触发落在不存在的条目上 ⇒ 类型化 NoBeliefEntry。
#[test]
fn triggers_without_entry_are_typed_errors() {
    let sc = scenario();
    let npc = AccountId(3);
    let state = NpcInformationState::new(npc);
    let market_view = market(30_000);
    let mut book = BeliefBook::new(
        npc,
        StrategyProfile::Retail(RetailStyle::Momentum),
        AnalysisProfile::new(
            AnalysisWeights::new(3_000, 2_000, 2_000, 1_500, 1_500).expect("weights"),
            Some(FundamentalMethod::CashFlow),
        )
        .expect("slot"),
        &mut assumptions_rng(0.0),
    );
    let ctx = NpcObservationContext::new(npc, &state, &sc.library, &market_view).expect("ctx");
    let inputs = BeliefInputs {
        ctx: &ctx,
        company: sc.company.clone(),
        kind: CompanyKind::Industrial,
        total_issued_shares: ISSUED_SHARES,
        as_of_trading_day: 1_100,
    };
    for cause in [
        BeliefCause::HorizonExpired,
        BeliefCause::ExperienceFailure {
            order: engine::orderbook::OrderId(1),
        },
    ] {
        let err = book
            .apply_cause(&stock_code(), cause, &inputs)
            .expect_err("no entry yet");
        assert!(matches!(err, BeliefError::NoBeliefEntry));
    }
}

/// 零基本面权重（方法缺失）⇒ 估值显式 Unavailable(MethodDisabled)，NPC 仍持条目。
#[test]
fn method_disabled_records_typed_unavailability() {
    let sc = scenario();
    let npc = AccountId(4);
    let mut state = NpcInformationState::new(npc);
    state
        .record_acquisition(
            npc,
            &sc.library,
            sc.annual_ids[3],
            hour_after(sc.annual_instants[3]),
        )
        .expect("acquire");
    let market_view = market(30_000);
    let mut book = BeliefBook::new(
        npc,
        StrategyProfile::Retail(RetailStyle::Noise),
        AnalysisProfile::new(
            AnalysisWeights::new(0, 1_000, 1_000, 1_000, 7_000).expect("weights"),
            None,
        )
        .expect("no-method profile is legal"),
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
    .expect("entry still forms for the disabled method");
    let entry = book.entry(&stock_code()).unwrap();
    assert_eq!(
        entry.valuation,
        engine::strategy::ValuationOutcome::Unavailable {
            reason: engine::strategy::ValuationUnavailable::MethodDisabled
        }
    );
}
