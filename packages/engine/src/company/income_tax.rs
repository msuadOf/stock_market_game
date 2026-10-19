use std::collections::BTreeMap;

use crate::accounting::{
    AccountElement, AccountingAmount, AccountingError, AccountingPeriod, Books, BusinessEventId,
    BusinessKind, CashFlowClass, IncomeTaxComputation, IncomeTaxPolicy, JournalEntry, JournalLine,
    LedgerAccountId, LossEntry, PostingSide,
};
use crate::calendar::{CivilDate, CIVIL_YEAR_MAX, CIVIL_YEAR_MIN};

pub(crate) const DTA: &str = "1811";
pub(crate) const CIT_PAYABLE: &str = "222104";
pub(crate) const TAX_EXP: &str = "6801";

#[derive(Clone, Eq, PartialEq, Debug)]
pub struct IncomeTaxOutcome {
    pub event: Option<BusinessEventId>,
    pub pretax: AccountingAmount,
    pub current_tax: AccountingAmount,
    pub current_tax_delta: AccountingAmount,
    pub loss_offset_used: AccountingAmount,
    pub loss_added: AccountingAmount,
    pub losses_expired: AccountingAmount,
    pub deferred_delta: AccountingAmount,
}

#[derive(Clone, Eq, PartialEq, Debug, thiserror::Error)]
pub enum IncomeTaxOwnerError {
    #[error("inconsistent income tax state: {detail}")]
    StateInconsistent { detail: String },
    #[error(transparent)]
    Accounting(#[from] AccountingError),
}

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct IncomeTaxPosition {
    loss_pool: Vec<LossEntry>,
    assessments: BTreeMap<i32, IncomeTaxAssessment>,
    initial_deferred_tax_asset: AccountingAmount,
    restatements: BTreeMap<BusinessEventId, AccountingPeriod>,
}

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct IncomeTaxAssessment {
    pretax: AccountingAmount,
    current_tax: AccountingAmount,
    opening_loss_pool: Vec<LossEntry>,
    deferred_tax_asset: AccountingAmount,
}

pub(crate) struct TaxCascadePreview {
    pub(crate) position: IncomeTaxPosition,
    pub(crate) computation: IncomeTaxComputation,
    pub(crate) entries: Vec<JournalEntry>,
    pub(crate) current_tax_delta: AccountingAmount,
    pub(crate) deferred_delta: AccountingAmount,
    pub(crate) next_event_id: u64,
}

impl IncomeTaxPosition {
    pub(crate) fn latest_assessment_year(&self) -> Option<i32> {
        self.assessments.keys().next_back().copied()
    }
    pub(crate) fn restatements(&self) -> &BTreeMap<BusinessEventId, AccountingPeriod> {
        &self.restatements
    }

    pub(crate) fn new(books: &Books) -> Result<Self, IncomeTaxOwnerError> {
        validate_tax_accounts(books)?;
        let initial_deferred_tax_asset = books
            .ledger()
            .account_net_debit(&crate::accounting::LedgerAccountId(DTA.into()))?;
        if initial_deferred_tax_asset.is_negative() {
            return Err(invalid("开局递延所得税资产不得为负"));
        }
        if initial_deferred_tax_asset != opening_deferred_asset(books)? {
            return Err(invalid("首次税务 Owner 必须从真实开局递延税资产构造"));
        }
        Ok(Self {
            loss_pool: Vec::new(),
            assessments: BTreeMap::new(),
            initial_deferred_tax_asset,
            restatements: BTreeMap::new(),
        })
    }
    pub(crate) fn loss_pool(&self) -> &[LossEntry] {
        &self.loss_pool
    }

    pub(crate) fn validate(&self, policy: &IncomeTaxPolicy) -> Result<(), IncomeTaxOwnerError> {
        validate_pool(&self.loss_pool)?;
        if self.initial_deferred_tax_asset.is_negative() {
            return Err(invalid("初始递延税资产不得为负"));
        }
        let mut previous: Option<(i32, Vec<LossEntry>)> = None;
        for (&year, assessment) in &self.assessments {
            if !(CIVIL_YEAR_MIN..=CIVIL_YEAR_MAX).contains(&year) {
                return Err(invalid("评估年度越界"));
            }
            validate_pool(&assessment.opening_loss_pool)?;
            if assessment
                .opening_loss_pool
                .iter()
                .any(|entry| entry.origin_year >= year)
            {
                return Err(invalid("年度期初池包含本年或未来亏损"));
            }
            if let Some((previous_year, pool)) = &previous {
                if *previous_year + 1 != year || pool != &assessment.opening_loss_pool {
                    return Err(invalid("年度评估链年份或期初期末池不连续"));
                }
            }
            let computation =
                policy.compute(assessment.pretax, year, &assessment.opening_loss_pool)?;
            if computation.current_tax != assessment.current_tax
                || computation.deferred_tax_asset != assessment.deferred_tax_asset
            {
                return Err(invalid("年度评估税额或递延税资产不符合保存的年初基准"));
            }
            previous = Some((year, computation.ending_pool));
        }
        if let Some((_, ending_pool)) = previous {
            if ending_pool != self.loss_pool {
                return Err(invalid("最终亏损池与年度评估链不符"));
            }
        }
        Ok(())
    }

    pub(crate) fn validate_books(&self, books: &Books) -> Result<(), IncomeTaxOwnerError> {
        validate_tax_accounts(books)?;
        validate_tax_sources(books)?;
        validate_restatements(&self.restatements, books)?;
        if self.assessments.is_empty() {
            let dta = books
                .ledger()
                .account_net_debit(&crate::accounting::LedgerAccountId(DTA.into()))?;
            if self.initial_deferred_tax_asset != dta
                || books.journal().entries().any(|entry| {
                    entry.kind == BusinessKind::TaxAccrual
                        && entry.lines.iter().any(|line| {
                            [CIT_PAYABLE, DTA, TAX_EXP].contains(&line.account.0.as_str())
                        })
                })
            {
                return Err(invalid(
                    "空年度评估链不得隐藏已存在的税务计提凭证或递延税资产变动",
                ));
            }
        }
        if self.initial_deferred_tax_asset != opening_deferred_asset(books)? {
            return Err(invalid("初始递延税资产与真实开局凭证不符"));
        }
        if let Some(latest) = self.assessments.values().next_back() {
            let dta = books
                .ledger()
                .account_net_debit(&crate::accounting::LedgerAccountId(DTA.into()))?;
            if dta != latest.deferred_tax_asset {
                return Err(invalid("递延所得税资产总账余额与最新年度评估不符"));
            }
            let mut recorded_tax = AccountingAmount::ZERO;
            for entry in books
                .journal()
                .entries()
                .filter(|entry| entry.kind == BusinessKind::TaxAccrual)
            {
                for line in entry
                    .lines
                    .iter()
                    .filter(|line| line.account.0 == CIT_PAYABLE)
                {
                    recorded_tax = if line.side == PostingSide::Credit {
                        recorded_tax.add(line.amount)?
                    } else {
                        recorded_tax.sub(line.amount)?
                    };
                }
            }
            let expected_tax = self
                .assessments
                .values()
                .try_fold(AccountingAmount::ZERO, |total, state| {
                    total.add(state.current_tax)
                })?;
            if recorded_tax != expected_tax {
                return Err(invalid("当期所得税累计计提凭证与年度评估链不符"));
            }
        }
        Ok(())
    }
}

fn invalid(detail: &str) -> IncomeTaxOwnerError {
    IncomeTaxOwnerError::StateInconsistent {
        detail: detail.into(),
    }
}

fn validate_tax_accounts(books: &Books) -> Result<(), IncomeTaxOwnerError> {
    for (code, element) in [
        (DTA, AccountElement::Asset),
        (CIT_PAYABLE, AccountElement::Liability),
        (TAX_EXP, AccountElement::Expense),
    ] {
        let definition = books
            .ledger()
            .chart()
            .get(&LedgerAccountId(code.into()))
            .ok_or_else(|| invalid(&format!("所得税 Owner 缺少权威科目 {code}")))?;
        if definition.element != element || definition.is_cash || definition.is_contra {
            return Err(invalid(&format!(
                "所得税科目 {code} 的要素、现金或备抵语义不正确"
            )));
        }
    }
    Ok(())
}

fn opening_deferred_asset(books: &Books) -> Result<AccountingAmount, IncomeTaxOwnerError> {
    let mut balance = AccountingAmount::ZERO;
    for entry in books
        .journal()
        .entries()
        .filter(|entry| entry.kind == BusinessKind::OpeningBalance)
    {
        for line in entry.lines.iter().filter(|line| line.account.0 == DTA) {
            balance = if line.side == PostingSide::Debit {
                balance.add(line.amount)?
            } else {
                balance.sub(line.amount)?
            };
        }
    }
    Ok(balance)
}

fn validate_tax_sources(books: &Books) -> Result<(), IncomeTaxOwnerError> {
    for entry in books.journal().entries() {
        let tax_lines = entry
            .lines
            .iter()
            .filter(|line| [DTA, CIT_PAYABLE, TAX_EXP].contains(&line.account.0.as_str()))
            .collect::<Vec<_>>();
        if tax_lines.is_empty() || entry.kind == BusinessKind::OpeningBalance {
            continue;
        }
        if entry.kind == BusinessKind::TaxAccrual {
            if entry.cash_flow != CashFlowClass::NonCash || tax_lines.len() != entry.lines.len() {
                return Err(invalid("所得税计提必须为纯税务非现金凭证"));
            }
        } else if entry.kind == BusinessKind::TaxPayment {
            if tax_lines
                .iter()
                .any(|line| line.account.0 != CIT_PAYABLE || line.side != PostingSide::Debit)
                || entry.cash_flow != CashFlowClass::Operating
                || entry.lines.iter().any(|line| {
                    line.account.0 != CIT_PAYABLE
                        && (line.side != PostingSide::Credit
                            || !books.ledger().chart().is_cash(&line.account))
                })
            {
                return Err(invalid(
                    "所得税缴纳只能借记应交所得税、贷记实际现金，不支持退款或非现金抵税",
                ));
            }
        } else {
            return Err(invalid("所得税 Owner 凭证不得隐藏为普通经营业务"));
        }
    }
    Ok(())
}

fn validate_pool(pool: &[LossEntry]) -> Result<(), IncomeTaxOwnerError> {
    for (index, entry) in pool.iter().enumerate() {
        if !entry.remaining.is_positive()
            || !(CIVIL_YEAR_MIN..=CIVIL_YEAR_MAX).contains(&entry.origin_year)
            || (index > 0 && pool[index - 1].origin_year >= entry.origin_year)
        {
            return Err(invalid("亏损池必须按有效起源年严格递增且金额为正"));
        }
    }
    Ok(())
}

fn validate_restatements(
    adjustments: &BTreeMap<BusinessEventId, AccountingPeriod>,
    books: &Books,
) -> Result<(), IncomeTaxOwnerError> {
    for (&source, &target) in adjustments {
        let date = books
            .journal()
            .posted_date(source)
            .ok_or_else(|| invalid("税务重述引用未知凭证来源"))?;
        if target >= AccountingPeriod::from_ymd(date.year(), date.month())? {
            return Err(invalid("税务重述必须作用于实际过账期间之前的历史期间"));
        }
    }
    Ok(())
}

fn year_pretax(
    books: &Books,
    adjustments: &BTreeMap<BusinessEventId, AccountingPeriod>,
) -> Result<BTreeMap<i32, AccountingAmount>, IncomeTaxOwnerError> {
    let mut totals = BTreeMap::new();
    for entry in books.journal().entries() {
        let year = adjustments
            .get(&entry.source)
            .copied()
            .unwrap_or_else(|| entry.period())
            .year();
        for line in &entry.lines {
            let definition = books
                .ledger()
                .chart()
                .get(&line.account)
                .ok_or_else(|| invalid("凭证科目不在权威科目表中"))?;
            let delta = match (definition.element, line.side) {
                (AccountElement::Revenue, PostingSide::Credit)
                | (AccountElement::Expense, PostingSide::Debit)
                    if line.account.0 != TAX_EXP =>
                {
                    if definition.element == AccountElement::Revenue {
                        line.amount
                    } else {
                        line.amount.neg()?
                    }
                }
                (AccountElement::Revenue, PostingSide::Debit)
                | (AccountElement::Expense, PostingSide::Credit)
                    if line.account.0 != TAX_EXP =>
                {
                    if definition.element == AccountElement::Revenue {
                        line.amount.neg()?
                    } else {
                        line.amount
                    }
                }
                _ => continue,
            };
            let total = totals.entry(year).or_insert(AccountingAmount::ZERO);
            *total = total.add(delta)?;
        }
    }
    Ok(totals)
}

fn tax_lines(
    current_delta: AccountingAmount,
    deferred_delta: AccountingAmount,
) -> Result<Vec<crate::accounting::JournalLine>, IncomeTaxOwnerError> {
    let mut lines = Vec::new();
    for (delta, debit, credit) in [
        (current_delta, TAX_EXP, CIT_PAYABLE),
        (deferred_delta, DTA, TAX_EXP),
    ] {
        if delta.is_positive() {
            lines.push(line(debit, PostingSide::Debit, delta));
            lines.push(line(credit, PostingSide::Credit, delta));
        } else if delta.is_negative() {
            let amount = delta.neg()?;
            lines.push(line(credit, PostingSide::Debit, amount));
            lines.push(line(debit, PostingSide::Credit, amount));
        }
    }
    Ok(lines)
}

pub(crate) fn preview_tax_cascade(
    books: &Books,
    position: &IncomeTaxPosition,
    policy: &IncomeTaxPolicy,
    next_event_id: u64,
    from_year: i32,
    posted_on: CivilDate,
    adjustments: &BTreeMap<BusinessEventId, AccountingPeriod>,
) -> Result<TaxCascadePreview, IncomeTaxOwnerError> {
    compute_tax_cascade(
        books,
        position,
        policy,
        next_event_id,
        from_year,
        posted_on,
        adjustments,
    )
}

fn line(code: &str, side: PostingSide, amount: AccountingAmount) -> JournalLine {
    JournalLine {
        account: LedgerAccountId(code.into()),
        side,
        amount,
    }
}

fn compute_tax_cascade(
    books: &Books,
    saved_position: &IncomeTaxPosition,
    policy: &IncomeTaxPolicy,
    event_cursor: u64,
    from_year: i32,
    posted_on: CivilDate,
    adjustments: &BTreeMap<BusinessEventId, AccountingPeriod>,
) -> Result<TaxCascadePreview, IncomeTaxOwnerError> {
    saved_position.validate(policy)?;
    saved_position.validate_books(books)?;
    if !(CIVIL_YEAR_MIN..=CIVIL_YEAR_MAX).contains(&from_year) || from_year > posted_on.year() {
        return Err(invalid("请求税务年度必须有效且不晚于过账年度"));
    }
    let mut position = saved_position.clone();
    for (&source, &target) in adjustments {
        if position
            .restatements
            .get(&source)
            .is_some_and(|previous| *previous != target)
        {
            return Err(invalid("同一凭证来源的历史有效期间不可变更"));
        }
        position.restatements.insert(source, target);
    }
    validate_restatements(&position.restatements, books)?;
    let first_year = position
        .assessments
        .keys()
        .next()
        .copied()
        .unwrap_or(from_year);
    if from_year < first_year {
        return Err(invalid("请求年度早于已保存的首个年初亏损基准"));
    }
    let latest_year = position
        .assessments
        .keys()
        .next_back()
        .copied()
        .unwrap_or(from_year)
        .max(from_year);
    let mut pool = match position.assessments.values().next() {
        Some(first) => first.opening_loss_pool.clone(),
        None => position.loss_pool.clone(),
    };
    if pool.iter().any(|entry| entry.origin_year >= first_year) {
        return Err(invalid("年初亏损池不得包含本年或未来年份的亏损"));
    }
    if position.assessments.is_empty() {
        position.initial_deferred_tax_asset = books
            .ledger()
            .account_net_debit(&LedgerAccountId(DTA.into()))?;
    }
    let mut old_previous_dta = position.initial_deferred_tax_asset;
    let mut new_previous_dta = position.initial_deferred_tax_asset;
    let pretax_by_year = year_pretax(books, &position.restatements)?;
    let mut current_tax_delta = AccountingAmount::ZERO;
    let mut deferred_delta = AccountingAmount::ZERO;
    let mut entries = Vec::new();
    let mut next_event_id = event_cursor;
    let mut requested_computation = None;
    for year in first_year..=latest_year {
        let pretax = pretax_by_year
            .get(&year)
            .copied()
            .unwrap_or(AccountingAmount::ZERO);
        let computation = policy.compute(pretax, year, &pool)?;
        let previous = position.assessments.get(&year);
        let old_tax = previous
            .map(|state| state.current_tax)
            .unwrap_or(AccountingAmount::ZERO);
        let old_dta = previous
            .map(|state| state.deferred_tax_asset)
            .unwrap_or(old_previous_dta);
        let tax_delta = computation.current_tax.sub(old_tax)?;
        let dta_movement = computation.deferred_tax_asset.sub(new_previous_dta)?;
        let old_dta_movement = old_dta.sub(old_previous_dta)?;
        let dta_delta = dta_movement.sub(old_dta_movement)?;
        current_tax_delta = current_tax_delta.add(tax_delta)?;
        deferred_delta = deferred_delta.add(dta_delta)?;
        let lines = tax_lines(tax_delta, dta_delta)?;
        if !lines.is_empty() {
            if year > posted_on.year() {
                return Err(invalid("后续年度税务差额不得过账到更早自然年度"));
            }
            let source = BusinessEventId::new(next_event_id);
            next_event_id = next_event_id
                .checked_add(1)
                .ok_or_else(|| invalid("税务事件身份空间耗尽"))?;
            if year != posted_on.year() {
                position
                    .restatements
                    .insert(source, AccountingPeriod::from_ymd(year, 12)?);
            }
            entries.push(JournalEntry {
                source,
                date: posted_on,
                kind: BusinessKind::TaxAccrual,
                cash_flow: CashFlowClass::NonCash,
                lines,
            });
        }
        position.assessments.insert(
            year,
            IncomeTaxAssessment {
                pretax,
                current_tax: computation.current_tax,
                opening_loss_pool: pool,
                deferred_tax_asset: computation.deferred_tax_asset,
            },
        );
        pool = computation.ending_pool.clone();
        old_previous_dta = old_dta;
        new_previous_dta = computation.deferred_tax_asset;
        if year == from_year {
            requested_computation = Some(computation);
        }
    }
    position.loss_pool = pool;
    position.validate(policy)?;
    Ok(TaxCascadePreview {
        position,
        computation: requested_computation.ok_or_else(|| invalid("请求年度没有产生评估"))?,
        entries,
        current_tax_delta,
        deferred_delta,
        next_event_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (Books, IncomeTaxPosition, IncomeTaxPolicy) {
        let books = Books::new(crate::company::industrial::industrial_account_chart());
        let position = IncomeTaxPosition::new(&books).unwrap();
        let policy = IncomeTaxPolicy {
            rate_bp: 2500,
            loss_carryforward_years: 5,
        };
        (books, position, policy)
    }

    fn date(year: i32) -> CivilDate {
        CivilDate::from_ymd(year, 12, 31).unwrap()
    }

    fn revenue(books: &mut Books, source: u64, year: i32, amount: i128) {
        let amount = AccountingAmount::from_cents(amount);
        books
            .post_batch(vec![JournalEntry {
                source: BusinessEventId::new(source),
                date: date(year),
                kind: BusinessKind::InterestAccrual,
                cash_flow: CashFlowClass::Operating,
                lines: vec![
                    line("1002", PostingSide::Debit, amount),
                    line("6001", PostingSide::Credit, amount),
                ],
            }])
            .unwrap();
    }

    fn install(books: &mut Books, position: &mut IncomeTaxPosition, preview: TaxCascadePreview) {
        books.post_batch(preview.entries).unwrap();
        *position = preview.position;
    }

    #[test]
    fn shared_tax_owner_rejects_refund_and_non_cash_payment_entries() {
        for offset in ["1002", "1122"] {
            let (mut books, position, _) = fixture();
            books
                .post_batch(vec![JournalEntry {
                    source: BusinessEventId::new(1),
                    date: date(2030),
                    kind: BusinessKind::TaxPayment,
                    cash_flow: CashFlowClass::Operating,
                    lines: vec![
                        line(
                            CIT_PAYABLE,
                            if offset == "1002" {
                                PostingSide::Credit
                            } else {
                                PostingSide::Debit
                            },
                            AccountingAmount::from_cents(100),
                        ),
                        line(
                            offset,
                            if offset == "1002" {
                                PostingSide::Debit
                            } else {
                                PostingSide::Credit
                            },
                            AccountingAmount::from_cents(100),
                        ),
                    ],
                }])
                .unwrap();
            assert!(position.validate_books(&books).is_err(), "{offset}");
        }
    }

    #[test]
    fn shared_tax_restore_binds_initial_deferred_asset_to_opening_journal() {
        let (mut books, mut position, policy) = fixture();
        books
            .post_batch(vec![
                JournalEntry {
                    source: BusinessEventId::new(1),
                    date: date(2030),
                    kind: BusinessKind::InterestAccrual,
                    cash_flow: CashFlowClass::NonCash,
                    lines: vec![
                        line(
                            "6603",
                            PostingSide::Debit,
                            AccountingAmount::from_cents(400),
                        ),
                        line(
                            "2231",
                            PostingSide::Credit,
                            AccountingAmount::from_cents(400),
                        ),
                    ],
                },
                JournalEntry {
                    source: BusinessEventId::new(2),
                    date: date(2030),
                    kind: BusinessKind::TaxAccrual,
                    cash_flow: CashFlowClass::NonCash,
                    lines: vec![
                        line(DTA, PostingSide::Debit, AccountingAmount::from_cents(100)),
                        line(
                            TAX_EXP,
                            PostingSide::Credit,
                            AccountingAmount::from_cents(100),
                        ),
                    ],
                },
            ])
            .unwrap();
        position.initial_deferred_tax_asset = AccountingAmount::from_cents(50);
        position.assessments.insert(
            2030,
            IncomeTaxAssessment {
                pretax: AccountingAmount::from_cents(-400),
                current_tax: AccountingAmount::ZERO,
                opening_loss_pool: Vec::new(),
                deferred_tax_asset: AccountingAmount::from_cents(100),
            },
        );
        position.loss_pool = vec![LossEntry {
            origin_year: 2030,
            remaining: AccountingAmount::from_cents(400),
        }];
        position.validate(&policy).unwrap();
        assert!(position.validate_books(&books).is_err());
    }

    #[test]
    fn shared_tax_owner_rejects_misclassified_or_cash_tax_accounts() {
        let chart = crate::company::industrial::industrial_account_chart();
        for (target, replacement) in [
            (
                DTA,
                crate::accounting::AccountDef::new("递延所得税资产", AccountElement::Liability),
            ),
            (
                CIT_PAYABLE,
                crate::accounting::AccountDef::new("应交所得税", AccountElement::Liability)
                    .with_cash(),
            ),
            (
                TAX_EXP,
                crate::accounting::AccountDef::new("所得税费用", AccountElement::Expense)
                    .with_contra(),
            ),
        ] {
            let accounts = chart
                .iter()
                .map(|(id, definition)| {
                    (
                        id.clone(),
                        if id.0 == target {
                            replacement.clone()
                        } else {
                            definition.clone()
                        },
                    )
                })
                .collect();
            let books = Books::new(
                crate::accounting::AccountChart::new(chart.version(), accounts).unwrap(),
            );
            assert!(IncomeTaxPosition::new(&books).is_err());
        }
    }

    #[test]
    fn shared_tax_owner_rejects_tax_facts_hidden_as_ordinary_business() {
        let (mut books, position, _) = fixture();
        books
            .post_batch(vec![JournalEntry {
                source: BusinessEventId::new(1),
                date: date(2030),
                kind: BusinessKind::InterestAccrual,
                cash_flow: CashFlowClass::NonCash,
                lines: vec![
                    line(
                        TAX_EXP,
                        PostingSide::Debit,
                        AccountingAmount::from_cents(100),
                    ),
                    line(
                        CIT_PAYABLE,
                        PostingSide::Credit,
                        AccountingAmount::from_cents(100),
                    ),
                ],
            }])
            .unwrap();
        assert!(position.validate_books(&books).is_err());
    }

    #[test]
    fn shared_tax_candidate_repeats_without_posting_or_consuming_identity() {
        let (mut books, mut position, policy) = fixture();
        revenue(&mut books, 1, 2030, 400);
        let first = preview_tax_cascade(
            &books,
            &position,
            &policy,
            2,
            2030,
            date(2030),
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(first.current_tax_delta.cents(), 100);
        assert_eq!(first.next_event_id, 3);
        install(&mut books, &mut position, first);
        let before = books.clone();
        let repeat = preview_tax_cascade(
            &books,
            &position,
            &policy,
            3,
            2030,
            date(2030),
            &BTreeMap::new(),
        )
        .unwrap();
        assert!(repeat.entries.is_empty());
        assert_eq!(repeat.current_tax_delta, AccountingAmount::ZERO);
        assert_eq!(repeat.next_event_id, 3);
        assert_eq!(books, before);
        repeat.position.validate(&policy).unwrap();
        repeat.position.validate_books(&books).unwrap();
    }

    #[test]
    fn shared_tax_restatement_recalculates_later_year_without_refunding_cash() {
        let (mut books, mut position, policy) = fixture();
        revenue(&mut books, 1, 2030, 400);
        revenue(&mut books, 2, 2031, 400);
        let first = preview_tax_cascade(
            &books,
            &position,
            &policy,
            3,
            2030,
            date(2030),
            &BTreeMap::new(),
        )
        .unwrap();
        install(&mut books, &mut position, first);
        let later = preview_tax_cascade(
            &books,
            &position,
            &policy,
            4,
            2031,
            date(2031),
            &BTreeMap::new(),
        )
        .unwrap();
        install(&mut books, &mut position, later);
        books
            .post_batch(vec![JournalEntry {
                source: BusinessEventId::new(5),
                date: date(2032),
                kind: BusinessKind::InterestAccrual,
                cash_flow: CashFlowClass::NonCash,
                lines: vec![
                    line(
                        "6603",
                        PostingSide::Debit,
                        AccountingAmount::from_cents(800),
                    ),
                    line(
                        "2231",
                        PostingSide::Credit,
                        AccountingAmount::from_cents(800),
                    ),
                ],
            }])
            .unwrap();
        let cash = books
            .ledger()
            .account_net_debit(&LedgerAccountId("1002".into()))
            .unwrap();
        let mapping = BTreeMap::from([(
            BusinessEventId::new(5),
            AccountingPeriod::from_ymd(2030, 12).unwrap(),
        )]);
        let cascade =
            preview_tax_cascade(&books, &position, &policy, 6, 2030, date(2032), &mapping).unwrap();
        assert_eq!(cascade.current_tax_delta.cents(), -200);
        assert_eq!(cascade.entries.len(), 2);
        assert!(cascade.entries.iter().all(|entry| entry.date == date(2032)));
        install(&mut books, &mut position, cascade);
        assert_eq!(
            books
                .ledger()
                .account_net_debit(&LedgerAccountId("1002".into()))
                .unwrap(),
            cash
        );
        position.validate(&policy).unwrap();
        position.validate_books(&books).unwrap();
    }

    #[test]
    fn shared_tax_candidate_rejects_unknown_sources_and_exhausted_identity_atomically() {
        let (mut books, position, policy) = fixture();
        revenue(&mut books, 1, 2030, 400);
        let before = books.clone();
        let invalid_map = BTreeMap::from([(
            BusinessEventId::new(99),
            AccountingPeriod::from_ymd(2029, 12).unwrap(),
        )]);
        assert!(preview_tax_cascade(
            &books,
            &position,
            &policy,
            2,
            2030,
            date(2030),
            &invalid_map
        )
        .is_err());
        assert!(preview_tax_cascade(
            &books,
            &position,
            &policy,
            u64::MAX,
            2030,
            date(2030),
            &BTreeMap::new()
        )
        .is_err());
        assert_eq!(books, before);
        assert!(position.assessments.is_empty());
    }
}
