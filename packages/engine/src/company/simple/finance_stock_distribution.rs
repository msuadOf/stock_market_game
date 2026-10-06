//! Simple 账面送转展示登记。
//!
//! 本批 Simple 账面未建模资本公积科目与来源类别，也不变更注册资本法定事实，
//! 因此送转在这里只冻结面值口径的展示事实（`StockDistributionFinanceFact`），
//! 不做借贷过账、不产生公司现金或投资者现金；真实股份入账由 Session 侧的
//! 股东名册与账户结算完成。发行股数事实由 `IssuerRegistry` 在真实入账时更新。

use super::*;

impl SimpleFinanceState {
    pub fn stock_distribution_facts(
        &self,
    ) -> Result<Vec<StockDistributionFinanceFact>, SimpleFinanceError> {
        Ok(self.stock_distributions.values().cloned().collect())
    }

    /// 冻结一次送转声明的账面展示事实；同一事件身份重复声明幂等，内容不同显式冲突。
    pub fn declare_stock_distribution(
        &mut self,
        declaration: StockDistributionDeclaration,
    ) -> Result<bool, SimpleFinanceError> {
        if let Some(existing) = self.stock_distributions.get(&declaration.event_id) {
            if existing.event_id == declaration.event_id
                && existing.approval_reference == declaration.approval_reference
                && existing.kind == declaration.kind
                && existing.approved_on == declaration.approved_on
                && existing.new_shares == declaration.new_shares
                && existing.par_value_per_share == declaration.par_value_per_share
                && existing.capital_increase == declaration.capital_increase
                && existing.registered_capital_at_approval == declaration.registered_capital_at_approval
            {
                return Ok(true);
            }
            return Err(SimpleFinanceError::StockDistributionConflict(
                declaration.event_id,
            ));
        }
        if declaration.event_id.trim().is_empty()
            || declaration.approval_reference.trim().is_empty()
            || declaration.new_shares == 0
            || declaration.par_value_per_share <= crate::money::Money::ZERO
        {
            return Err(SimpleFinanceError::StockDistributionInvalid(
                "送转事件身份、批准引用、正数新增股数与正数每股面值必须明确".into(),
            ));
        }
        if declaration.approved_on <= self.as_of {
            return Err(SimpleFinanceError::StockDistributionInvalid(
                "送转批准日期必须晚于最后结算日，避免向已封账期间回写".into(),
            ));
        }
        let legal_facts = self.legal_facts.0.as_ref().ok_or_else(|| {
            SimpleFinanceError::StockDistributionInvalid(
                "送转面值口径需要显式绑定的公司注册资本来源事实".into(),
            )
        })?;
        if declaration.registered_capital_at_approval != legal_facts.registered_capital {
            return Err(SimpleFinanceError::StockDistributionInvalid(
                "送转声明的注册资本与公司已绑定法律事实不一致".into(),
            ));
        }
        let expected_capital_cents = i128::from(declaration.par_value_per_share.cents())
            .checked_mul(i128::from(declaration.new_shares))
            .ok_or_else(|| {
                SimpleFinanceError::StockDistributionInvalid("送转股本增加金额溢出".into())
            })?;
        if declaration.capital_increase != AccountingAmount::from_cents(expected_capital_cents) {
            return Err(SimpleFinanceError::StockDistributionInvalid(
                "送转股本增加金额必须等于每股面值乘以新增股数".into(),
            ));
        }
        match declaration.kind {
            crate::company::stock_distribution::StockDistributionKind::BonusShares => {
                // 送股（股票股利）来自可分配利润：面值总额不得超过已弥补亏损、
                // 提取法定公积金后的可分配利润（《公司法》第 210 条）。
                let profit = self.distributable_profit()?;
                if declaration.capital_increase > profit.available_for_distribution {
                    return Err(SimpleFinanceError::StockDistributionInvalid(format!(
                        "送股面值总额 {} 超过可分配利润 {}",
                        declaration.capital_increase.to_yuan_string(),
                        profit.available_for_distribution.to_yuan_string()
                    )));
                }
            }
            crate::company::stock_distribution::StockDistributionKind::CapitalReserveConversion => {
                // 资本公积转增的余额与来源类别未在 Simple 账面建模（无资本公积科目，
                // 也未建模法定公积金 25% 留存下限的适用差异）；本批不做账面前置校验，
                // 该边界在 docs/trading-rules.md 显式登记。
            }
        }
        let mut candidate = self.clone();
        candidate.stock_distributions.insert(
            declaration.event_id.clone(),
            StockDistributionFinanceFact {
                event_id: declaration.event_id,
                approval_reference: declaration.approval_reference,
                kind: declaration.kind,
                approved_on: declaration.approved_on,
                new_shares: declaration.new_shares,
                par_value_per_share: declaration.par_value_per_share,
                capital_increase: declaration.capital_increase,
                registered_capital_at_approval: declaration.registered_capital_at_approval,
                credited_on: None,
            },
        );
        candidate.validate()?;
        *self = candidate;
        Ok(false)
    }

    /// 真实新股入账后回填账面事实；同一事件只允许回填一次。
    pub fn record_stock_distribution_credit(
        &mut self,
        event_id: &str,
        credited_on: CivilDate,
        new_shares: u64,
    ) -> Result<bool, SimpleFinanceError> {
        let fact = self.stock_distributions.get(event_id).ok_or_else(|| {
            SimpleFinanceError::StockDistributionInvalid(format!("未声明的送转事件 {event_id}"))
        })?;
        if let Some(existing) = fact.credited_on {
            if existing == credited_on && fact.new_shares == new_shares {
                return Ok(true);
            }
            return Err(SimpleFinanceError::StockDistributionConflict(format!(
                "{event_id} 已按不同日期或数量回填入账"
            )));
        }
        if fact.new_shares != new_shares {
            return Err(SimpleFinanceError::StockDistributionInvalid(format!(
                "送转事件 {event_id} 的入账股数与声明不一致"
            )));
        }
        if credited_on < fact.approved_on {
            return Err(SimpleFinanceError::StockDistributionInvalid(
                "送转入账日期早于批准日期".into(),
            ));
        }
        let mut candidate = self.clone();
        let fact = candidate
            .stock_distributions
            .get_mut(event_id)
            .expect("fact existence was checked above");
        fact.credited_on = Some(credited_on);
        candidate.validate()?;
        *self = candidate;
        Ok(false)
    }
}
