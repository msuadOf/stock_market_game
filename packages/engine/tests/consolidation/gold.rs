//! 金样一（混合行业 80% 集团）与「全部售出」内部交易金样。
//!
//! 混合行业金样（母公司工业 v2 + 子公司银行 v3，持股 8000bp = 80%）：
//! - 母公司：开局 现金 5,000,000 + 固定资产 3,000,000 = 实收资本 8,000,000；
//!   1 月现销 1,000,000、现付管理费用 400,000 → NI 600,000、权益滚动 8,600,000。
//! - 子公司（银行）：开局 存放央行 2,000,000 = 实收资本 2,000,000；1 月手续费
//!   500,000、付存款利息 300,000 → NI 200,000、权益滚动 2,200,000。
//! - 合并（无内部交易）：NI 800,000（少数股东 40,000 / 归母 760,000）；
//!   权益 10,800,000（少数股东 440,000 / 归母 10,360,000）；
//!   合并现金 7,800,000 = 5,600,000 + 2,200,000（跨两张不同科目表按科目代码
//!   加总：1002 只在工业表、1003 只在银行表、4001 两表共享）。

use super::{acct, entry, member, request, yuan};
use engine::accounting::consolidation::{consolidate, ScopeId};
use engine::accounting::{
    AccountingAmount, BusinessKind, CashFlowClass, LedgerAccountId, PostingSide,
};
use engine::company::bank::bank_chart_v3;
use engine::company::industrial::industrial_chart_v2;
use PostingSide::{Credit, Debit};

/// 合并余额的净借方（断言辅助；贷方余额为负）。
fn net(
    out: &engine::accounting::consolidation::ConsolidationOutput,
    code: &str,
) -> AccountingAmount {
    out.adjusted_balances
        .get(&LedgerAccountId(code.to_string()))
        .map(|b| b.net_debit().expect("balance arithmetic"))
        .unwrap_or(AccountingAmount::ZERO)
}

#[test]
fn mixed_industry_group_80_20_minority_split_is_exact() {
    // 母公司（工业 v2）：开局 + 1 月现销 + 管理费用。
    let parent = super::books_with(
        industrial_chart_v2(),
        vec![
            entry(
                1,
                "2029-12-31",
                BusinessKind::OpeningBalance,
                CashFlowClass::Financing,
                &[
                    (acct::BANK, Debit, 5_000_000),
                    (acct::FIXED_ASSETS, Debit, 3_000_000),
                    (acct::CAPITAL, Credit, 8_000_000),
                ],
            ),
            entry(
                2,
                "2030-01-15",
                BusinessKind::CashRevenue,
                CashFlowClass::Operating,
                &[
                    (acct::BANK, Debit, 1_000_000),
                    (acct::REVENUE, Credit, 1_000_000),
                ],
            ),
            entry(
                3,
                "2030-01-20",
                BusinessKind::CashExpense,
                CashFlowClass::Operating,
                &[
                    (acct::ADMIN_EXP, Debit, 400_000),
                    (acct::BANK, Credit, 400_000),
                ],
            ),
        ],
    );
    // 子公司（银行 v3）：开局 + 1 月手续费 + 付存款利息。
    let sub = super::books_with(
        bank_chart_v3(),
        vec![
            entry(
                1,
                "2029-12-31",
                BusinessKind::OpeningBalance,
                CashFlowClass::Financing,
                &[
                    (acct::CENTRAL_BANK, Debit, 2_000_000),
                    (acct::CAPITAL, Credit, 2_000_000),
                ],
            ),
            entry(
                2,
                "2030-01-15",
                BusinessKind::FeeAndCommissionEarned,
                CashFlowClass::Operating,
                &[
                    (acct::CENTRAL_BANK, Debit, 500_000),
                    (acct::FEE_INCOME, Credit, 500_000),
                ],
            ),
            entry(
                3,
                "2030-01-20",
                BusinessKind::DepositInterestPaid,
                CashFlowClass::Operating,
                &[
                    (acct::INTEREST_EXP, Debit, 300_000),
                    (acct::CENTRAL_BANK, Credit, 300_000),
                ],
            ),
        ],
    );
    // 持股：80,000 / 100,000 = 8000bp（精确）。
    let req = request(
        "MIX-PARENT",
        vec![
            member("MIX-PARENT", None, 1_000_000, 0, &parent),
            member("MIX-BANK", Some("MIX-PARENT"), 100_000, 80_000, &sub),
        ],
    );
    let out = consolidate(req).expect("mixed-industry group must consolidate");

    assert_eq!(
        out.scope,
        ScopeId::Consolidated(engine::accounting::consolidation::MemberId(
            "MIX-PARENT".to_string()
        ))
    );
    assert!(out.worksheet.is_empty());
    // 少数股东拆分（20%）：损益 200,000 × 20% = 40,000；权益 2,200,000 × 20% = 440,000。
    assert_eq!(out.minority.len(), 1);
    let mi = &out.minority[0];
    assert_eq!(mi.parent_ownership_bp, 8000);
    assert_eq!(mi.minority_bp, 2000);
    assert_eq!(mi.subsidiary_adjusted_net_income, yuan(200_000));
    assert_eq!(mi.subsidiary_adjusted_equity, yuan(2_200_000));
    assert_eq!(mi.minority_net_income, yuan(40_000));
    assert_eq!(mi.minority_equity, yuan(440_000));
    // 合并损益与权益总量 + 归母/少数拆分（手算金样）。
    assert_eq!(out.consolidated_net_income, yuan(800_000));
    assert_eq!(out.net_income_to_parent, yuan(760_000));
    assert_eq!(out.net_income_to_minority, yuan(40_000));
    assert_eq!(out.consolidated_equity, yuan(10_800_000));
    assert_eq!(out.equity_to_parent, yuan(10_360_000));
    assert_eq!(out.minority_equity_total, yuan(440_000));
    // 跨科目表按代码加总：1002（仅工业）+ 1003（仅银行）+ 4001（两表共享）。
    assert_eq!(net(&out, acct::BANK), yuan(5_600_000));
    assert_eq!(net(&out, acct::CENTRAL_BANK), yuan(2_200_000));
    assert_eq!(net(&out, acct::FIXED_ASSETS), yuan(3_000_000));
    assert_eq!(net(&out, acct::CAPITAL), yuan(-10_000_000));
    assert_eq!(net(&out, acct::REVENUE), yuan(-1_000_000));
    assert_eq!(net(&out, acct::FEE_INCOME), yuan(-500_000));
    assert_eq!(net(&out, acct::ADMIN_EXP), yuan(400_000));
    assert_eq!(net(&out, acct::INTEREST_EXP), yuan(300_000));
    // 集团现金不变：合并现金 == Σ 成员现金（逐分相等）。
    let member_cash = parent
        .ledger()
        .cash_total()
        .expect("parent cash")
        .add(sub.ledger().cash_total().expect("sub cash"))
        .expect("member cash sum");
    assert_eq!(out.consolidated_cash, member_cash);
    assert_eq!(out.consolidated_cash, yuan(7_800_000));
    // 成员清单按 id 稳定排序。
    assert_eq!(
        out.members,
        vec![
            engine::accounting::consolidation::MemberId("MIX-BANK".to_string()),
            engine::accounting::consolidation::MemberId("MIX-PARENT".to_string()),
        ]
    );
}

/// 全部售出的内部交易：无未实现利润，抵销分录只有「收入/成本」两行（零金额
/// 存货行不产生）；子公司（卖方）NI 不受影响。
///
/// 子公司：开局 现金 400,000 + 库存 600,000；全部赊销给母公司 @1,000,000
/// （成本 600,000）→ NI 400,000。母公司：开局 现金 2,000,000；购入后全部
/// 对外赊售 @1,200,000 → NI 200,000。合并 NI 600,000（少数 80,000）；
/// 合并 1405 = 0；6401 = 600,000（集团口径成本）；6001 贷余 1,200,000（仅对外）。
#[test]
fn fully_sold_intercompany_sale_has_no_inventory_line() {
    let sub = super::books_with(
        industrial_chart_v2(),
        vec![
            entry(
                1,
                "2029-12-31",
                BusinessKind::OpeningBalance,
                CashFlowClass::Financing,
                &[
                    (acct::BANK, Debit, 400_000),
                    (acct::INVENTORY, Debit, 600_000),
                    (acct::CAPITAL, Credit, 1_000_000),
                ],
            ),
            entry(
                2,
                "2030-01-10",
                BusinessKind::CreditSale,
                CashFlowClass::NonCash,
                &[
                    (acct::AR, Debit, 1_000_000),
                    (acct::COGS, Debit, 600_000),
                    (acct::REVENUE, Credit, 1_000_000),
                    (acct::INVENTORY, Credit, 600_000),
                ],
            ),
        ],
    );
    let parent = super::books_with(
        industrial_chart_v2(),
        vec![
            entry(
                1,
                "2029-12-31",
                BusinessKind::OpeningBalance,
                CashFlowClass::Financing,
                &[
                    (acct::BANK, Debit, 2_000_000),
                    (acct::CAPITAL, Credit, 2_000_000),
                ],
            ),
            entry(
                2,
                "2030-01-12",
                BusinessKind::CreditSale,
                CashFlowClass::NonCash,
                &[
                    (acct::INVENTORY, Debit, 1_000_000),
                    (acct::PAYABLE, Credit, 1_000_000),
                ],
            ),
            entry(
                3,
                "2030-01-25",
                BusinessKind::CreditSale,
                CashFlowClass::NonCash,
                &[
                    (acct::AR, Debit, 1_200_000),
                    (acct::COGS, Debit, 1_000_000),
                    (acct::REVENUE, Credit, 1_200_000),
                    (acct::INVENTORY, Credit, 1_000_000),
                ],
            ),
        ],
    );
    let mut req = request(
        "GRP-PARENT",
        vec![
            member("GRP-PARENT", None, 1_000_000, 0, &parent),
            member("GRP-SUB", Some("GRP-PARENT"), 100_000, 80_000, &sub),
        ],
    );
    req.intercompany_balances = vec![
        super::ic_balance("GRP-SUB", "GRP-PARENT", acct::AR, 1_000_000),
        super::ic_balance("GRP-PARENT", "GRP-SUB", acct::PAYABLE, 1_000_000),
    ];
    req.intercompany_sales = vec![super::ic_sale(
        "GRP-SUB",
        "GRP-PARENT",
        1_000_000,
        600_000,
        0,
    )];
    let out = consolidate(req).expect("fully sold sale must consolidate");

    // 销售抵销分录恰好两行（收入/成本各 1,000,000），零金额存货行不出现。
    let sale_entries: Vec<_> = out
        .worksheet
        .iter()
        .filter(|e| {
            matches!(
                e.reason,
                engine::accounting::consolidation::WorksheetReason::IntercompanySale
            )
        })
        .collect();
    assert_eq!(sale_entries.len(), 1);
    assert_eq!(sale_entries[0].lines.len(), 2);
    // 应收/应付全额抵销（内部部分清零，仅剩对外应收）。
    assert_eq!(net(&out, acct::AR), yuan(1_200_000));
    assert_eq!(net(&out, acct::PAYABLE), yuan(0));
    assert_eq!(net(&out, acct::INVENTORY), yuan(0));
    assert_eq!(net(&out, acct::COGS), yuan(600_000));
    assert_eq!(net(&out, acct::REVENUE), yuan(-1_200_000));
    // 子公司 NI 不变（无未实现利润）；少数 20% × 400,000 = 80,000。
    assert_eq!(out.minority[0].minority_net_income, yuan(80_000));
    assert_eq!(out.consolidated_net_income, yuan(600_000));
    assert_eq!(out.net_income_to_parent, yuan(520_000));
    assert_eq!(out.consolidated_cash, yuan(2_400_000));
}
