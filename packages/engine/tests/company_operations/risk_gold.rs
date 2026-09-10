//! 违约/风险面金样（K4 验收：公司违约生成业务风险、未执行任何股东分配）
//! + 资产减值迹象。与 shock_gold 共用 fixtures。

use super::fixtures::*;
use engine::accounting::{AccountingAmount, LedgerAccountId};
use engine::calendar::CivilDate;
use engine::company::events::{ActiveShock, ShockKind};
use engine::company::operations::CompanyOperations;
use engine::company::CompanyId;

fn net(books: &engine::accounting::Books, code: &str) -> AccountingAmount {
    books
        .ledger()
        .account_net_debit(&LedgerAccountId(code.to_string()))
        .expect("ledger query")
}

fn run_days(ops: &mut CompanyOperations, start: CivilDate, days: i64) {
    let mut cursor = start;
    for _ in 0..days {
        ops.advance_civil_day(cursor).expect("day advances");
        cursor = cursor.next().expect("fixture dates advance");
    }
}

fn ind_a() -> CompanyId {
    CompanyId("C-IND-A".to_string())
}

fn bank_id() -> CompanyId {
    CompanyId("C-BANK".to_string())
}

fn c_ins() -> CompanyId {
    CompanyId("C-INS".to_string())
}

/// 客户信用恶化生成业务风险（坏账准备/贷款准备计提），且全程无股东分配
/// （权益科目余额恒等于开局值）。
#[test]
fn credit_deterioration_creates_business_risk_without_shareholder_distribution() {
    let start = d(START);
    let mut baseline = CompanyOperations::new(
        four_company_config(11, quiet_params(), d("2029-12-31")),
        start,
    )
    .expect("baseline assembles");
    let mut shocked = CompanyOperations::new(
        four_company_config(11, quiet_params(), d("2029-12-31")),
        start,
    )
    .expect("shocked assembles");
    shocked
        .apply_company_shock(
            &ind_a(),
            ActiveShock {
                kind: ShockKind::CreditDeterioration,
                amplitude_bp: 500,
                starts_on: start,
                expires_on: d("2030-01-05"),
            },
        )
        .expect("company shock injects");
    shocked
        .apply_company_shock(
            &bank_id(),
            ActiveShock {
                kind: ShockKind::CreditDeterioration,
                amplitude_bp: 500,
                starts_on: start,
                expires_on: d("2030-01-05"),
            },
        )
        .expect("bank shock injects");

    run_days(&mut baseline, start, 5);
    run_days(&mut shocked, start, 5);

    // 工商 A：ECL 率 1% → 6%；5 日应收 565,000 分 → 准备恰 33,900 分。
    let base_allow = net(
        baseline
            .industrial_books(&ind_a())
            .expect("A present")
            .books(),
        "1231",
    );
    let shock_allow = net(
        shocked
            .industrial_books(&ind_a())
            .expect("A present")
            .books(),
        "1231",
    );
    assert_eq!(base_allow, amt(-5_650));
    assert_eq!(shock_allow, amt(-33_900));

    // 银行：贷款重估到压力情景（PD 20% × LGD 50% = 10%）。
    // 基线 = 日终一阶段 0.4% × 2 笔 × 80,000 = 640；压力 = 逐笔
    // rhe(账面×10%)：LN-1 (80,000+35)→8,004、LN-4 (80,000+9)→8,001。
    let base_ecl = net(
        baseline
            .bank_books(&bank_id())
            .expect("bank present")
            .books(),
        "1303",
    );
    let shock_ecl = net(
        shocked
            .bank_books(&bank_id())
            .expect("bank present")
            .books(),
        "1303",
    );
    assert_eq!(base_ecl, amt(-640));
    assert_eq!(shock_ecl, amt(-16_005));

    // 无股东分配：所有公司权益科目净额恒等于开局值（利润不自动结转、不派现）。
    for ops in [&baseline, &shocked] {
        assert_eq!(
            net(
                ops.industrial_books(&ind_a()).expect("A present").books(),
                "4001"
            ),
            amt(-4_100_000)
        );
        assert_eq!(
            net(
                ops.bank_books(&bank_id()).expect("bank present").books(),
                "4001"
            ),
            amt(-5_000_000)
        );
        assert_eq!(
            net(
                ops.insurance_books(&c_ins()).expect("INS present").books(),
                "4001"
            ),
            amt(-2_000_000)
        );
    }
}

/// 生产中断停工（中断窗口内零生产），到期自动恢复。
#[test]
fn production_interruption_halts_and_recovers() {
    let start = d(START);
    let mut baseline = CompanyOperations::new(
        two_industrial_config(13, quiet_params(), d("2029-12-31")),
        start,
    )
    .expect("baseline assembles");
    let mut halted = CompanyOperations::new(
        two_industrial_config(13, quiet_params(), d("2029-12-31")),
        start,
    )
    .expect("halted assembles");
    halted
        .apply_company_shock(
            &ind_a(),
            ActiveShock {
                kind: ShockKind::ProductionInterruption,
                amplitude_bp: 0,
                starts_on: start,
                expires_on: d("2030-01-03"),
            },
        )
        .expect("interruption injects");

    run_days(&mut baseline, start, 5);
    run_days(&mut halted, start, 5);

    // 中断 3 日：产成品 50 −3×10 +2×(8−10) = 16；基线 50 +5×(8−10) = 40。
    let halted_fg = halted
        .industrial_books(&ind_a())
        .expect("A present")
        .inventory()
        .quantity(&engine::accounting::InventoryItemCode("FG-1".to_string()));
    let baseline_fg = baseline
        .industrial_books(&ind_a())
        .expect("A present")
        .inventory()
        .quantity(&engine::accounting::InventoryItemCode("FG-1".to_string()));
    assert_eq!(halted_fg, 16);
    assert_eq!(baseline_fg, 40);
}

/// 资产减值迹象 → 固定资产减值计提（2000bp × 账面 ¥10,000 = ¥2,000）。
#[test]
fn asset_impairment_signal_posts_impairment() {
    let start = d(START);
    let mut ops = CompanyOperations::new(
        two_industrial_config(17, quiet_params(), d("2029-12-31")),
        start,
    )
    .expect("ops assembles");
    ops.apply_company_shock(
        &ind_a(),
        ActiveShock {
            kind: ShockKind::AssetImpairmentSignal,
            amplitude_bp: 2_000,
            starts_on: start,
            expires_on: d("2030-01-05"),
        },
    )
    .expect("impairment signal injects");
    run_days(&mut ops, start, 1);

    assert_eq!(
        net(
            ops.industrial_books(&ind_a()).expect("A present").books(),
            "1603"
        ),
        amt(-200_000)
    );
}
