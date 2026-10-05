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
