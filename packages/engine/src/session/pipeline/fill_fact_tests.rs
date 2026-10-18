//! 任务65 #11（收据单次规范化）红测。
//!
//! 目标契约：`FillFact` 在 ReceiptAggregation 产出点一次派生（typed、构造即校验），
//! settlement / retail / institutional / lifecycle 消费 typed 结果，不再各自从
//! `EnvelopeReceipt` 的 before/after 字段重推 (qty, gross)。
//! FillFact 为内部派生视图：不 derive serde、不入存档/wire/business_state_hash
//! （审计记录仍是 EnvelopeReceipt）。
//!
//! 红态：`fill_fact` 模块、`apply_owned_receipt_transaction` 三元组返回、
//! `ReceiptSettlementPlan::from_fill_facts` 均不存在——本文件编译失败即为红。

use super::fill_fact::{FillFact, FillFactInput, FILL_FACT_DERIVATION_COUNT};
use super::ledger_tests::*;
use super::receipt_aggregation::apply_owned_receipt_transaction;
use super::settlement::ReceiptSettlementPlan;
use super::ReceiptIndex;
use crate::session::{account_book::AccountBook, account_paged_map::AccountPagedMap};
use crate::Money;
use crate::{Account, AccountId, AccountKind, RetailExperienceState};
use std::collections::BTreeMap;

/// 与 account_settlement_tests 同构的最小 retail 账户 fixture（只服务于本文件）。
fn settlement_accounts() -> AccountBook {
    BTreeMap::from([(
        AccountId(1),
        Account::new(
            AccountId(1),
            AccountKind::Retail,
            Money::from_cents(200_000),
        ),
    )])
    .into()
}

fn retail_experience() -> AccountPagedMap<RetailExperienceState> {
    BTreeMap::from([(
        AccountId(1),
        RetailExperienceState::without_equity_reference(),
    )])
    .into()
}

/// T1：Fill 事实在 ReceiptAggregation 产出点一次派生，index 与权威收据链一致。
#[test]
fn fill_facts_are_derived_once_at_receipt_aggregation_output() {
    let ledger = created_ledger();
    let batch = vec![receipt(0)];

    let (_candidate, receipts) = apply_owned_receipt_transaction(
        ledger,
        Vec::new(),
        vec![batch],
        vec![key()],
        &crate::ComputeContext::shared_default().unwrap(),
    )
    .unwrap();
    let fill_facts = receipts.fill_facts().iter().collect::<Vec<_>>();
    assert_eq!(receipts.len(), 1);
    assert_eq!(fill_facts.len(), 1);
    // 每条 Fill 恰好一条 typed fact：全局 index 对得上，qty/gross 来自权威链而非重推。
    assert_eq!(fill_facts[0].index(), ReceiptIndex(receipts[0].index));
    assert_eq!(fill_facts[0].qty(), 100);
    assert_eq!(fill_facts[0].gross(), Money::from_cents(100));
    assert_eq!(fill_facts[0].charged(), receipts[0].charged);
    assert_eq!(fill_facts[0].key(), &receipts[0].envelope);
}

/// T2：settlement 从 FillFact 消费的结果与旧路径（直接从收据推导）逐字段一致。
#[test]
fn settlement_from_fill_facts_matches_legacy_receipt_derivation() {
    let receipts = vec![receipt(0)];
    let (_ledger, indexed) = apply_owned_receipt_transaction(
        created_ledger(),
        Vec::new(),
        vec![receipts],
        vec![key()],
        &crate::ComputeContext::shared_default().unwrap(),
    )
    .unwrap();

    let accounts = settlement_accounts();
    let legacy = ReceiptSettlementPlan::from_receipts(&indexed)
        .unwrap()
        .prepare_accounts(
            &crate::ComputeContext::shared_default().unwrap(),
            &accounts,
            true,
        )
        .unwrap();
    let typed = ReceiptSettlementPlan::from_fill_facts(indexed.fill_facts().iter())
        .unwrap()
        .prepare_accounts(
            &crate::ComputeContext::shared_default().unwrap(),
            &accounts,
            true,
        )
        .unwrap();

    // 两条路径对同一账户影子必须产出相同结算结果（金额、数量、审计计数）。
    // Account/Position 未派生 PartialEq，用可序列化字段（cash/positions）做字节级比较。
    assert_eq!(legacy.0.len(), typed.0.len());
    for (account_id, legacy_account) in &legacy.0 {
        let typed_account = typed
            .0
            .get(account_id)
            .unwrap_or_else(|| panic!("typed 结算缺少账户 {account_id:?}"));
        assert_eq!(legacy_account.cash(), typed_account.cash());
        assert_eq!(
            serde_json::to_vec(legacy_account.positions()).unwrap(),
            serde_json::to_vec(typed_account.positions()).unwrap()
        );
    }
    assert_eq!(legacy.1, typed.1);
}

/// T3：同一批收据经 settlement+retail+institutional 全消费后，派生只发生一次。
/// 同时断言 seen 身份数 == Fill 数（institutional 分支不再重插 seen）。
#[test]
fn one_receipt_batch_is_normalized_in_a_single_derivation_pass() {
    use super::account_settlement::prepare_settlement_transaction;
    use super::retail_projection::RetailProjectionSeen;

    // 两条同账户 Fill（同一 envelope 的连续腿）组成一批；全局 index 必须互异。
    let mut receipts = vec![receipt(0)];
    let mut second = receipt(1);
    second.index = 1;
    receipts.push(second);
    let accounts = settlement_accounts();
    let experience = retail_experience();
    let seen = RetailProjectionSeen::default();

    FILL_FACT_DERIVATION_COUNT.with(|count| count.set(0));
    let prepared =
        prepare_settlement_transaction(&accounts, &experience, &seen, 10, &receipts, true).unwrap();

    // settlement + retail + institutional 三类消费共享一次派生。
    FILL_FACT_DERIVATION_COUNT.with(|count| assert_eq!(count.get(), 1));
    // seen 单遍插入：2 条 Fill 恰好 2 个身份，没有 institutional 侧的第二轮。
    let identities = prepared.seen.authoritative_identities();
    assert_eq!(identities.len(), receipts.len());
}

/// T4：FillFact 构造即校验——非正数量/非正成交额必须显式 Err（铁律二，不静默吞）。
#[test]
fn fill_fact_construction_rejects_non_positive_qty_or_gross() {
    let identity = key();
    let valid =
        |qty_before: u32, qty_after: u32, value_before: i64, value_after: i64| FillFactInput {
            index: ReceiptIndex(0),
            key: &identity,
            qty_before,
            qty_after,
            value_before: Money::from_cents(value_before),
            value_after: Money::from_cents(value_after),
            charged: super::FeeComponents::ZERO,
        };

    // 合法腿：qty>0 且 gross>0。
    assert!(FillFact::new(valid(100, 0, 0, 100)).is_ok());
    // qty 回退或为零。
    assert!(FillFact::new(valid(100, 100, 0, 100)).is_err());
    assert!(FillFact::new(valid(100, 150, 0, 100)).is_err());
    // 非正 gross。
    assert!(FillFact::new(valid(100, 0, 100, 100)).is_err());
    assert!(FillFact::new(valid(100, 0, 100, 50)).is_err());
}
