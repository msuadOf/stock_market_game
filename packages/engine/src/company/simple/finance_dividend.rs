use super::*;
use std::collections::BTreeMap;

const RETAINED_EARNINGS: &str = "4103";

fn line(account: &str, side: PostingSide, amount: AccountingAmount) -> JournalLine {
    JournalLine {
        account: LedgerAccountId(account.into()),
        side,
        amount,
    }
}

impl SimpleFinanceState {
    pub fn dividend_plan_facts(&self) -> Result<Vec<DividendPlanFact>, SimpleFinanceError> {
        let source_evidence = match (&self.legal_facts.0, self.dividends.is_empty()) {
            (Some(facts), _) => facts.source_evidence.clone(),
            (None, true) => String::new(),
            (None, false) => {
                return Err(SimpleFinanceError::DividendInvalid(
                    "存在分红计划但缺少注册资本法定事实".into(),
                ));
            }
        };
        Ok(self
            .dividends
            .iter()
            .map(|(plan_id, plan)| DividendPlanFact {
                plan_id: plan_id.clone(),
                approved_on: plan.declaration.approved_on,
                total_gross: plan.declaration.total_gross,
                registered_capital: plan.declaration.registered_capital,
                registered_capital_source_evidence: source_evidence.clone(),
                declaration_source: plan.declaration_source,
                payments: plan
                    .payments
                    .iter()
                    .map(|(payment_id, payment)| DividendPaymentFact {
                        payment_id: payment_id.clone(),
                        paid_on: payment.paid_on,
                        amount: payment.amount,
                        source: payment.source,
                    })
                    .collect(),
            })
            .collect())
    }

    pub fn dividend_payment_facts(&self, plan_id: &str) -> Option<Vec<DividendPaymentFact>> {
        self.dividends.get(plan_id).map(|plan| {
            plan.payments
                .iter()
                .map(|(payment_id, payment)| DividendPaymentFact {
                    payment_id: payment_id.clone(),
                    paid_on: payment.paid_on,
                    amount: payment.amount,
                    source: payment.source,
                })
                .collect()
        })
    }

    pub fn distributable_profit(&self) -> Result<DistributableProfit, SimpleFinanceError> {
        let registered_capital = self
            .legal_facts
            .0
            .as_ref()
            .ok_or_else(|| {
                SimpleFinanceError::DividendUnsupported(
                    "缺少显式绑定的公司法定注册资本来源事实".into(),
                )
            })?
            .registered_capital;
        if registered_capital.is_negative() || registered_capital.is_zero() {
            return Err(SimpleFinanceError::DividendInvalid(
                "注册资本必须是正数法定事实".into(),
            ));
        }
        // 未入账送转已经批准并预留来源；免计提门槛按其全部入账后的注册资本
        // 保守核定，避免先后批准后由自身增资重新触发计提，导致后续入账卡死。
        let registered_capital = self
            .stock_distributions
            .values()
            .filter(|fact| fact.credited_on.is_none())
            .try_fold(registered_capital, |capital, fact| {
                capital.add(fact.capital_increase)
            })?;
        let retained_debit = self
            .books
            .ledger()
            .account_net_debit(&LedgerAccountId(RETAINED_EARNINGS.into()))?;
        let after_loss = self.books.ledger().net_income()?.sub(retained_debit)?;
        let reserve_balance = self
            .books
            .ledger()
            .account_net_debit(&LedgerAccountId(
                crate::accounting::reports::simple_summary::STATUTORY_RESERVE.into(),
            ))?
            .neg()?;
        let (reserve_required, reserve_basis_year) = if !after_loss.is_positive()
            || reserve_balance >= registered_capital.apply_basis_points(5_000)?
        {
            (AccountingAmount::ZERO, None)
        } else {
            let year = latest_complete_fiscal_year(self)?;
            let Some(year) = year else {
                return Err(SimpleFinanceError::DividendUnsupported(
                    "尚无完整自然年度税后利润账簿事实，不能核定法定公积金".into(),
                ));
            };
            let annual_profit = annual_after_tax_profit(&self.books, year)?;
            let prior_retained = cumulative_profit_before(&self.books, year)?
                .sub(retained_debit_before(&self.books, year)?)?;
            let after_prior_losses = annual_profit.add(if prior_retained.is_negative() {
                prior_retained
            } else {
                AccountingAmount::ZERO
            })?;
            let target = if after_prior_losses.is_positive() {
                after_prior_losses.apply_basis_points(1_000)?
            } else {
                AccountingAmount::ZERO
            };
            let already_reserved_dividends = self
                .dividends
                .values()
                .filter(|plan| plan.reserve_basis_year.0 == Some(year))
                .try_fold(AccountingAmount::ZERO, |sum, plan| sum.add(plan.reserve))?;
            let already_reserved_stock = self
                .stock_distributions
                .values()
                .filter(|fact| fact.reserve_basis_year == Some(year))
                .try_fold(AccountingAmount::ZERO, |sum, fact| sum.add(fact.statutory_reserve))?;
            let already_reserved = already_reserved_dividends.add(already_reserved_stock)?;
            let remaining = target.sub(already_reserved)?;
            if remaining.is_positive() {
                (remaining, Some(year))
            } else {
                (AccountingAmount::ZERO, None)
            }
        };
        let uncredited_bonus_reserved = self
            .stock_distributions
            .values()
            .filter(|fact| {
                fact.credited_on.is_none()
                    && matches!(
                        fact.kind,
                        crate::company::stock_distribution::StockDistributionKind::BonusShares
                    )
            })
            .try_fold(AccountingAmount::ZERO, |sum, fact| {
                sum.add(fact.capital_increase)
            })?;
        let available = after_loss
            .sub(reserve_required)?
            .sub(uncredited_bonus_reserved)?;
        Ok(DistributableProfit {
            accumulated_after_loss: after_loss,
            statutory_reserve: reserve_required,
            available_for_distribution: if available.is_positive() {
                available
            } else {
                AccountingAmount::ZERO
            },
            reserve_basis_year,
        })
    }

    pub fn declare_dividend(
        &mut self,
        declaration: DividendDeclaration,
    ) -> Result<DividendPlanReceipt, SimpleFinanceError> {
        if let Some(existing) = self.dividends.get(&declaration.plan_id) {
            if existing.declaration == declaration {
                return Ok(DividendPlanReceipt {
                    plan_id: declaration.plan_id,
                    amount: existing.declaration.total_gross,
                    statutory_reserve: existing.reserve,
                    already_declared: true,
                });
            }
            return Err(SimpleFinanceError::DividendPlanConflict(
                declaration.plan_id,
            ));
        }
        if declaration.plan_id.trim().is_empty() || !declaration.total_gross.is_positive() {
            return Err(SimpleFinanceError::DividendInvalid(
                "计划标识及正数分红总额必须明确".into(),
            ));
        }
        if declaration.approved_on <= self.as_of {
            return Err(SimpleFinanceError::DividendInvalid(
                "批准日期必须晚于最后结算日，避免向已封账期间回写".into(),
            ));
        }
        let legal_facts = self.legal_facts.0.as_ref().ok_or_else(|| {
            SimpleFinanceError::DividendUnsupported("缺少显式绑定的公司法定注册资本来源事实".into())
        })?;
        if declaration.registered_capital != legal_facts.registered_capital {
            return Err(SimpleFinanceError::DividendLegalFactsConflict(
                "计划注册资本与公司已绑定法律事实不一致".into(),
            ));
        }
        let profit = self.distributable_profit()?;
        if declaration.total_gross > profit.available_for_distribution {
            return Err(SimpleFinanceError::DividendInvalid(format!(
                "分红总额 {} 超过已弥补亏损及提取法定公积后的可分配利润 {}",
                declaration.total_gross.to_yuan_string(),
                profit.available_for_distribution.to_yuan_string()
            )));
        }
        let mut candidate = self.clone();
        let mut lines = Vec::new();
        let retained = declaration.total_gross.add(profit.statutory_reserve)?;
        if retained.is_positive() {
            lines.push(line(RETAINED_EARNINGS, PostingSide::Debit, retained));
        }
        if profit.statutory_reserve.is_positive() {
            lines.push(line(
                crate::accounting::reports::simple_summary::STATUTORY_RESERVE,
                PostingSide::Credit,
                profit.statutory_reserve,
            ));
        }
        lines.push(line(
            crate::accounting::reports::simple_summary::DIVIDEND_PAYABLE,
            PostingSide::Credit,
            declaration.total_gross,
        ));
        let next = candidate
            .next_event_id
            .checked_add(1)
            .ok_or_else(|| SimpleFinanceError::DividendInvalid("财务事件序号耗尽".into()))?;
        let declaration_source = BusinessEventId::new(candidate.next_event_id);
        candidate.books.post_batch(vec![JournalEntry {
            source: declaration_source,
            date: declaration.approved_on,
            kind: BusinessKind::CompanyDividendDeclaration,
            cash_flow: CashFlowClass::NonCash,
            lines,
        }])?;
        candidate.next_event_id = next;
        candidate.dividends.insert(
            declaration.plan_id.clone(),
            DividendPlanState {
                declaration: declaration.clone(),
                declaration_source,
                reserve: profit.statutory_reserve,
                reserve_basis_year: RequiredOption(profit.reserve_basis_year),
                payments: BTreeMap::new(),
            },
        );
        candidate.validate()?;
        *self = candidate;
        Ok(DividendPlanReceipt {
            plan_id: declaration.plan_id,
            amount: declaration.total_gross,
            statutory_reserve: profit.statutory_reserve,
            already_declared: false,
        })
    }

    pub fn define_dividend_legal_facts(
        &mut self,
        facts: DividendLegalFacts,
    ) -> Result<(), SimpleFinanceError> {
        if !facts.registered_capital.is_positive() || facts.source_evidence.trim().is_empty() {
            return Err(SimpleFinanceError::DividendInvalid(
                "注册资本须为正数，且必须记录明确的事实来源".into(),
            ));
        }
        match &self.legal_facts.0 {
            Some(existing) if existing == &facts => Ok(()),
            Some(_) => Err(SimpleFinanceError::DividendLegalFactsConflict(
                "每家公司仅可绑定一次注册资本来源事实".into(),
            )),
            None => {
                self.legal_facts = RequiredOption(Some(facts));
                Ok(())
            }
        }
    }

    pub fn pay_dividend(
        &mut self,
        plan_id: &str,
        payment_id: &str,
        paid_on: CivilDate,
        amount: AccountingAmount,
    ) -> Result<DividendPaymentReceipt, SimpleFinanceError> {
        let plan = self.dividends.get(plan_id).ok_or_else(|| {
            SimpleFinanceError::DividendInvalid(format!("未批准分红计划 {plan_id}"))
        })?;
        if let Some(prior) = plan.payments.get(payment_id) {
            if prior.paid_on == paid_on && prior.amount == amount {
                return Ok(DividendPaymentReceipt {
                    plan_id: plan_id.into(),
                    payment_id: payment_id.into(),
                    amount,
                    paid_on,
                    already_paid: true,
                    simple_display_only: true,
                    within_six_month_deadline: within_dividend_deadline(
                        plan.declaration.approved_on,
                        paid_on,
                    )?,
                });
            }
            return Err(SimpleFinanceError::DividendPlanConflict(payment_id.into()));
        }
        if payment_id.trim().is_empty() || !amount.is_positive() {
            return Err(SimpleFinanceError::DividendInvalid(
                "付款批次标识及正数实付金额必须明确".into(),
            ));
        }
        if paid_on < plan.declaration.approved_on {
            return Err(SimpleFinanceError::DividendInvalid(
                "支付日期早于分配决议批准日期".into(),
            ));
        }
        let already_paid = plan
            .payments
            .values()
            .try_fold(AccountingAmount::ZERO, |sum, payment| {
                sum.add(payment.amount)
            })?;
        if already_paid.add(amount)? > plan.declaration.total_gross {
            return Err(SimpleFinanceError::DividendInvalid(
                "分红付款批次总额超过已批准分红总额".into(),
            ));
        }
        let within_six_month_deadline =
            within_dividend_deadline(plan.declaration.approved_on, paid_on)?;
        let mut candidate = self.clone();
        let next = candidate
            .next_event_id
            .checked_add(1)
            .ok_or_else(|| SimpleFinanceError::DividendInvalid("财务事件序号耗尽".into()))?;
        let source = BusinessEventId::new(candidate.next_event_id);
        candidate.books.post_batch(vec![JournalEntry {
            source,
            date: paid_on,
            kind: BusinessKind::CompanyDividendPayment,
            cash_flow: CashFlowClass::NonCash,
            lines: vec![
                line(
                    crate::accounting::reports::simple_summary::DIVIDEND_PAYABLE,
                    PostingSide::Debit,
                    amount,
                ),
                line(
                    crate::accounting::reports::simple_summary::DIVIDEND_SETTLEMENT_ASSET,
                    PostingSide::Credit,
                    amount,
                ),
            ],
        }])?;
        candidate.next_event_id = next;
        candidate
            .dividends
            .get_mut(plan_id)
            .expect("plan cloned from same state")
            .payments
            .insert(
                payment_id.into(),
                DividendPaymentState {
                    source,
                    paid_on,
                    amount,
                },
            );
        // 已付分红增加 → 账面展示现金按公式减少（仅展示值，不动真实资金）。
        candidate.refresh_book_display()?;
        candidate.validate()?;
        *self = candidate;
        Ok(DividendPaymentReceipt {
            plan_id: plan_id.into(),
            payment_id: payment_id.into(),
            amount,
            paid_on,
            already_paid: false,
            simple_display_only: true,
            within_six_month_deadline,
        })
    }
}

fn within_dividend_deadline(
    approved_on: CivilDate,
    paid_on: CivilDate,
) -> Result<bool, SimpleFinanceError> {
    let month_index = approved_on.year() * 12 + i32::from(approved_on.month()) - 1 + 6;
    let year = month_index.div_euclid(12);
    let month = (month_index.rem_euclid(12) + 1) as u8;
    let last_day = if month == 12 {
        CivilDate::from_ymd(year + 1, 1, 1)?.prev()?.day()
    } else {
        CivilDate::from_ymd(year, month + 1, 1)?.prev()?.day()
    };
    let deadline = CivilDate::from_ymd(year, month, approved_on.day().min(last_day))?;
    Ok(paid_on <= deadline)
}

fn annual_after_tax_profit(
    books: &Books,
    year: i32,
) -> Result<AccountingAmount, SimpleFinanceError> {
    let mut result = AccountingAmount::ZERO;
    for entry in books
        .journal()
        .entries()
        .filter(|entry| entry.date.year() == year)
    {
        for row in &entry.lines {
            let definition = books.ledger().chart().get(&row.account).ok_or_else(|| {
                SimpleFinanceError::DividendInvalid(format!("账簿科目 {} 缺失", row.account))
            })?;
            let amount = match definition.element {
                crate::accounting::AccountElement::Revenue => match row.side {
                    PostingSide::Credit => row.amount,
                    PostingSide::Debit => row.amount.neg()?,
                },
                crate::accounting::AccountElement::Expense => match row.side {
                    PostingSide::Debit => row.amount.neg()?,
                    PostingSide::Credit => row.amount,
                },
                _ => continue,
            };
            result = result.add(amount)?;
        }
    }
    Ok(result)
}

fn cumulative_profit_before(
    books: &Books,
    year: i32,
) -> Result<AccountingAmount, SimpleFinanceError> {
    let mut result = AccountingAmount::ZERO;
    for entry in books
        .journal()
        .entries()
        .filter(|entry| entry.date.year() < year)
    {
        for row in &entry.lines {
            let definition = books.ledger().chart().get(&row.account).ok_or_else(|| {
                SimpleFinanceError::DividendInvalid(format!("账簿科目 {} 缺失", row.account))
            })?;
            let amount = match definition.element {
                crate::accounting::AccountElement::Revenue => match row.side {
                    PostingSide::Credit => row.amount,
                    PostingSide::Debit => row.amount.neg()?,
                },
                crate::accounting::AccountElement::Expense => match row.side {
                    PostingSide::Debit => row.amount.neg()?,
                    PostingSide::Credit => row.amount,
                },
                _ => continue,
            };
            result = result.add(amount)?;
        }
    }
    Ok(result)
}

fn retained_debit_before(books: &Books, year: i32) -> Result<AccountingAmount, SimpleFinanceError> {
    let mut result = AccountingAmount::ZERO;
    let account = LedgerAccountId(RETAINED_EARNINGS.into());
    for entry in books
        .journal()
        .entries()
        .filter(|entry| entry.kind == BusinessKind::OpeningBalance && entry.date.year() < year)
    {
        for row in entry.lines.iter().filter(|row| row.account == account) {
            result = match row.side {
                PostingSide::Debit => result.add(row.amount)?,
                PostingSide::Credit => result.sub(row.amount)?,
            };
        }
    }
    Ok(result)
}

fn latest_complete_fiscal_year(
    state: &SimpleFinanceState,
) -> Result<Option<i32>, SimpleFinanceError> {
    let latest_candidate = if state.as_of.month() == 12 && state.as_of.day() == 31 {
        state.as_of.year()
    } else {
        state.as_of.year() - 1
    };
    for year in (state.opening_date.year()..=latest_candidate).rev() {
        let first = CivilDate::from_ymd(year, 1, 1)?;
        let last = CivilDate::from_ymd(year, 12, 31)?;
        if state
            .recognized_periods
            .iter()
            .any(|(start, _)| *start <= first)
            && state.recognized_periods.iter().any(|(_, end)| *end >= last)
        {
            return Ok(Some(year));
        }
    }
    Ok(None)
}
