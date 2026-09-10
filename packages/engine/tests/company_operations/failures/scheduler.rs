//! 调度器拒绝面：重复 due key、股东动作、过期注册、跳日派发。

use super::super::fixtures::*;
use engine::calendar::CivilDate;
use engine::company::operations::CompanyOperations;
use engine::company::scheduler::{
    OperatingScheduler, ScheduledAction, SchedulerError, SchedulerRequest,
};
use engine::company::CompanyId;

fn interest_due(company: &str, key: &str, due_date: CivilDate) -> SchedulerRequest {
    SchedulerRequest::Due {
        key: key.to_string(),
        due_date,
        action: ScheduledAction::InterestAccrual {
            company: CompanyId(company.to_string()),
        },
    }
}

/// 同 key 重复注册（仍待办）→ 类型化拒绝，队列零改动。
#[test]
fn duplicate_due_key_is_rejected_without_mutation() {
    let mut scheduler = OperatingScheduler::new();
    scheduler
        .submit(interest_due(
            "C-IND-A",
            "INT:C-IND-A:2030-01-02",
            d("2030-01-02"),
        ))
        .expect("first submit");
    let before = scheduler.pending().to_vec();
    let error = scheduler
        .submit(interest_due(
            "C-IND-A",
            "INT:C-IND-A:2030-01-02",
            d("2030-01-02"),
        ))
        .expect_err("duplicate key rejected");
    assert!(
        matches!(error, SchedulerError::DuplicateDueKey { ref key } if key == "INT:C-IND-A:2030-01-02")
    );
    assert_eq!(scheduler.pending(), before.as_slice());
}

/// 股东动作送入调度器 → 明确不支持（K3 红线：结算仅设计，无运行时队列）。
#[test]
fn shareholder_actions_are_unsupported() {
    let mut scheduler = OperatingScheduler::new();
    let error = scheduler
        .submit(SchedulerRequest::ShareholderDistribution {
            company: CompanyId("C-IND-A".to_string()),
            detail: "dividend".to_string(),
        })
        .expect_err("shareholder action must be rejected");
    assert!(matches!(
        error,
        SchedulerError::ShareholderActionsUnsupported { .. }
    ));
    assert!(scheduler.pending().is_empty());
}

/// 日期回拨注册（due ≤ 已结算日）→ 类型化拒绝。
#[test]
fn due_registration_in_past_is_rejected() {
    let mut ops = CompanyOperations::new(
        two_industrial_config(3, quiet_params(), d("2029-12-31")),
        d(START),
    )
    .expect("ops assembles");
    ops.advance_civil_day(d(START)).expect("day 1 settles");
    let error = ops
        .submit_due(interest_due("C-IND-A", "INT:C-IND-A:2030-01-01", d(START)))
        .expect_err("past due rejected");
    assert!(matches!(
        error,
        engine::company::operations::OperationsError::Scheduler(
            SchedulerError::DueDateInPast { .. }
        )
    ));
}

/// pop_due_on 越过未处理 due（跳日）→ 类型化拒绝，不静默丢失。
#[test]
fn pop_due_detects_skipped_dates() {
    let mut scheduler = OperatingScheduler::new();
    scheduler
        .submit(interest_due(
            "C-IND-A",
            "INT:C-IND-A:2030-01-05",
            d("2030-01-05"),
        ))
        .expect("submit");
    let error = scheduler
        .pop_due_on(d("2030-01-06"))
        .expect_err("skip detected");
    assert!(matches!(error, SchedulerError::DueSkipped { .. }));
}
