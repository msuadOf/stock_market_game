//! 类型化拒绝用例（内部往来/内部销售申报）。
//!
//! 对手方余额不符必须列出两侧数值，绝不用差额 plug 平账；工作底稿分录
//! 不得触碰现金科目；申报科目必须存在于对应成员科目表且要素相符。

use super::super::{acct, ic_balance, ic_sale, member, request, yuan};
use super::pair_books;
use engine::accounting::consolidation::{consolidate, ConsolidationError, MemberId};
use engine::accounting::LedgerAccountId;

#[test]
fn counterparty_balance_mismatch_lists_both_balances() {
    let (parent, sub) = pair_books();
    let mut req = request(
        "IC-ROOT",
        vec![
            member("IC-ROOT", None, 1_000, 0, &parent),
            member("IC-SUB", Some("IC-ROOT"), 100, 80, &sub),
        ],
    );
    // 母公司应付 900,000 vs 子公司应收 1,000,000：两侧都要出现在错误里。
    req.intercompany_balances = vec![
        ic_balance("IC-SUB", "IC-ROOT", acct::AR, 1_000_000),
        ic_balance("IC-ROOT", "IC-SUB", acct::PAYABLE, 900_000),
    ];
    let err = consolidate(req).expect_err("mismatched counterparty balances must be rejected");
    match err {
        ConsolidationError::CounterpartyMismatch {
            side_a,
            side_b: Some(side_b),
        } => {
            assert_eq!(side_a.amount, yuan(1_000_000));
            assert_eq!(side_b.amount, yuan(900_000));
            assert_eq!(side_a.account.0, acct::AR);
            assert_eq!(side_b.account.0, acct::PAYABLE);
        }
        other => panic!("expected CounterpartyMismatch, got {other:?}"),
    }
}

#[test]
fn one_sided_intercompany_balance_is_rejected() {
    let (parent, sub) = pair_books();
    let mut req = request(
        "ONE-ROOT",
        vec![
            member("ONE-ROOT", None, 1_000, 0, &parent),
            member("ONE-SUB", Some("ONE-ROOT"), 100, 80, &sub),
        ],
    );
    req.intercompany_balances = vec![ic_balance("ONE-SUB", "ONE-ROOT", acct::AR, 1_000_000)];
    let err = consolidate(req).expect_err("one-sided declaration must be rejected");
    assert!(matches!(
        err,
        ConsolidationError::CounterpartyMismatch { side_b: None, .. }
    ));
}

#[test]
fn intercompany_pair_of_same_element_is_rejected() {
    let (parent, sub) = pair_books();
    let mut req = request(
        "SAME-ROOT",
        vec![
            member("SAME-ROOT", None, 1_000, 0, &parent),
            member("SAME-SUB", Some("SAME-ROOT"), 100, 80, &sub),
        ],
    );
    // 两侧都申报资产类科目（应收 vs 应收）：往来抵销要求一资产一负债。
    req.intercompany_balances = vec![
        ic_balance("SAME-SUB", "SAME-ROOT", acct::AR, 1_000_000),
        ic_balance("SAME-ROOT", "SAME-SUB", acct::AR, 1_000_000),
    ];
    let err = consolidate(req).expect_err("same-element pair must be rejected");
    assert!(matches!(
        err,
        ConsolidationError::IntercompanyPairShape { .. }
    ));
}

#[test]
fn intercompany_touching_cash_is_rejected() {
    let (parent, sub) = pair_books();
    let mut req = request(
        "CASH-ROOT",
        vec![
            member("CASH-ROOT", None, 1_000, 0, &parent),
            member("CASH-SUB", Some("CASH-ROOT"), 100, 80, &sub),
        ],
    );
    req.intercompany_balances = vec![
        ic_balance("CASH-SUB", "CASH-ROOT", acct::BANK, 1_000_000),
        ic_balance("CASH-ROOT", "CASH-SUB", acct::PAYABLE, 1_000_000),
    ];
    let err = consolidate(req).expect_err("cash-touching declaration must be rejected");
    assert!(matches!(
        err,
        ConsolidationError::IntercompanyTouchesCash { .. }
    ));
}

#[test]
fn unknown_intercompany_member_is_rejected() {
    let (parent, sub) = pair_books();
    let mut req = request(
        "UNK-ROOT",
        vec![
            member("UNK-ROOT", None, 1_000, 0, &parent),
            member("UNK-SUB", Some("UNK-ROOT"), 100, 80, &sub),
        ],
    );
    req.intercompany_balances = vec![ic_balance("GHOST", "UNK-ROOT", acct::AR, 1_000)];
    let err = consolidate(req).expect_err("unknown member must be rejected");
    assert_eq!(
        err,
        ConsolidationError::UnknownIntercompanyMember {
            member: MemberId("GHOST".to_string()),
        }
    );
}

#[test]
fn sale_cost_beyond_invoice_is_rejected() {
    let (parent, sub) = pair_books();
    let mut req = request(
        "COST-ROOT",
        vec![
            member("COST-ROOT", None, 1_000, 0, &parent),
            member("COST-SUB", Some("COST-ROOT"), 100, 80, &sub),
        ],
    );
    req.intercompany_sales = vec![ic_sale("COST-SUB", "COST-ROOT", 1_000_000, 1_200_000, 0)];
    let err = consolidate(req).expect_err("cost beyond invoice must be rejected");
    assert!(matches!(
        err,
        ConsolidationError::SaleCostBeyondInvoice { .. }
    ));
}

#[test]
fn sale_unsold_beyond_invoice_is_rejected() {
    let (parent, sub) = pair_books();
    let mut req = request(
        "UNS-ROOT",
        vec![
            member("UNS-ROOT", None, 1_000, 0, &parent),
            member("UNS-SUB", Some("UNS-ROOT"), 100, 80, &sub),
        ],
    );
    req.intercompany_sales = vec![ic_sale(
        "UNS-SUB", "UNS-ROOT", 1_000_000, 600_000, 1_400_000,
    )];
    let err = consolidate(req).expect_err("unsold beyond invoice must be rejected");
    assert!(matches!(
        err,
        ConsolidationError::SaleUnsoldBeyondInvoice { .. }
    ));
}

#[test]
fn sale_account_with_wrong_element_is_rejected() {
    let (parent, sub) = pair_books();
    let mut req = request(
        "ELM-ROOT",
        vec![
            member("ELM-ROOT", None, 1_000, 0, &parent),
            member("ELM-SUB", Some("ELM-ROOT"), 100, 80, &sub),
        ],
    );
    let mut sale = ic_sale("ELM-SUB", "ELM-ROOT", 1_000_000, 600_000, 0);
    sale.revenue_account = LedgerAccountId(acct::BANK.to_string());
    req.intercompany_sales = vec![sale];
    let err = consolidate(req).expect_err("wrong-element account must be rejected");
    assert!(matches!(err, ConsolidationError::SaleAccountElement { .. }));
}

#[test]
fn books_unchanged_after_rejection() {
    let (parent, sub) = pair_books();
    let parent_before = parent.clone();
    let sub_before = sub.clone();
    let mut req = request(
        "KEEP-ROOT",
        vec![
            member("KEEP-ROOT", None, 1_000, 0, &parent),
            member("KEEP-SUB", Some("KEEP-ROOT"), 100, 80, &sub),
        ],
    );
    req.intercompany_balances = vec![
        ic_balance("KEEP-SUB", "KEEP-ROOT", acct::AR, 1_000_000),
        ic_balance("KEEP-ROOT", "KEEP-SUB", acct::PAYABLE, 900_000),
    ];
    assert!(consolidate(req).is_err());
    // 抵销是工作底稿：拒绝路径下成员账套字节不变。
    assert_eq!(parent, parent_before);
    assert_eq!(sub, sub_before);
}
