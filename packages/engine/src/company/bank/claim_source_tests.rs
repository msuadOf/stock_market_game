use super::*;
use crate::accounting::{
    AccountingAmount, BusinessEventId, BusinessKind, IncomeTaxPolicy, PostingSide,
};
use crate::calendar::CivilDate;
use crate::company::{CounterpartyId, CounterpartyKind, ExternalCounterparty};

fn date(text: &str) -> CivilDate {
    CivilDate::from_iso(text).expect("明确自然日期")
}

fn amount(cents: i128) -> AccountingAmount {
    AccountingAmount::from_cents(cents)
}

fn bank() -> BankBooks {
    BankBooks::new(BankConfig {
        chart: bank_account_chart(),
        as_of: date("2030-01-01"),
        opening_lines: vec![
            line("1003", PostingSide::Debit, amount(20_000_000)),
            line("4001", PostingSide::Credit, amount(20_000_000)),
        ],
        counterparties: vec![ExternalCounterparty {
            id: CounterpartyId("borrower".into()),
            kind: CounterpartyKind::Customer,
            name: "虚拟借款客户".into(),
        }],
        ecl_policy: EclPolicy {
            version: 1,
            stage1_default: vec![EclScenario {
                weight_bp: 10000,
                pd_bp: 0,
                lgd_bp: 0,
            }],
            lifetime_default: vec![EclScenario {
                weight_bp: 10000,
                pd_bp: 0,
                lgd_bp: 0,
            }],
        },
        income_tax_policy: IncomeTaxPolicy {
            rate_bp: 2500,
            loss_carryforward_years: 5,
        },
    })
    .expect("真实银行Owner")
}

fn issue(bank: &mut BankBooks, loan: &str, principal: i128, rate: i32) -> BusinessEventId {
    let events = bank
        .issue_loan(
            BankProductKind::TermLoan,
            ContractId(loan.into()),
            &CounterpartyId("borrower".into()),
            amount(principal),
            rate,
            date("2030-01-01"),
            date("2030-01-03"),
        )
        .expect("真实发放贷款");
    let sources: Vec<_> = events
        .into_iter()
        .filter(|source| {
            bank.books()
                .journal()
                .entries()
                .any(|entry| entry.source == *source && entry.kind == BusinessKind::LoanIssued)
        })
        .collect();
    assert_eq!(sources.len(), 1);
    sources[0]
}

#[test]
fn positive_accrual_returns_exact_sources_for_equal_amount_loans() {
    let mut bank = bank();
    let first = issue(&mut bank, "loan-a", 3_650_000, 1000);
    let second = issue(&mut bank, "loan-b", 3_650_000, 1000);
    let items = bank
        .accrue_loan_interest(date("2030-01-02"))
        .expect("真实同日同额计提");
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].amount, items[1].amount);
    let sources: Vec<_> = items
        .iter()
        .map(|item| {
            item.source
                .expect("每笔正额结果必须携带真实来源，不能按金额猜")
        })
        .collect();
    assert_ne!(sources[0], sources[1]);
    for item in &items {
        let source = item.source.expect("真实来源");
        let entry = bank
            .books()
            .journal()
            .entries()
            .find(|entry| entry.source == source)
            .expect("实际凭证");
        assert_eq!(entry.kind, BusinessKind::LoanInterestAccrued);
        assert_eq!(entry.date, date("2030-01-02"));
        assert!(entry
            .lines
            .iter()
            .any(|entry_line| entry_line.account.0 == "1131"
                && entry_line.side == PostingSide::Debit
                && entry_line.amount == item.amount));
        assert!(bank
            .loan_claim_sources()
            .iter()
            .any(|fact| fact.loan == item.loan
                && fact.source == source
                && fact.kind == BankLoanClaimKind::Interest));
    }
    for (loan, source) in [("loan-a", first), ("loan-b", second)] {
        assert!(bank
            .loan_claim_sources()
            .iter()
            .any(|fact| fact.loan.0 == loan
                && fact.source == source
                && fact.kind == BankLoanClaimKind::Principal));
    }
}

#[test]
fn zero_accrual_has_no_posted_source_and_retry_produces_no_new_facts() {
    let mut bank = bank();
    issue(&mut bank, "zero-rate", 100, 0);
    let before = bank.books().journal().entry_count();
    let items = bank
        .accrue_loan_interest(date("2030-01-02"))
        .expect("零额仍推进计息日期");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].source, None);
    assert_eq!(items[0].amount, AccountingAmount::ZERO);
    assert_eq!(bank.books().journal().entry_count(), before);
    assert_eq!(
        bank.loan_claim_sources().len(),
        1,
        "只保留真实本金来源，不为零额造利息来源"
    );
    let before = bank.clone();
    assert!(bank
        .accrue_loan_interest(date("2030-01-02"))
        .expect("同日重试")
        .is_empty());
    assert_eq!(bank, before);
}

#[test]
fn restore_rejects_source_kind_not_matching_real_journal() {
    let mut bank = bank();
    let source = issue(&mut bank, "loan-a", 3_650_000, 1000);
    bank.loan_claim_sources.clear();
    bank.loan_claim_sources.push(BankLoanClaimSource {
        loan: ContractId("loan-a".into()),
        source,
        kind: BankLoanClaimKind::Interest,
    });
    assert!(
        serde_json::from_value::<BankBooks>(
            serde_json::to_value(bank).expect("真实账簿＋错配来源")
        )
        .is_err(),
        "真实本金来源不能冒充利息来源"
    );
}

#[test]
fn source_facts_survive_strict_restore_and_reject_missing_or_duplicate_coverage() {
    let mut bank = bank();
    issue(&mut bank, "loan-a", 3_650_000, 1000);
    bank.accrue_loan_interest(date("2030-01-02"))
        .expect("真实利息来源");
    assert_eq!(
        bank.loan_claim_sources().len(),
        2,
        "本金与实际利息分别记录来源"
    );
    let restored: BankBooks =
        serde_json::from_value(serde_json::to_value(&bank).expect("真实保存")).expect("严格恢复");
    assert_eq!(restored, bank);
    let mut missing = serde_json::to_value(&bank).expect("真实保存");
    missing["loan_claim_sources"] = serde_json::json!([]);
    assert!(
        serde_json::from_value::<BankBooks>(missing).is_err(),
        "全部真实Loan凭证必须有来源身份覆盖"
    );
    let mut duplicate = serde_json::to_value(&bank).expect("真实保存");
    let record = duplicate["loan_claim_sources"][0].clone();
    duplicate["loan_claim_sources"]
        .as_array_mut()
        .expect("来源事实")
        .push(record);
    assert!(
        serde_json::from_value::<BankBooks>(duplicate).is_err(),
        "一笔实际来源不能属于两个事实"
    );
}

#[test]
fn restore_rejects_source_cursor_collision_including_empty_loans() {
    let mut bank = bank();
    let source = issue(&mut bank, "loan-a", 3_650_000, 1000);
    let saved = serde_json::to_value(&bank).expect("实际本金来源");
    let mut collision = saved;
    collision["next_event_id"] = serde_json::json!(source.value());
    assert!(serde_json::from_value::<BankBooks>(collision).is_err());
    let mut empty = self::bank();
    empty.next_event_id = 0;
    assert!(
        serde_json::from_value::<BankBooks>(serde_json::to_value(empty).expect("真实开局账"))
            .is_err(),
        "空贷款不能绕过已过账开局事件游标"
    );
    let mut sparse = bank;
    sparse.next_event_id = 99;
    let restored: BankBooks =
        serde_json::from_value(serde_json::to_value(&sparse).expect("游标合法空洞"))
            .expect("高于实际凭证不要求连续");
    assert_eq!(restored, sparse);
}

#[test]
fn restore_rejects_unknown_loan_fields_and_repeated_journal_source() {
    let mut bank = bank();
    let source = issue(&mut bank, "loan-a", 3_650_000, 1000);
    let saved = serde_json::to_value(&bank).expect("实际本金来源");
    let mut unknown = saved.clone();
    unknown["loans"]["loan-a"]["guessed_source"] = serde_json::json!(source.value());
    assert!(serde_json::from_value::<BankBooks>(unknown).is_err());
    let mut duplicate = saved;
    let entry = duplicate["books"]["journal"]["batches"]
        .as_array()
        .expect("真实批次")
        .iter()
        .flat_map(|batch| batch.as_array().expect("真实分录"))
        .find(|entry| entry["source"] == serde_json::json!(source.value()))
        .expect("实际本金")
        .clone();
    duplicate["books"]["journal"]["batches"]
        .as_array_mut()
        .expect("真实批次")
        .push(serde_json::json!([entry]));
    assert!(serde_json::from_value::<BankBooks>(duplicate).is_err());
}

#[test]
fn original_terms_and_positive_overdue_interest_survive_restore() {
    let mut bank = bank();
    issue(&mut bank, "loan-a", 3_650_000, 1000);
    let items = bank
        .accrue_loan_interest(date("2030-01-04"))
        .expect("逾期继续原合同计息");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].amount, amount(3000));
    assert!(items[0].source.is_some());
    let restored: BankBooks =
        serde_json::from_value(serde_json::to_value(&bank).expect("实际来源"))
            .expect("恢复逾期贷款");
    let loan = restored.loan(&ContractId("loan-a".into())).expect("原贷款");
    assert_eq!(loan.start_date(), date("2030-01-01"));
    assert_eq!(loan.maturity_date(), date("2030-01-03"));
    assert_eq!(loan.last_accrual_date(), date("2030-01-04"));
    assert_eq!(bank, restored);
}

#[test]
fn failed_loan_issue_and_accrual_do_not_install_sources_or_states() {
    let mut bank = bank();
    let before = bank.clone();
    assert!(bank
        .issue_loan(
            BankProductKind::TermLoan,
            ContractId("too-large".into()),
            &CounterpartyId("borrower".into()),
            amount(30_000_000),
            1000,
            date("2030-01-01"),
            date("2030-01-03")
        )
        .is_err());
    assert_eq!(bank, before);
    issue(&mut bank, "loan-a", 3_650_000, 1000);
    let before = bank.clone();
    assert!(bank.accrue_loan_interest(date("2029-12-31")).is_err());
    assert_eq!(bank, before);
    bank.next_event_id = u64::MAX;
    let before = bank.clone();
    assert!(bank.accrue_loan_interest(date("2030-01-02")).is_err());
    assert_eq!(bank, before);
    bank.next_event_id = 1;
    let before = bank.clone();
    assert!(bank.accrue_loan_interest(date("2030-01-02")).is_err());
    assert_eq!(bank, before);
}
