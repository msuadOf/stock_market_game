use std::collections::{BTreeMap, BTreeSet};

use super::{chart::acct, real_estate_account_chart, ProjectId, RealEstateBooks, RealEstateError};
use crate::accounting::AccountingAmount;
use crate::company::counterparty::CounterpartyId;

fn invalid(detail: impl Into<String>) -> RealEstateError {
    RealEstateError::OwnerStateInconsistent {
        detail: detail.into(),
    }
}

fn add_units(total: i128, units: i128) -> Result<i128, RealEstateError> {
    total
        .checked_add(units)
        .ok_or_else(|| invalid("合同套数汇总溢出"))
}

impl RealEstateBooks {
    pub(crate) fn validate_owner_state(&self) -> Result<(), RealEstateError> {
        self.validate_income_tax_state()?;
        self.capitalization_policy.validate()?;
        if self.max_projects == 0 || self.projects.len() > self.max_projects {
            return Err(invalid("项目数量与配置上限不一致"));
        }
        if self.budget.operating_cash_floor().is_negative() {
            return Err(invalid("经营现金下限不得为负"));
        }
        let chart = self.books.ledger().chart();
        for (account, expected) in real_estate_account_chart().iter() {
            let actual = chart
                .get(account)
                .ok_or_else(|| invalid(format!("缺少地产科目{account}")))?;
            if actual.element != expected.element
                || actual.is_cash != expected.is_cash
                || actual.is_contra != expected.is_contra
            {
                return Err(invalid(format!(
                    "地产科目{account}分类或现金／备抵标记不一致"
                )));
            }
        }
        if self.net_of(acct::CASH)?.is_negative() || self.books.ledger().cash_total()?.is_negative()
        {
            return Err(invalid("地产账套不允许负现金"));
        }
        if self
            .books
            .journal()
            .entries()
            .any(|entry| entry.source.value() >= self.next_event_id)
        {
            return Err(invalid("下一事件身份必须晚于已过账来源"));
        }
        for (id, party) in self.counterparties.iter() {
            if id != &party.id || id.0.trim().is_empty() || party.name.trim().is_empty() {
                return Err(invalid(format!("对手方{id:?}登记身份不一致或为空")));
            }
        }
        for flow in self.counterparties.flows() {
            self.ensure_counterparty(&flow.counterparty)?;
            if !flow.amount.is_positive() {
                return Err(invalid("对手方收付流水金额必须为正"));
            }
        }
        self.validate_projects_and_presales()?;
        self.validate_project_loans()?;
        self.validate_final_receivables()
    }

    fn validate_projects_and_presales(&self) -> Result<(), RealEstateError> {
        let mut reserved: BTreeMap<&ProjectId, i128> = BTreeMap::new();
        let mut delivered: BTreeMap<&ProjectId, i128> = BTreeMap::new();
        let mut liability = AccountingAmount::ZERO;
        for (id, sale) in &self.presales {
            let project = self
                .projects
                .get(sale.project())
                .ok_or_else(|| invalid(format!("预售{id:?}引用未知项目")))?;
            self.ensure_counterparty(sale.buyer())?;
            if sale.units() <= 0
                || !sale.price_total().is_positive()
                || sale.collected().is_negative()
                || sale.collected() > sale.price_total()
            {
                return Err(invalid(format!("预售{id:?}套数、总价或已收款无效")));
            }
            let totals = if sale.delivered() {
                if project.completed_on().is_none() {
                    return Err(invalid(format!("预售{id:?}已交付但项目未完工")));
                }
                &mut delivered
            } else {
                liability = liability.add(sale.collected())?;
                &mut reserved
            };
            let total = totals.entry(sale.project()).or_insert(0);
            *total = add_units(*total, sale.units())?;
        }
        let mut inventory = AccountingAmount::ZERO;
        for (id, project) in &self.projects {
            if project.total_units() <= 0
                || project.remaining_units() < 0
                || project.remaining_units() > project.total_units()
                || !project.land_cost().is_positive()
                || [
                    project.development_cost(),
                    project.capitalized_interest(),
                    project.remaining_cost(),
                    project.carried_out_cost(),
                ]
                .iter()
                .any(|amount| amount.is_negative())
            {
                return Err(invalid(format!("项目{id:?}套数或成本无效")));
            }
            let invested = project
                .land_cost()
                .add(project.development_cost())?
                .add(project.capitalized_interest())?;
            if invested != project.remaining_cost().add(project.carried_out_cost())?
                || (project.remaining_units() == 0 && !project.remaining_cost().is_zero())
            {
                return Err(invalid(format!("项目{id:?}投入、结存与结转成本不守恒")));
            }
            let delivered_units = delivered.get(id).copied().unwrap_or(0);
            let reserved_units = reserved.get(id).copied().unwrap_or(0);
            if project.total_units() - project.remaining_units() != delivered_units
                || reserved_units > project.remaining_units()
            {
                return Err(invalid(format!(
                    "项目{id:?}交付标记或未交付套数与结存不一致"
                )));
            }
            if (project.dev_started_on().is_none()
                && (!project.development_cost().is_zero()
                    || project.completed_on().is_some()
                    || project.interrupted_on().is_some()
                    || !project.interruptions().is_empty()))
                || (project.completed_on().is_some() && project.interrupted_on().is_some())
            {
                return Err(invalid(format!("项目{id:?}开发、完工或中断状态不一致")));
            }
            project.validate_timeline(id)?;
            inventory = inventory.add(project.remaining_cost())?;
        }
        if inventory != self.net_of(acct::DEV_INVENTORY)? {
            return Err(invalid("项目结存成本与1541开发存货余额不一致"));
        }
        if liability.neg()? != self.net_of(acct::CONTRACT_LIAB)? {
            return Err(invalid("未交付预售已收款与2203合同负债余额不一致"));
        }
        if self.net_of(acct::DEV_IMPAIR_ALLOW)?.is_positive() {
            return Err(invalid("开发存货减值准备不得为净借方余额"));
        }
        Ok(())
    }

    fn validate_project_loans(&self) -> Result<(), RealEstateError> {
        let mut short_term = AccountingAmount::ZERO;
        let mut long_term = AccountingAmount::ZERO;
        let mut interest = AccountingAmount::ZERO;
        for (id, loan) in &self.loans {
            self.ensure_counterparty(loan.lender())?;
            if let Some(project) = loan.project() {
                if !self.projects.contains_key(project) {
                    return Err(invalid(format!("借款{id:?}引用未知项目")));
                }
            }
            if loan.outstanding().is_negative()
                || loan.accrued_unpaid().is_negative()
                || loan.annual_rate_bp() < 0
                || loan.carried_cap().units().unsigned_abs() > 1_825_000
                || loan.carried_exp().units().unsigned_abs() > 1_825_000
                || (loan.project().is_none() && loan.carried_cap().units() != 0)
            {
                return Err(invalid(format!("借款{id:?}本金、利息、利率或双链余数无效")));
            }
            match loan.debt_account() {
                acct::ST_DEBT => short_term = short_term.add(loan.outstanding())?,
                acct::LT_DEBT => long_term = long_term.add(loan.outstanding())?,
                account => return Err(invalid(format!("借款{id:?}债务科目{account}无效"))),
            }
            interest = interest.add(loan.accrued_unpaid())?;
        }
        for (account, amount) in [
            (acct::ST_DEBT, short_term),
            (acct::LT_DEBT, long_term),
            (acct::INT_PAYABLE, interest),
        ] {
            if amount.neg()? != self.net_of(account)? {
                return Err(invalid(format!("借款子账与科目{account}余额不一致")));
            }
        }
        Ok(())
    }

    fn validate_final_receivables(&self) -> Result<(), RealEstateError> {
        let delivered_buyers: BTreeSet<_> = self
            .presales
            .values()
            .filter(|sale| sale.delivered())
            .map(|sale| sale.buyer())
            .collect();
        for (id, item) in self.receivables.iter() {
            let party = CounterpartyId(item.party().into());
            self.ensure_counterparty(&party)?;
            if item.open_amount().is_negative()
                || item.due_on() < item.opened_on()
                || !delivered_buyers.contains(&party)
            {
                return Err(invalid(format!("尾款{id:?}金额、日期或已交付买方身份无效")));
            }
        }
        if self.receivables.written_off_total()?.is_negative() {
            return Err(invalid("尾款核销总额不得为负"));
        }
        if self.receivables.total_open()? != self.net_of(acct::AR)? {
            return Err(invalid("尾款开项与1122应收账款余额不一致"));
        }
        Ok(())
    }
}
