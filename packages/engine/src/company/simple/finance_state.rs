use super::*;
impl SimpleFinanceState {
    pub fn create(
        company: CompanyId,
        kind: CompanyKind,
        config: &SimpleFinanceConfig,
        as_of: CivilDate,
    ) -> Result<Self, SimpleFinanceError> {
        config.tax_policy.validate()?;
        if company.0.trim().is_empty() || as_of.next()?.month() == as_of.month() {
            return Err(SimpleFinanceError::Invalid(
                "公司身份与开账月末日期必须明确".into(),
            ));
        }
        let mut books = Books::new(super::kind::summary_chart(kind)?);
        for item in &config.opening_lines {
            if books
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
                    "Simple 期初不得直接填入收入或费用".into(),
                ));
            }
        }
        books.post_batch(vec![JournalEntry {
            source: BusinessEventId::new(1),
            date: as_of,
            kind: BusinessKind::OpeningBalance,
            cash_flow: CashFlowClass::Financing,
            lines: config.opening_lines.clone(),
        }])?;
        let income_tax_position = IncomeTaxPosition::new(&books)?;
        let state = Self {
            company,
            kind,
            config: config.clone(),
            books,
            closing: ClosingEngine::new(),
            opening_date: as_of,
            as_of,
            last_month: AccountingPeriod::of_date(as_of),
            next_event_id: 2,
            income_tax_position,
            recognized_periods: Vec::new(),
        };
        state.validate()?;
        Ok(state)
    }
    pub fn books(&self) -> &Books {
        &self.books
    }
    pub fn company_id(&self) -> &CompanyId {
        &self.company
    }
    pub fn kind(&self) -> CompanyKind {
        self.kind
    }
    pub fn config(&self) -> &SimpleFinanceConfig {
        &self.config
    }
    pub fn as_of(&self) -> CivilDate {
        self.as_of
    }
    pub fn opening_date(&self) -> CivilDate {
        self.opening_date
    }
    pub fn closing(&self) -> &ClosingEngine {
        &self.closing
    }
    pub fn member_id(&self) -> MemberId {
        MemberId(self.company.0.clone())
    }
    pub(crate) fn income_tax_position(&self) -> &IncomeTaxPosition {
        &self.income_tax_position
    }
    pub(crate) fn next_event_id(&self) -> u64 {
        self.next_event_id
    }
    pub(crate) fn install_correction(
        &mut self,
        books: Books,
        closing: ClosingEngine,
        position: IncomeTaxPosition,
        next_event_id: u64,
    ) -> Result<(), SimpleFinanceError> {
        let mut candidate = self.clone();
        candidate.books = books;
        candidate.closing = closing;
        candidate.income_tax_position = position;
        candidate.next_event_id = next_event_id;
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }
    pub fn prepare_report(
        &mut self,
        period: AccountingPeriod,
        kind: ReportKind,
    ) -> Result<&ReportSet, SimpleFinanceError> {
        self.report(period, kind)
            .ok_or_else(|| SimpleFinanceError::Invalid(format!("报告期间 {period} 尚未定稿")))
    }
    pub fn report(&self, period: AccountingPeriod, kind: ReportKind) -> Option<&ReportSet> {
        self.closing
            .versions(&ScopeId::Standalone(self.member_id()), period, kind)
            .last()
    }
}
