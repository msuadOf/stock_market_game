//! 错误运输保持原 typed source、诊断上下文与紧凑 Result；不执行领域事务。

use super::capital_approval_context::{CapitalApprovalContext, CapitalApprovalError};
use super::company_capital_restore::{CapitalRestoreContext, CompanyCapitalRestoreError};
use super::company_command_candidates::{CompanyCommandCause, CompanyCommandError};
use super::company_commands_types::PlayerProposalKind;
use super::corporate_actions::distributions_day_end::{
    DistributionsDayEndError, DistributionsDayEndFailure,
};
use super::player_company_command_candidates::{PlayerCommandCause, PlayerCommandError};
use super::stock_distribution_command_candidate::{
    StockDistributionApprovalCause, StockDistributionApprovalError,
};
use crate::company::{CompanyId, CompanySystemError};
use crate::{AccountId, CivilDate, StockCode, StockId};
use std::error::Error;

fn date() -> CivilDate {
    CivilDate::from_iso("2030-06-03").unwrap()
}

fn source_as<T: Error + 'static>(error: &dyn Error) -> &T {
    error
        .source()
        .expect("包装错误必须保留原 source")
        .downcast_ref::<T>()
        .expect("装箱不能改变 source 的原具体错误类型")
}

macro_rules! compact_error {
    ($name:ident, $error:ty) => {
        #[test]
        fn $name() {
            let size = std::mem::size_of::<$error>();
            assert!(
                size <= 128,
                "{} 占 {size} 字节，超过 Result 错误运输的 128 字节预算",
                stringify!($error)
            );
        }
    };
}

compact_error!(capital_restore_error_is_compact, CompanyCapitalRestoreError);
compact_error!(capital_approval_error_is_compact, CapitalApprovalError);
compact_error!(cash_approval_error_is_compact, CompanyCommandError);
compact_error!(
    stock_approval_error_is_compact,
    StockDistributionApprovalError
);
compact_error!(player_candidate_error_is_compact, PlayerCommandError);
compact_error!(distributions_error_is_compact, DistributionsDayEndError);

fn cash_error() -> CompanyCommandError {
    CompanyCommandError {
        company: CompanyId(7),
        stock: StockCode("600101".into()),
        plan_id: "cash-plan-3".into(),
        day: date(),
        cause: CompanyCommandCause::Company(CompanySystemError::SystemState(
            "原始公司事实缺失".into(),
        ))
        .into(),
    }
}

#[test]
fn player_cash_error_keeps_the_complete_typed_chain_and_display() {
    let cash = cash_error();
    assert_eq!(
        cash.to_string(),
        "现金分红候选失败：公司 7，证券 StockCode(\"600101\")，计划 cash-plan-3，日期 2030-06-03：公司系统输入非法：原始公司事实缺失"
    );
    let error = PlayerCommandError {
        account: AccountId(23),
        company: CompanyId(7),
        kind: PlayerProposalKind::CashDividend,
        day: date(),
        cause: PlayerCommandCause::Approval(cash).into(),
    };
    let player_cause = source_as::<PlayerCommandCause>(&error);
    let original_cash = source_as::<CompanyCommandError>(player_cause);
    let cash_cause = source_as::<CompanyCommandCause>(original_cash);
    assert!(matches!(
        source_as::<CompanySystemError>(cash_cause),
        CompanySystemError::SystemState(detail) if detail == "原始公司事实缺失"
    ));
    assert!(error.to_string().starts_with(
        "玩家公司候选失败：账户 AccountId(23)，公司 7，类别 CashDividend，日期 2030-06-03：现金分红候选失败："
    ));
}

#[test]
fn stock_approval_error_keeps_the_original_company_source() {
    let error = StockDistributionApprovalError {
        company: CompanyId(7),
        stock: StockCode("600101".into()),
        event_id: "stock-event-4".into(),
        day: date(),
        cause: StockDistributionApprovalCause::Company(CompanySystemError::SystemState(
            "原始送转事实缺失".into(),
        ))
        .into(),
    };
    assert_eq!(
        error.to_string(),
        "送转候选失败：公司 7，证券 StockCode(\"600101\")，事件 stock-event-4，日期 2030-06-03：公司系统输入非法：原始送转事实缺失"
    );
    let cause = source_as::<StockDistributionApprovalCause>(&error);
    assert!(matches!(
        source_as::<CompanySystemError>(cause),
        CompanySystemError::SystemState(detail) if detail == "原始送转事实缺失"
    ));
}

#[test]
fn distributions_error_keeps_numeric_operands_and_original_source_type() {
    let error = DistributionsDayEndError {
        day: date(),
        event_id: "payment-5".into(),
        stock: Some(StockId(2)),
        account: Some(AccountId(23)),
        source: DistributionsDayEndFailure::NegativeAvailableCash {
            account: AccountId(23),
            actual: crate::Money::from_cents(-17),
        }
        .into(),
    };
    assert!(matches!(
        source_as::<DistributionsDayEndFailure>(&error),
        DistributionsDayEndFailure::NegativeAvailableCash { account: AccountId(23), actual }
            if actual.cents() == -17
    ));
    let message = error.to_string();
    assert!(message.contains("payment-5"), "{message}");
    assert!(message.contains("StockId(2)"), "{message}");
    assert!(message.contains("-17"), "{message}");
}

#[test]
fn capital_restore_context_and_typed_source_survive_approval_conversion() {
    let context = CapitalRestoreContext {
        company: Some(CompanyId(7)),
        stock: Some(StockId(2)),
        event_id: Some("capital-event-6".into()),
        receipt_index: Some(11),
        day: Some(date()),
        ..CapitalRestoreContext::default()
    };
    let replay = CompanyCapitalRestoreError::System {
        context: context.clone().into(),
        source: CompanySystemError::SystemState("原始资本事实缺失".into()),
    };
    assert_eq!(
        replay.to_string(),
        format!("资本恢复公司事实校验失败：{context:?}：公司系统输入非法：原始资本事实缺失")
    );
    let error: CapitalApprovalError = replay.into();
    let replay = source_as::<CompanyCapitalRestoreError>(&error);
    assert!(matches!(
        source_as::<CompanySystemError>(replay),
        CompanySystemError::SystemState(detail) if detail == "原始资本事实缺失"
    ));
    // 原 prepare 的 Result 类型不因内部布局调整而改变。
    let _prepare: fn(
        &super::GameSession,
        CompanyId,
    ) -> Result<CapitalApprovalContext, CapitalApprovalError> = CapitalApprovalContext::prepare;
}

#[test]
fn capital_restore_keeps_original_share_split_source() {
    let error = CompanyCapitalRestoreError::ShareSplit {
        context: CapitalRestoreContext {
            company: Some(CompanyId(7)),
            event_id: Some("split-event-7".into()),
            ..CapitalRestoreContext::default()
        }
        .into(),
        source: crate::company::share_split::ShareSplitError::ZeroIssuedShares {
            event_id: "split-event-7".into(),
            stock: StockCode("600101".into()),
            direction: crate::company::share_split::ShareSplitDirection::Consolidate,
            ratio: 100,
            issued_shares_before: 1,
            issued_shares_after: 0,
        },
    };
    assert!(matches!(
        source_as::<crate::company::share_split::ShareSplitError>(&error),
        crate::company::share_split::ShareSplitError::ZeroIssuedShares {
            issued_shares_before: 1,
            issued_shares_after: 0,
            ratio: 100,
            ..
        }
    ));
    assert!(error.to_string().contains("split-event-7"));
}
