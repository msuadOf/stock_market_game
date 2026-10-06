use super::*;
impl SimpleFinanceState {
    pub fn validate(&self) -> Result<(), SimpleFinanceError> {
        self.config.tax_policy.validate()?;
        if self.books.ledger().chart() != &super::kind::summary_chart(self.kind)? {
            return Err(SimpleFinanceError::Invalid(
                "Simple CompanyKind 与汇总科目表不一致".into(),
            ));
        }
        if self.company.0.trim().is_empty()
            || self.opening_date > self.as_of
            || self.last_month != AccountingPeriod::of_date(self.as_of)
            || self.as_of.next()?.month() == self.as_of.month()
        {
            return Err(SimpleFinanceError::Invalid(
                "摘要公司身份或期末日期不一致".into(),
            ));
        }
        let opening = self
            .books
            .journal()
            .entries()
            .next()
            .ok_or_else(|| SimpleFinanceError::Invalid("缺少汇总期初凭证".into()))?;
        if opening.source != BusinessEventId::new(1)
            || opening.date != self.opening_date
            || opening.kind != BusinessKind::OpeningBalance
            || opening.lines != self.config.opening_lines
        {
            return Err(SimpleFinanceError::Invalid(
                "汇总账簿与显式期初配置不一致".into(),
            ));
        }
        for item in &opening.lines {
            if self
                .books
                .ledger()
                .chart()
                .get(&item.account)
                .is_some_and(|definition| {
                    matches!(
                        definition.element,
                        crate::accounting::AccountElement::Revenue
                            | crate::accounting::AccountElement::Expense
                    )
                })
            {
                return Err(SimpleFinanceError::Invalid(
                    "Simple 期初不得隐藏收入或费用".into(),
                ));
            }
        }
        let max_source = self
            .books
            .journal()
            .entries()
            .map(|entry| entry.source.value())
            .max()
            .ok_or_else(|| SimpleFinanceError::Invalid("缺少凭证来源".into()))?;
        if self.next_event_id <= max_source {
            return Err(SimpleFinanceError::Invalid(
                "下一个摘要事件来源不能重用既有来源".into(),
            ));
        }
        let mut unpaid_dividends = AccountingAmount::ZERO;
        let mut reserved_amount = AccountingAmount::ZERO;
        let mut dividend_sources = std::collections::BTreeSet::new();
        if self.legal_facts.0.as_ref().is_some_and(|facts| {
            !facts.registered_capital.is_positive() || facts.source_evidence.trim().is_empty()
        }) {
            return Err(SimpleFinanceError::DividendInvalid(
                "已绑定的分红法律事实金额或来源说明非法".into(),
            ));
        }
        for (plan_id, plan) in &self.dividends {
            // 送转入账会按面值演进注册资本法定事实；分红声明冻结的是其批准时点的
            // 注册资本。按「当前法定注册资本 − 批准日当天及之后才入账的送转股本
            // 增加」重构批准时点口径：批准发生在日内、送转入账发生在日终，因此
            // 批准日当天的入账也尚未反映在声明口径中。
            let mut later_stock_credits = AccountingAmount::ZERO;
            for fact in self
                .stock_distributions
                .values()
                .filter(|fact| {
                    fact.credited_on
                        .is_some_and(|credited| credited >= plan.declaration.approved_on)
                })
            {
                later_stock_credits = later_stock_credits.add(fact.capital_increase)?;
            }
            let capital_at_approval = self
                .legal_facts
                .0
                .as_ref()
                .ok_or_else(|| {
                    SimpleFinanceError::DividendInvalid(
                        "存在分红计划但缺少注册资本法定事实".into(),
                    )
                })?
                .registered_capital
                .sub(later_stock_credits)?;
            if plan_id.trim().is_empty()
                || plan.declaration.plan_id != *plan_id
                || !plan.declaration.total_gross.is_positive()
                || plan.declaration.registered_capital.is_negative()
                || plan.declaration.registered_capital.is_zero()
                || capital_at_approval != plan.declaration.registered_capital
                || plan.declaration.approved_on <= self.opening_date
                || plan.reserve.is_negative()
            {
                return Err(SimpleFinanceError::DividendInvalid(format!(
                    "分红计划 {plan_id} 状态或注册资本事实不一致"
                )));
            }
            let declaration_source = self
                .books
                .journal()
                .entries()
                .find(|entry| entry.source == plan.declaration_source)
                .ok_or_else(|| {
                    SimpleFinanceError::DividendInvalid(format!(
                        "分红计划 {plan_id} 缺少绑定的批准分录"
                    ))
                })?;
            if !dividend_sources.insert(plan.declaration_source) {
                return Err(SimpleFinanceError::DividendInvalid(
                    "多个分红计划共用批准分录来源".into(),
                ));
            }
            let retained_debit = plan.declaration.total_gross.add(plan.reserve)?;
            let mut expected_declaration_lines = vec![JournalLine {
                account: LedgerAccountId("4103".into()),
                side: PostingSide::Debit,
                amount: retained_debit,
            }];
            if plan.reserve.is_positive() {
                expected_declaration_lines.push(JournalLine {
                    account: LedgerAccountId(
                        crate::accounting::reports::simple_summary::STATUTORY_RESERVE.into(),
                    ),
                    side: PostingSide::Credit,
                    amount: plan.reserve,
                });
            }
            expected_declaration_lines.push(JournalLine {
                account: LedgerAccountId(
                    crate::accounting::reports::simple_summary::DIVIDEND_PAYABLE.into(),
                ),
                side: PostingSide::Credit,
                amount: plan.declaration.total_gross,
            });
            if declaration_source.kind != BusinessKind::CompanyDividendDeclaration
                || declaration_source.date != plan.declaration.approved_on
                || declaration_source.cash_flow != CashFlowClass::NonCash
                || declaration_source.lines != expected_declaration_lines
            {
                return Err(SimpleFinanceError::DividendInvalid(format!(
                    "分红计划 {plan_id} 与批准分录不一致"
                )));
            }
            if plan.reserve.is_positive() != plan.reserve_basis_year.0.is_some() {
                return Err(SimpleFinanceError::DividendInvalid(format!(
                    "分红计划 {plan_id} 的公积金计提年度缺失或多余"
                )));
            }
            if let Some(year) = plan.reserve_basis_year.0 {
                if year > self.as_of.year() || year < self.opening_date.year() {
                    return Err(SimpleFinanceError::DividendInvalid(format!(
                        "分红计划 {plan_id} 的公积金年度越界"
                    )));
                }
            }
            let paid = plan.payments.iter().try_fold(AccountingAmount::ZERO, |sum, (payment_id, payment)| {
                if payment_id.trim().is_empty() || !payment.amount.is_positive()
                    || payment.paid_on < plan.declaration.approved_on
                {
                    return Err(SimpleFinanceError::DividendInvalid(format!(
                        "分红付款批次 {payment_id} 状态非法"
                    )));
                }
                let payment_source = self.books.journal().entries()
                    .find(|entry| entry.source == payment.source)
                    .ok_or_else(|| SimpleFinanceError::DividendInvalid(format!(
                        "付款批次 {payment_id} 缺少绑定的账簿分录"
                    )))?;
                if !dividend_sources.insert(payment.source) {
                    return Err(SimpleFinanceError::DividendInvalid(
                        "多个分红计划或付款批次共用账簿来源".into(),
                    ));
                }
                if payment_source.kind != BusinessKind::CompanyDividendPayment
                    || payment_source.date != payment.paid_on
                    || payment_source.cash_flow != CashFlowClass::NonCash
                    || payment_source.lines != vec![
                        JournalLine {
                            account: LedgerAccountId(crate::accounting::reports::simple_summary::DIVIDEND_PAYABLE.into()),
                            side: PostingSide::Debit, amount: payment.amount,
                        },
                        JournalLine {
                            account: LedgerAccountId(crate::accounting::reports::simple_summary::DIVIDEND_SETTLEMENT_ASSET.into()),
                            side: PostingSide::Credit, amount: payment.amount,
                        },
                    ]
                {
                    return Err(SimpleFinanceError::DividendInvalid(format!(
                        "付款批次 {payment_id} 与账簿分录不一致"
                    )));
                }
                sum.add(payment.amount).map_err(SimpleFinanceError::from)
            })?;
            if paid > plan.declaration.total_gross {
                return Err(SimpleFinanceError::DividendInvalid(format!(
                    "分红计划 {plan_id} 实付超过批准总额"
                )));
            }
            unpaid_dividends = unpaid_dividends.add(plan.declaration.total_gross.sub(paid)?)?;
            reserved_amount = reserved_amount.add(plan.reserve)?;
        }
        // 送转与配股声明的 registered_capital_at_approval 冻结各自批准时点口径，
        // 按「当前法定注册资本 − 批准日当天及之后才入账的送转/配股股本增加」重构
        // 核对；批准在日内、入账在日终，批准日当天的入账尚未反映在声明口径中。
        let capital_increase_after = |approved_on: crate::calendar::CivilDate| {
            let mut total = AccountingAmount::ZERO;
            for other in self
                .stock_distributions
                .values()
                .filter(|other| {
                    other
                        .credited_on
                        .is_some_and(|credited| credited >= approved_on)
                })
            {
                total = total.add(other.capital_increase)?;
            }
            for other in self
                .rights_offerings
                .values()
                .filter(|other| {
                    other
                        .settled_on
                        .is_some_and(|settled| settled >= approved_on)
                })
            {
                total = total.add(other.capital_increase.unwrap_or(AccountingAmount::ZERO))?;
            }
            // 回购注销核减注册资本：从重构口径中扣除批准日之后的核减额。
            for other in self
                .issuer_repurchases
                .values()
                .filter(|other| {
                    other
                        .cancelled_on
                        .is_some_and(|cancelled| cancelled >= approved_on)
                })
            {
                total = total.sub(other.capital_reduction.unwrap_or(AccountingAmount::ZERO))?;
            }
            Ok::<_, SimpleFinanceError>(total)
        };
        for (event_id, fact) in &self.stock_distributions {
            let later_credits = capital_increase_after(fact.approved_on)?;
            let expected = self
                .legal_facts
                .0
                .as_ref()
                .ok_or_else(|| {
                    SimpleFinanceError::StockDistributionInvalid(
                        "存在送转声明但缺少注册资本法定事实".into(),
                    )
                })?
                .registered_capital
                .sub(later_credits)?;
            if fact.registered_capital_at_approval != expected {
                return Err(SimpleFinanceError::StockDistributionInvalid(format!(
                    "送转声明 {event_id} 的批准时点注册资本与法定事实演进历史不一致"
                )));
            }
        }
        for (event_id, fact) in &self.rights_offerings {
            let later_credits = capital_increase_after(fact.approved_on)?;
            let expected = self
                .legal_facts
                .0
                .as_ref()
                .ok_or_else(|| {
                    SimpleFinanceError::StockDistributionInvalid(
                        "存在配股声明但缺少注册资本法定事实".into(),
                    )
                })?
                .registered_capital
                .sub(later_credits)?;
            if fact.registered_capital_at_approval != expected {
                return Err(SimpleFinanceError::StockDistributionInvalid(format!(
                    "配股声明 {event_id} 的批准时点注册资本与法定事实演进历史不一致"
                )));
            }
            if let (Some(settled_on), Some(proceeds), Some(capital_increase)) =
                (fact.settled_on, fact.proceeds, fact.capital_increase)
            {
                let expected_proceeds = i128::from(fact.price_per_share.cents())
                    .checked_mul(i128::from(fact.issued_shares))
                    .ok_or_else(|| {
                        SimpleFinanceError::StockDistributionInvalid(
                            "配股募集资金重构金额溢出".into(),
                        )
                    })?;
                if proceeds != AccountingAmount::from_cents(expected_proceeds) {
                    return Err(SimpleFinanceError::StockDistributionInvalid(format!(
                        "配股声明 {event_id} 的募集资金与发行价×认购股数不一致"
                    )));
                }
                let expected_capital = i128::from(fact.par_value_per_share.cents())
                    .checked_mul(i128::from(fact.issued_shares))
                    .ok_or_else(|| {
                        SimpleFinanceError::StockDistributionInvalid(
                            "配股股本增加重构金额溢出".into(),
                        )
                    })?;
                if capital_increase != AccountingAmount::from_cents(expected_capital) {
                    return Err(SimpleFinanceError::StockDistributionInvalid(format!(
                        "配股声明 {event_id} 的股本增加与面值×认购股数不一致"
                    )));
                }
                if settled_on < fact.approved_on
                    || (fact.issued_shares == 0
                        && proceeds != AccountingAmount::from_cents(0))
                {
                    return Err(SimpleFinanceError::StockDistributionInvalid(format!(
                        "配股声明 {event_id} 的结算回填日期、股数或募集资金非法"
                    )));
                }
            } else if fact.settled_on.is_some()
                || fact.proceeds.is_some()
                || fact.capital_increase.is_some()
            {
                return Err(SimpleFinanceError::StockDistributionInvalid(format!(
                    "配股声明 {event_id} 的结算回填字段不完整"
                )));
            }
        }
        for (event_id, fact) in &self.issuer_repurchases {
            if fact.event_id.trim().is_empty()
                || fact.approval_reference.trim().is_empty()
                || !fact.synthetic_funding.is_positive()
            {
                return Err(SimpleFinanceError::StockDistributionInvalid(format!(
                    "回购声明 {event_id} 的身份或额度非法"
                )));
            }
            match (fact.completed_on, fact.spent, fact.withdrawn_remainder) {
                (None, None, None) => {}
                (Some(completed_on), Some(spent), Some(withdrawn)) => {
                    if completed_on < fact.approved_on
                        || spent.add(withdrawn)? != fact.synthetic_funding
                        || spent.is_negative()
                        || withdrawn.is_negative()
                    {
                        return Err(SimpleFinanceError::StockDistributionInvalid(format!(
                            "回购声明 {event_id} 的完成回填与获批额度不一致"
                        )));
                    }
                }
                _ => {
                    return Err(SimpleFinanceError::StockDistributionInvalid(format!(
                        "回购声明 {event_id} 的完成回填字段不完整"
                    )));
                }
            }
            if fact.cancelled_shares > 0 || fact.cancelled_on.is_some() {
                if fact.completed_on.is_none()
                    || fact.cancelled_on.is_none()
                    || fact.capital_reduction.is_none()
                    || fact.cancelled_shares == 0
                {
                    return Err(SimpleFinanceError::StockDistributionInvalid(format!(
                        "回购声明 {event_id} 的注销回填字段不完整"
                    )));
                }
            }
        }
        let payable = self
            .books
            .ledger()
            .account_net_debit(&LedgerAccountId(
                crate::accounting::reports::simple_summary::DIVIDEND_PAYABLE.into(),
            ))?
            .neg()?;
        if payable != unpaid_dividends {
            return Err(SimpleFinanceError::DividendInvalid(
                "应付股利科目与未支付计划不一致".into(),
            ));
        }
        let reserve_account = self
            .books
            .ledger()
            .account_net_debit(&LedgerAccountId(
                crate::accounting::reports::simple_summary::STATUTORY_RESERVE.into(),
            ))?
            .neg()?;
        if reserve_account < reserved_amount {
            return Err(SimpleFinanceError::DividendInvalid(
                "法定公积金科目低于已批准计划提取额".into(),
            ));
        }
        self.books.ledger().trial_balance()?;
        self.income_tax_position
            .validate(&self.config.tax_policy.income_tax)?;
        self.income_tax_position.validate_books(&self.books)?;
        if self
            .income_tax_position
            .latest_assessment_year()
            .is_some_and(|year| year > self.as_of.year())
            || self
                .income_tax_position
                .loss_pool()
                .iter()
                .any(|entry| entry.origin_year > self.as_of.year())
        {
            return Err(SimpleFinanceError::Invalid(
                "税务评估及亏损池不得来自未来年度".into(),
            ));
        }
        let scope = ScopeId::Standalone(self.member_id());
        if self.income_tax_position.restatements()
            != &self
                .closing
                .restatement_periods(&scope)
                .cloned()
                .unwrap_or_default()
        {
            return Err(SimpleFinanceError::Invalid(
                "税务与报告更正工作底稿不同步".into(),
            ));
        }
        let mut expected = self.opening_date.next()?;
        for &(start, end) in &self.recognized_periods {
            if start != expected
                || start.day() != 1
                || end < start
                || end > self.as_of
                || end.next()?.month() == end.month()
                || start.year() != end.year()
            {
                return Err(SimpleFinanceError::Invalid(
                    "已生成财务期间出现缺口或重叠".into(),
                ));
            }
            expected = end.next()?;
        }
        if expected != self.as_of.next()? {
            return Err(SimpleFinanceError::Invalid(
                "最后生成期间与财务截至日不一致".into(),
            ));
        }
        self.validate_reports()?;
        Ok(())
    }
}
