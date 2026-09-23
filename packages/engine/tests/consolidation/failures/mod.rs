//! 类型化拒绝用例（图形/所有权/期间）+ 无子公司 NotApplicable。
//!
//! 每条拒绝都必须携带定位与数值上下文（铁律二：不静默吞错、不用差额 plug
//! 平账）；拒绝后成员账套字节不变（Books: PartialEq 全量对比）。

mod intercompany;

use super::{entry, member, request};
use engine::accounting::consolidation::{consolidate, ConsolidationError, MemberId};
use engine::accounting::{AccountChart, Books, BusinessKind, CashFlowClass, PostingSide};
use engine::company::industrial::industrial_chart_v2;
use PostingSide::{Credit, Debit};

/// 标准两成员夹具（母 + 80% 子，两个期间 2029-12 + 2030-01 都有分录，
/// 期间覆盖一致——拒绝必须来自被测守卫本身）。
pub(crate) fn pair_books() -> (Books, Books) {
    let parent = super::books_with(
        industrial_chart_v2(),
        vec![
            super::entry(
                1,
                "2029-12-31",
                BusinessKind::OpeningBalance,
                CashFlowClass::Financing,
                &[
                    (super::acct::BANK, Debit, 1_000_000),
                    (super::acct::CAPITAL, Credit, 1_000_000),
                ],
            ),
            super::entry(
                2,
                "2030-01-10",
                BusinessKind::CashExpense,
                CashFlowClass::Operating,
                &[
                    (super::acct::ADMIN_EXP, Debit, 1_000),
                    (super::acct::BANK, Credit, 1_000),
                ],
            ),
        ],
    );
    let sub = super::books_with(
        industrial_chart_v2(),
        vec![
            super::entry(
                1,
                "2029-12-31",
                BusinessKind::OpeningBalance,
                CashFlowClass::Financing,
                &[
                    (super::acct::BANK, Debit, 500_000),
                    (super::acct::CAPITAL, Credit, 500_000),
                ],
            ),
            super::entry(
                2,
                "2030-01-15",
                BusinessKind::CashRevenue,
                CashFlowClass::Operating,
                &[
                    (super::acct::BANK, Debit, 2_000),
                    (super::acct::REVENUE, Credit, 2_000),
                ],
            ),
        ],
    );
    (parent, sub)
}

#[test]
fn group_cycle_is_rejected_with_member_context() {
    let (parent, sub) = pair_books();
    // B→C→B 成环（永远到不了根）。
    let req = request(
        "CY-ROOT",
        vec![
            member("CY-ROOT", None, 1_000, 0, &parent),
            member("CY-B", Some("CY-C"), 100, 80, &sub),
            member("CY-C", Some("CY-B"), 100, 80, &parent),
        ],
    );
    let err = consolidate(req).expect_err("cycle must be rejected");
    assert!(matches!(err, ConsolidationError::GroupCycle { .. }));
}

#[test]
fn duplicate_member_is_rejected() {
    let (parent, sub) = pair_books();
    let req = request(
        "DUP-ROOT",
        vec![
            member("DUP-A", None, 1_000, 0, &parent),
            member("DUP-A", None, 1_000, 0, &sub),
        ],
    );
    let err = consolidate(req).expect_err("duplicate member must be rejected");
    assert_eq!(
        err,
        ConsolidationError::DuplicateMember {
            member: MemberId("DUP-A".to_string()),
        }
    );
}

#[test]
fn control_ownership_mismatch_is_rejected() {
    let (parent, sub) = pair_books();
    // 50,000/100,000 = 5000bp：恰好半数不构成控制（须严格 > 5000bp）。
    let req = request(
        "OWN-ROOT",
        vec![
            member("OWN-ROOT", None, 1_000, 0, &parent),
            member("OWN-SUB", Some("OWN-ROOT"), 100_000, 50_000, &sub),
        ],
    );
    let err = consolidate(req).expect_err("non-controlling stake must be rejected");
    assert_eq!(
        err,
        ConsolidationError::ControlOwnershipMismatch {
            member: MemberId("OWN-SUB".to_string()),
            ownership_bp: 5000,
        }
    );
}

#[test]
fn ownership_not_representable_is_rejected_without_rounding() {
    let (parent, sub) = pair_books();
    // 1/3 股 = 3333.33bp：不整除 → 显式拒绝，绝不静默舍入。
    let req = request(
        "FRAC-ROOT",
        vec![
            member("FRAC-ROOT", None, 1_000, 0, &parent),
            member("FRAC-SUB", Some("FRAC-ROOT"), 3, 1, &sub),
        ],
    );
    let err = consolidate(req).expect_err("indivisible ownership must be rejected");
    assert!(matches!(
        err,
        ConsolidationError::OwnershipNotRepresentable { .. }
    ));
}

#[test]
fn holding_beyond_issued_is_rejected() {
    let (parent, sub) = pair_books();
    let req = request(
        "HELD-ROOT",
        vec![
            member("HELD-ROOT", None, 1_000, 0, &parent),
            member("HELD-SUB", Some("HELD-ROOT"), 100_000, 120_000, &sub),
        ],
    );
    let err = consolidate(req).expect_err("over-issued holding must be rejected");
    assert!(matches!(
        err,
        ConsolidationError::HoldingBeyondIssued { .. }
    ));
}

#[test]
fn period_coverage_mismatch_is_rejected() {
    // 母公司覆盖 2029-12 + 2030-01；子公司只有 2029-12。
    let parent = super::books_with(
        industrial_chart_v2(),
        vec![
            entry(
                1,
                "2029-12-31",
                BusinessKind::OpeningBalance,
                CashFlowClass::Financing,
                &[
                    (super::acct::BANK, Debit, 1_000_000),
                    (super::acct::CAPITAL, Credit, 1_000_000),
                ],
            ),
            entry(
                2,
                "2030-01-10",
                BusinessKind::CashExpense,
                CashFlowClass::Operating,
                &[
                    (super::acct::ADMIN_EXP, Debit, 1_000),
                    (super::acct::BANK, Credit, 1_000),
                ],
            ),
        ],
    );
    let sub = super::books_with(
        industrial_chart_v2(),
        vec![entry(
            1,
            "2029-12-31",
            BusinessKind::OpeningBalance,
            CashFlowClass::Financing,
            &[
                (super::acct::BANK, Debit, 500_000),
                (super::acct::CAPITAL, Credit, 500_000),
            ],
        )],
    );
    let req = request(
        "PER-ROOT",
        vec![
            member("PER-ROOT", None, 1_000, 0, &parent),
            member("PER-SUB", Some("PER-ROOT"), 100, 80, &sub),
        ],
    );
    let err = consolidate(req).expect_err("period coverage mismatch must be rejected");
    match err {
        ConsolidationError::PeriodCoverageMismatch {
            member, expected, ..
        } => {
            assert_eq!(member, MemberId("PER-SUB".to_string()));
            assert_eq!(expected.len(), 2);
        }
        other => panic!("expected PeriodCoverageMismatch, got {other:?}"),
    }
}

#[test]
fn nested_group_is_rejected_as_unsupported() {
    let (parent, sub) = pair_books();
    // C 的母公司是 B（B 又是 A 的子公司）→ 多层集团：显式不支持。
    let req = request(
        "NST-ROOT",
        vec![
            member("NST-ROOT", None, 1_000, 0, &parent),
            member("NST-B", Some("NST-ROOT"), 100, 80, &sub),
            member("NST-C", Some("NST-B"), 100, 90, &parent),
        ],
    );
    let err = consolidate(req).expect_err("nested group must be rejected");
    assert!(matches!(
        err,
        ConsolidationError::NestedGroupUnsupported { .. }
    ));
}

#[test]
fn member_outside_group_is_rejected() {
    let (parent, sub) = pair_books();
    // OUT 是另一个顶级成员（无母公司）→ 不属于请求合并的集团。
    let req = request(
        "OUT-ROOT",
        vec![
            member("OUT-ROOT", None, 1_000, 0, &parent),
            member("OUT-SUB", Some("OUT-ROOT"), 100, 80, &sub),
            member("OUT-OTHER", None, 1_000, 0, &parent),
        ],
    );
    let err = consolidate(req).expect_err("outside member must be rejected");
    assert!(matches!(err, ConsolidationError::MemberOutsideGroup { .. }));
}

#[test]
fn root_with_holding_is_rejected() {
    let (parent, sub) = pair_books();
    // 根无母公司，自持股权申报必须为零。
    let req = request(
        "RWH-ROOT",
        vec![
            member("RWH-ROOT", None, 1_000, 5, &parent),
            member("RWH-SUB", Some("RWH-ROOT"), 100, 80, &sub),
        ],
    );
    let err = consolidate(req).expect_err("root with holding must be rejected");
    assert_eq!(
        err,
        ConsolidationError::RootWithHolding {
            root: MemberId("RWH-ROOT".to_string()),
            parent_held_shares: 5,
        }
    );
}

#[test]
fn no_subsidiary_returns_typed_not_applicable() {
    let parent = super::books_with(
        AccountChart::generic_v1(),
        vec![entry(
            1,
            "2029-12-31",
            BusinessKind::OpeningBalance,
            CashFlowClass::Financing,
            &[
                (super::acct::BANK, Debit, 1_000_000),
                (super::acct::CAPITAL, Credit, 1_000_000),
            ],
        )],
    );
    let req = request("SOLO", vec![member("SOLO", None, 1_000, 0, &parent)]);
    let err = consolidate(req).expect_err("subsidiary-less company has no consolidation");
    assert_eq!(
        err,
        ConsolidationError::NotApplicable {
            company: MemberId("SOLO".to_string()),
        }
    );
}
