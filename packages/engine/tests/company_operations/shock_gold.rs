//! 共同经济冲击 → 不同公司不同经营响应（K4 金样）。
//!
//! 对照跑设计：同 seed + 静默冲击参数（无随机事件）跑基线；再注入单一结构化
//! 冲击跑对照——差异完全由该冲击的**经济字段**传导，不经任何随机路径。

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

/// 同一市场需求扩张（+3000bp，10 个自然日）作用于全部公司：
/// 工商多卖、保险多承保、**银行存贷流完全不受商品需求冲击影响**（K4 红线）。
#[test]
fn common_market_shock_diverges_operating_responses() {
    let start = d(START);
    let mut baseline = CompanyOperations::new(
        four_company_config(42, quiet_params(), d("2029-12-31")),
        start,
    )
    .expect("baseline ops assembles");
    let mut shocked = CompanyOperations::new(
        four_company_config(42, quiet_params(), d("2029-12-31")),
        start,
    )
    .expect("shocked ops assembles");
    shocked
        .apply_market_shock(ActiveShock {
            kind: ShockKind::MarketDemandShift,
            amplitude_bp: 3_000,
            starts_on: start,
            expires_on: d("2030-01-10"),
        })
        .expect("market shock injects");

    run_days(&mut baseline, start, 10);
    run_days(&mut shocked, start, 10);

    // 工商 A：需求 10→13 件/日，10 日收入恰多 30 件 × ¥100。
    let base_rev = net(
        baseline
            .industrial_books(&ind_a())
            .expect("A present")
            .books(),
        "6001",
    );
    let shock_rev = net(
        shocked
            .industrial_books(&ind_a())
            .expect("A present")
            .books(),
        "6001",
    );
    assert_eq!(base_rev.sub(shock_rev).expect("rev diff"), amt(300_000));

    // 保险：新单 2→3 组/日，10 日保费现金流恰多 10 组 × ¥60。
    let base_cash = net(
        baseline
            .insurance_books(&c_ins())
            .expect("INS present")
            .books(),
        "1002",
    );
    let shock_cash = net(
        shocked
            .insurance_books(&c_ins())
            .expect("INS present")
            .books(),
        "1002",
    );
    assert_eq!(shock_cash.sub(base_cash).expect("ins diff"), amt(60_000));

    // 银行：存/贷/手续费流不读需求字段——账套逐字节一致。
    assert_eq!(
        shocked.bank_books(&bank_id()).expect("bank present"),
        baseline.bank_books(&bank_id()).expect("bank present")
    );
}

fn c_ins() -> CompanyId {
    CompanyId("C-INS".to_string())
}

/// 行业成本冲击只作用于被标签行业：化工 B 采购变贵，家电 A 账套不变。
#[test]
fn industry_cost_shock_only_touches_tagged_industry() {
    let start = d(START);
    let mut baseline = CompanyOperations::new(
        two_industrial_config(7, quiet_params(), d("2029-12-31")),
        start,
    )
    .expect("baseline assembles");
    let mut shocked = CompanyOperations::new(
        two_industrial_config(7, quiet_params(), d("2029-12-31")),
        start,
    )
    .expect("shocked assembles");
    shocked
        .apply_industry_shock(ActiveShock {
            kind: ShockKind::IndustryCostShift {
                industry: engine::company::IndustryId("industrial-chemicals".to_string()),
            },
            amplitude_bp: 1_000,
            starts_on: start,
            expires_on: d("2030-01-10"),
        })
        .expect("industry shock injects");

    run_days(&mut baseline, start, 10);
    run_days(&mut shocked, start, 10);

    let b = CompanyId("C-IND-B".to_string());
    // B：采购单价 ¥2.0 → ¥2.2（+1000bp），10 日补货 4 件/日——进项税额
    // （222102，按当批货款直算、与移动加权平均无关）恰多 1,040 分；
    // 原料科目（移动加权平均耦合）净额高于基线。
    let base_vat = net(
        baseline.industrial_books(&b).expect("B present").books(),
        "222102",
    );
    let shock_vat = net(
        shocked.industrial_books(&b).expect("B present").books(),
        "222102",
    );
    assert_eq!(shock_vat.sub(base_vat).expect("vat diff"), amt(1_040));
    let base_raw = net(
        baseline.industrial_books(&b).expect("B present").books(),
        "1403",
    );
    let shock_raw = net(
        shocked.industrial_books(&b).expect("B present").books(),
        "1403",
    );
    assert!(
        shock_raw > base_raw,
        "pricier replenishment raises raw cost basis"
    );
    // A：行业标签不匹配 → 账套逐字节不变。
    assert_eq!(
        shocked.industrial_books(&ind_a()).expect("A present"),
        baseline.industrial_books(&ind_a()).expect("A present")
    );
}
