//! 金样二/三：内部赊销 + 未实现利润抵销（上游与下游两个方向）。
//!
//! 共同数字：转移价 1,000,000 / 卖方成本 600,000 / 期末买方未售存货
//! 500,000（转移价口径）→ 未实现利润 = (1,000,000−600,000)×500,000/1,000,000
//! = 200,000。抵销分录（工作底稿）：
//!   Dr 卖方收入 1,000,000 / Cr 卖方成本 800,000 / Cr 买方存货 200,000；
//!   Dr 买方应付 1,000,000 / Cr 卖方应收 1,000,000。
//!
//! 上游（子公司 → 母公司）：未实现利润冲减子公司损益 → 少数股东按 20%
//! 分担（少数损益 = 20% × (400,000−200,000) = 40,000；少数权益 = 20% ×
//! (1,400,000−200,000) = 240,000）。
//! 下游（母公司 → 子公司）：未实现利润只冲减母公司损益，少数股东不分担
//! （少数损益 = 20% × 700,000 = 140,000）——与上游形成对照断言。

use super::{acct, entry, ic_balance, ic_sale, member, request, yuan};
use engine::accounting::consolidation::consolidate;
use engine::accounting::{BusinessKind, CashFlowClass, LedgerAccountId, PostingSide};
use engine::company::industrial::industrial_chart_v2;
use PostingSide::{Credit, Debit};

/// 合并余额的净借方。
fn net(out: &engine::accounting::consolidation::ConsolidationOutput, code: &str) -> i128 {
    out.adjusted_balances
        .get(&LedgerAccountId(code.to_string()))
        .map(|b| b.net_debit().expect("balance arithmetic").cents())
        .unwrap_or(0)
}

/// 上游场景的子公司账套：开局 + 全部赊销给母公司（成本 600,000、售价 1,000,000）。
fn upstream_sub() -> engine::accounting::Books {
    super::books_with(
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
    )
}

/// 上游场景的母公司账套：开局 + 赊购 + 对外售出一半（成本口径 500,000）。
fn upstream_parent() -> engine::accounting::Books {
    super::books_with(
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
                    (acct::COGS, Debit, 500_000),
                    (acct::REVENUE, Credit, 1_200_000),
                    (acct::INVENTORY, Credit, 500_000),
                ],
            ),
        ],
    )
}

#[test]
fn upstream_unrealized_profit_reduces_minority_share() {
    let sub = upstream_sub();
    let parent = upstream_parent();
    let mut req = request(
        "GRP-PARENT",
        vec![
            member("GRP-PARENT", None, 1_000_000, 0, &parent),
            member("GRP-SUB", Some("GRP-PARENT"), 100_000, 80_000, &sub),
        ],
    );
    req.intercompany_balances = vec![
        ic_balance("GRP-SUB", "GRP-PARENT", acct::AR, 1_000_000),
        ic_balance("GRP-PARENT", "GRP-SUB", acct::PAYABLE, 1_000_000),
    ];
    // 上游：子公司是卖方；未售存货 500,000 → 未实现利润 200,000。
    req.intercompany_sales = vec![ic_sale(
        "GRP-SUB",
        "GRP-PARENT",
        1_000_000,
        600_000,
        500_000,
    )];
    let out = consolidate(req).expect("upstream sale must consolidate");

    // 抵销后合并余额（手算金样，分）。
    assert_eq!(net(&out, acct::AR), yuan(1_200_000).cents());
    assert_eq!(net(&out, acct::PAYABLE), 0);
    assert_eq!(net(&out, acct::INVENTORY), yuan(300_000).cents());
    assert_eq!(net(&out, acct::COGS), yuan(300_000).cents());
    assert_eq!(net(&out, acct::REVENUE), yuan(-1_200_000).cents());
    // 子公司调整后 NI = 400,000 − 200,000 = 200,000；少数 20% 分担未实现利润。
    assert_eq!(
        out.minority[0].subsidiary_adjusted_net_income,
        yuan(200_000)
    );
    assert_eq!(out.minority[0].minority_net_income, yuan(40_000));
    assert_eq!(out.minority[0].subsidiary_adjusted_equity, yuan(1_200_000));
    assert_eq!(out.minority[0].minority_equity, yuan(240_000));
    // 合并总量：NI = 700,000 + 200,000 = 900,000；权益 = 2,700,000 + 1,200,000。
    assert_eq!(out.consolidated_net_income, yuan(900_000));
    assert_eq!(out.net_income_to_parent, yuan(860_000));
    assert_eq!(out.consolidated_equity, yuan(3_900_000));
    assert_eq!(out.equity_to_parent, yuan(3_660_000));
    // 抵销分录不触现金：合并现金 == Σ 成员现金（全部赊账，双方现金未动）。
    assert_eq!(out.consolidated_cash, yuan(2_400_000));
    let member_cash = sub
        .ledger()
        .cash_total()
        .expect("sub cash")
        .add(parent.ledger().cash_total().expect("parent cash"))
        .expect("member cash sum");
    assert_eq!(out.consolidated_cash, member_cash);
}

#[test]
fn downstream_unrealized_profit_stays_with_parent() {
    // 下游：母公司是卖方（开局带库存 600,000），子公司买后对外售出一半。
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
                    (acct::INVENTORY, Debit, 600_000),
                    (acct::CAPITAL, Credit, 2_600_000),
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
    let sub = super::books_with(
        industrial_chart_v2(),
        vec![
            entry(
                1,
                "2029-12-31",
                BusinessKind::OpeningBalance,
                CashFlowClass::Financing,
                &[
                    (acct::BANK, Debit, 1_000_000),
                    (acct::CAPITAL, Credit, 1_000_000),
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
                    (acct::COGS, Debit, 500_000),
                    (acct::REVENUE, Credit, 1_200_000),
                    (acct::INVENTORY, Credit, 500_000),
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
        ic_balance("GRP-PARENT", "GRP-SUB", acct::AR, 1_000_000),
        ic_balance("GRP-SUB", "GRP-PARENT", acct::PAYABLE, 1_000_000),
    ];
    req.intercompany_sales = vec![ic_sale(
        "GRP-PARENT",
        "GRP-SUB",
        1_000_000,
        600_000,
        500_000,
    )];
    let out = consolidate(req).expect("downstream sale must consolidate");

    // 合并余额与上游同形（数字相同）。
    assert_eq!(net(&out, acct::AR), yuan(1_200_000).cents());
    assert_eq!(net(&out, acct::PAYABLE), 0);
    assert_eq!(net(&out, acct::INVENTORY), yuan(300_000).cents());
    assert_eq!(net(&out, acct::COGS), yuan(300_000).cents());
    assert_eq!(net(&out, acct::REVENUE), yuan(-1_200_000).cents());
    // 关键对照：子公司 NI 700,000 不受下游未实现利润影响 → 少数 = 140,000。
    assert_eq!(
        out.minority[0].subsidiary_adjusted_net_income,
        yuan(700_000)
    );
    assert_eq!(out.minority[0].minority_net_income, yuan(140_000));
    assert_eq!(out.minority[0].subsidiary_adjusted_equity, yuan(1_700_000));
    assert_eq!(out.minority[0].minority_equity, yuan(340_000));
    // 合并总量：NI = 200,000 + 700,000 = 900,000；权益 = 2,800,000 + 1,700,000。
    assert_eq!(out.consolidated_net_income, yuan(900_000));
    assert_eq!(out.net_income_to_parent, yuan(760_000));
    assert_eq!(out.consolidated_equity, yuan(4_500_000));
    assert_eq!(out.equity_to_parent, yuan(4_160_000));
    assert_eq!(out.consolidated_cash, yuan(3_000_000));
}
