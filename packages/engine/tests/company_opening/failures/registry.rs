//! 注册表级拒绝：未知发行人、无发行人映射、重复公司 id、重复发行映射、
//! 集团环/未知母公司、股本不符、不平衡开局账套。

use super::super::*;
use engine::accounting::AccountingError;
use engine::company::{CompanyError, CompanyRegistry};

/// 未知发行人：上市公司映射的股票不在清单内 → 拒绝（注册表本身保持可用）。
#[test]
fn unknown_issuer_stock_rejected() {
    let registry = CompanyRegistry::new(vec![bare_config(
        CompanySpec {
            id: CompanyId("T-SSE".to_string()),
            name: "虚构发行人".to_string(),
            industry: IndustryId("machinery".to_string()),
            kind: CompanyKind::Industrial,
            listed_stock: Some(StockCode("688001".to_string())),
            issued_shares: 100_000_000,
            group_parent: None,
        },
        generic_opening(vec![
            opening_line("1002", PostingSide::Debit, 10_000_000),
            opening_line("4001", PostingSide::Credit, 10_000_000),
        ]),
    )])
    .expect("registry itself constructs");

    let stocks = default_stock_specs();
    let stock_refs: Vec<(StockCode, u64)> = stocks
        .iter()
        .map(|stock| (stock.code.clone(), stock.total_shares))
        .collect();
    match registry.validate_issuer_mapping(&stock_refs) {
        Err(CompanyError::UnknownIssuerStock { company, stock }) => {
            assert_eq!(company, CompanyId("T-SSE".to_string()));
            assert_eq!(stock.0, "688001");
        }
        other => panic!("expected UnknownIssuerStock, got {other:?}"),
    }
}

/// 清单内股票没有发行人 → 拒绝（发行映射必须是完整覆盖）。
#[test]
fn unmapped_stock_rejected() {
    let registry = default_registry();
    let mut stock_refs: Vec<(StockCode, u64)> = default_stock_specs()
        .iter()
        .map(|stock| (stock.code.clone(), stock.total_shares))
        .collect();
    stock_refs.push((StockCode("688001".to_string()), 100_000_000));
    match registry.validate_issuer_mapping(&stock_refs) {
        Err(CompanyError::UnmappedStock { stock }) => assert_eq!(stock.0, "688001"),
        other => panic!("expected UnmappedStock, got {other:?}"),
    }
}

/// 重复公司 id → 拒绝，且注册表整体不产生。
#[test]
fn duplicate_company_id_rejected() {
    let first = bare_config(
        unlisted_spec("T-DUP", CompanyKind::Industrial, 100),
        generic_opening(vec![
            opening_line("1002", PostingSide::Debit, 100),
            opening_line("4001", PostingSide::Credit, 100),
        ]),
    );
    let second = bare_config(
        unlisted_spec("T-DUP", CompanyKind::Bank, 200),
        generic_opening(vec![
            opening_line("1002", PostingSide::Debit, 200),
            opening_line("4001", PostingSide::Credit, 200),
        ]),
    );
    match CompanyRegistry::new(vec![first, second]) {
        Err(CompanyError::DuplicateCompanyId { company }) => {
            assert_eq!(company, CompanyId("T-DUP".to_string()))
        }
        other => panic!("expected DuplicateCompanyId, got {other:?}"),
    }
}

/// 两家公司映射同一股票 → 拒绝并携带两家公司上下文。
#[test]
fn duplicate_issuer_stock_rejected() {
    let issuer = || {
        bare_config(
            CompanySpec {
                id: CompanyId("T-ISS".to_string()),
                name: "虚构发行人".to_string(),
                industry: IndustryId("machinery".to_string()),
                kind: CompanyKind::Industrial,
                listed_stock: Some(StockCode("600101".to_string())),
                issued_shares: 8_928_571_429,
                group_parent: None,
            },
            generic_opening(vec![
                opening_line("1002", PostingSide::Debit, 10_000_000),
                opening_line("4001", PostingSide::Credit, 10_000_000),
            ]),
        )
    };
    let mut other = issuer();
    other.spec.id = CompanyId("T-ISS-2".to_string());
    match CompanyRegistry::new(vec![issuer(), other]) {
        Err(CompanyError::DuplicateIssuerStock {
            stock,
            company,
            first,
        }) => {
            assert_eq!(stock.0, "600101");
            assert_eq!(company, CompanyId("T-ISS-2".to_string()));
            assert_eq!(first, CompanyId("T-ISS".to_string()));
        }
        other => panic!("expected DuplicateIssuerStock, got {other:?}"),
    }
}

/// 集团环（互相为母 / 自我为母 / 深链成环）与未知母公司 → 拒绝。
#[test]
fn group_cycle_and_unknown_parent_rejected() {
    let config = |id: &str, parent: Option<&str>| {
        let mut spec = unlisted_spec(id, CompanyKind::Industrial, 100);
        spec.group_parent = parent.map(|parent_id| CompanyId(parent_id.to_string()));
        bare_config(
            spec,
            generic_opening(vec![
                opening_line("1002", PostingSide::Debit, 100),
                opening_line("4001", PostingSide::Credit, 100),
            ]),
        )
    };
    // A → B → A：环。
    match CompanyRegistry::new(vec![config("T-A", Some("T-B")), config("T-B", Some("T-A"))]) {
        Err(CompanyError::GroupCycle { company }) => {
            assert!(
                company == CompanyId("T-A".to_string()) || company == CompanyId("T-B".to_string())
            )
        }
        other => panic!("expected GroupCycle, got {other:?}"),
    }
    // 自我为母：环。
    match CompanyRegistry::new(vec![config("T-A", Some("T-A"))]) {
        Err(CompanyError::GroupCycle { .. }) => {}
        other => panic!("expected GroupCycle for self parent, got {other:?}"),
    }
    // 母公司不在集合内。
    match CompanyRegistry::new(vec![config("T-A", Some("T-GHOST"))]) {
        Err(CompanyError::UnknownGroupParent { company, parent }) => {
            assert_eq!(company, CompanyId("T-A".to_string()));
            assert_eq!(parent, CompanyId("T-GHOST".to_string()));
        }
        other => panic!("expected UnknownGroupParent, got {other:?}"),
    }
    // 深链 A → B → C → A 同样拒绝。
    match CompanyRegistry::new(vec![
        config("T-A", Some("T-B")),
        config("T-B", Some("T-C")),
        config("T-C", Some("T-A")),
    ]) {
        Err(CompanyError::GroupCycle { .. }) => {}
        other => panic!("expected GroupCycle for deep chain, got {other:?}"),
    }
}

/// 初始股本不符：公司 issued_shares ≠ 股票 total_shares → 拒绝并携带两侧数值。
#[test]
fn issued_shares_mismatch_rejected() {
    let registry = CompanyRegistry::new(vec![bare_config(
        CompanySpec {
            id: CompanyId("T-MISMATCH".to_string()),
            name: "虚构发行人".to_string(),
            industry: IndustryId("machinery".to_string()),
            kind: CompanyKind::Industrial,
            listed_stock: Some(StockCode("600101".to_string())),
            issued_shares: 100,
            group_parent: None,
        },
        generic_opening(vec![
            opening_line("1002", PostingSide::Debit, 10_000_000),
            opening_line("4001", PostingSide::Credit, 10_000_000),
        ]),
    )])
    .expect("registry constructs; mapping validated separately");
    let stock_refs = vec![(StockCode("600101".to_string()), 8_928_571_429u64)];
    match registry.validate_issuer_mapping(&stock_refs) {
        Err(CompanyError::IssuedSharesMismatch {
            company,
            issued_shares,
            total_shares,
            ..
        }) => {
            assert_eq!(company, CompanyId("T-MISMATCH".to_string()));
            assert_eq!(issued_shares, 100);
            assert_eq!(total_shares, 8_928_571_429);
        }
        other => panic!("expected IssuedSharesMismatch, got {other:?}"),
    }
}

/// 不平衡开局账套 → 拒绝（经 Books 复式验证路径），注册表整体不产生。
#[test]
fn unbalanced_opening_rejected() {
    let good = bare_config(
        unlisted_spec("T-GOOD", CompanyKind::Industrial, 1_000),
        generic_opening(vec![
            opening_line("1002", PostingSide::Debit, 100),
            opening_line("4001", PostingSide::Credit, 100),
        ]),
    );
    let bad = bare_config(
        unlisted_spec("T-BAD", CompanyKind::Industrial, 1_000),
        generic_opening(vec![
            opening_line("1002", PostingSide::Debit, 100),
            opening_line("4001", PostingSide::Credit, 90),
        ]),
    );
    match CompanyRegistry::new(vec![good, bad]) {
        Err(CompanyError::OpeningPost { company, source }) => {
            assert_eq!(company, CompanyId("T-BAD".to_string()));
            match source {
                AccountingError::BatchAborted { cause, .. } => {
                    assert!(
                        matches!(*cause.clone(), AccountingError::Unbalanced { .. }),
                        "root cause must be the unbalanced entry, got {cause:?}"
                    );
                }
                other => panic!("expected BatchAborted wrapping Unbalanced, got {other:?}"),
            }
        }
        other => panic!("expected OpeningPost, got {other:?}"),
    }
}
