//! 经营流拒绝面：无授信资金断裂（付款失败为业务状态，公司存活、不补钱）、
//! 交付失败（完工前交付类型化拒绝）。

use super::super::fixtures::*;
use engine::accounting::{AccountingAmount, LedgerAccountId};
use engine::company::events::{ActiveShock, ShockKind};
use engine::company::operations::{
    CompanyOperations, CompanyOperationsConfig, FlowParams, OperationsError, ScheduledAction,
    SchedulerRequest,
};
use engine::company::{CompanyId, ShockParams};

fn net(books: &engine::accounting::Books, code: &str) -> AccountingAmount {
    books
        .ledger()
        .account_net_debit(&LedgerAccountId(code.to_string()))
        .expect("ledger query")
}

fn broke_ops() -> CompanyOperations {
    CompanyOperations::new(
        CompanyOperationsConfig {
            seed: 5,
            shock_params: quiet_params(),
            companies: vec![industrial_broke(d("2029-12-31"))],
        },
        d(START),
    )
    .expect("broke ops assembles")
}

fn broke_id() -> CompanyId {
    CompanyId("C-IND-BROKE".to_string())
}

fn cash(ops: &CompanyOperations) -> AccountingAmount {
    net(
        ops.industrial_books(&broke_id())
            .expect("broke present")
            .books(),
        "1002",
    )
}

/// 无授信 + 现金断裂：付款失败逐日记入业务状态（PaymentFailed 映射），
/// 公司存活继续经营，现金轨迹精确（无救助、无凭空造钱）、权益不动。
#[test]
fn funds_break_without_credit_records_payment_failures() {
    let mut ops = broke_ops();
    // 手算现金轨迹（分）：见 fixtures 注释的逐日追踪。
    let expected_cash = [100, 0, 0, 126, 52];
    let mut total_failures = 0usize;
    let mut cursor = d(START);
    for expected in expected_cash {
        let report = ops.advance_civil_day(cursor).expect("day completes");
        total_failures += report.payment_failures.len();
        assert_eq!(cash(&ops), AccountingAmount::from_cents(expected));
        cursor = cursor.next().expect("dates advance");
    }
    assert!(
        total_failures >= 8,
        "payment failures recorded: {total_failures}"
    );
    // 公司存活：权益恒等于开局值（亏损不自动结转、绝无股东派钱）。
    assert_eq!(cash_of_equity(&ops), AccountingAmount::from_cents(-1_300));
    // 第 6 日仍可推进（断裂不是引擎错误）。
    ops.advance_civil_day(cursor).expect("day 6 completes");
}

fn cash_of_equity(ops: &CompanyOperations) -> AccountingAmount {
    net(
        ops.industrial_books(&broke_id())
            .expect("broke present")
            .books(),
        "4001",
    )
}

/// 停工：地产中断冲击 → suspend_development（开发投入暂停、不崩溃），
/// 恢复后继续。中断期开发存货余额停止增长。
#[test]
fn real_estate_interruption_suspends_development_spend() {
    let start = d(START);
    let config = |seed| CompanyOperationsConfig {
        seed,
        shock_params: quiet_params(),
        companies: vec![real_estate_c(d("2029-12-31"))],
    };
    let mut baseline = CompanyOperations::new(config(2), start).expect("baseline");
    let mut halted = CompanyOperations::new(config(2), start).expect("halted");
    halted
        .apply_company_shock(
            &CompanyId("C-RE".to_string()),
            ActiveShock {
                kind: ShockKind::ProductionInterruption,
                amplitude_bp: 0,
                starts_on: start,
                expires_on: d("2030-01-03"),
            },
        )
        .expect("interruption injects");

    let mut cursor = start;
    for _ in 0..6 {
        baseline.advance_civil_day(cursor).expect("baseline day");
        halted.advance_civil_day(cursor).expect("halted day");
        cursor = cursor.next().expect("dates advance");
    }
    // 基线 6 日开发投入：土地 200,000 + 6×50,000；中断窗口 3 日（01-01..
    // 01-03）零投入 → 差恰 3×50,000 = 150,000。
    let base_dev = net(
        baseline.real_estate_books(&re_id()).expect("RE").books(),
        "1541",
    );
    let halted_dev = net(
        halted.real_estate_books(&re_id()).expect("RE").books(),
        "1541",
    );
    assert_eq!(
        base_dev.sub(halted_dev).expect("dev diff"),
        AccountingAmount::from_cents(150_000)
    );
}

fn re_id() -> CompanyId {
    CompanyId("C-RE".to_string())
}

/// 交付失败：项目未完工前合同到期交付 → 类型化拒绝（DeliveryBeforeCompletion），
/// 不静默确认收入。
#[test]
fn delivery_before_completion_is_typed_rejection() {
    let start = d(START);
    let mut ops = CompanyOperations::new(
        CompanyOperationsConfig {
            seed: 4,
            shock_params: quiet_params(),
            companies: vec![real_estate_c(d("2029-12-31"))],
        },
        start,
    )
    .expect("ops assembles");
    ops.submit_due(SchedulerRequest::Due {
        key: "MAT:C-RE:PS-1".to_string(),
        due_date: d("2030-01-03"),
        action: ScheduledAction::ContractMaturity {
            company: re_id(),
            reference: "DL:PS-1".to_string(),
        },
    })
    .expect("bogus delivery due submits");

    ops.advance_civil_day(start).expect("day 1 (land + dev)");
    ops.advance_civil_day(d("2030-01-02"))
        .expect("day 2 signs first presale");
    let error = ops
        .advance_civil_day(d("2030-01-03"))
        .expect_err("delivery before completion rejected");
    assert!(matches!(
        error,
        OperationsError::RealEstate(
            engine::company::real_estate::RealEstateError::DeliveryBeforeCompletion { .. }
        )
    ));
}

/// 冲击注入的变体/窗口校验：市场位注入非市场冲击 → 类型化拒绝；
/// 到期日早于起始日 → 类型化拒绝。
#[test]
fn shock_injection_validates_kind_and_window() {
    let start = d(START);
    let mut ops = broke_ops();
    assert!(matches!(
        ops.apply_market_shock(ActiveShock {
            kind: ShockKind::CompanyDemandShift,
            amplitude_bp: 100,
            starts_on: start,
            expires_on: d("2030-01-05"),
        }),
        Err(OperationsError::ShockKindMismatch { .. })
    ));
    assert!(matches!(
        ops.apply_company_shock(
            &broke_id(),
            ActiveShock {
                kind: ShockKind::CompanyDemandShift,
                amplitude_bp: 100,
                starts_on: start,
                expires_on: d("2029-12-31"),
            },
        ),
        Err(OperationsError::InvalidShockWindow { .. })
    ));
}

#[test]
fn zero_day_duration_is_rejected_without_mutating_config() {
    let mut config = CompanyOperationsConfig {
        seed: 5,
        shock_params: quiet_params(),
        companies: vec![industrial_broke(d("2029-12-31"))],
    };
    let original = config.clone();
    if let FlowParams::Industrial(params) = &mut config.companies[0].flow {
        params.receivable_credit_days = 0;
    }
    let error =
        CompanyOperations::new(config.clone(), d(START)).expect_err("zero duration rejected");
    assert!(matches!(
        error,
        OperationsError::InvalidDuration { days: 0, .. }
    ));
    assert_eq!(config, {
        let mut expected = original;
        if let FlowParams::Industrial(params) = &mut expected.companies[0].flow {
            params.receivable_credit_days = 0;
        }
        expected
    });
}

/// 前史后仍拒绝越界开局推进（日期必须逐日）。
#[test]
fn advance_refuses_out_of_sequence_dates() {
    let mut ops = broke_ops();
    ops.advance_civil_day(d(START)).expect("day 1");
    let error = ops
        .advance_civil_day(d("2030-01-03"))
        .expect_err("skipped day rejected");
    assert!(matches!(error, OperationsError::DateOutOfSequence { .. }));
    let _ = ShockParams::default_v1(); // 参数版本入口保持可用
}
