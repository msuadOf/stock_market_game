//! Simple 账面配股／增发募集登记（2026-10-07 M 批）。
//!
//! 发行人是配股／增发的资金接收方，但 Simple 不追踪真实公司资金链
//! （ADR-0035/0039）：投资者侧真实现金由 Session 在缴款期日终划扣；
//! 发行人侧只冻结「声明即冻结、结算后回填」的账面事实。面值口径与送转一致
//! ——每股面值恒定、注册资本按 面值×新增股数 演进；发行价超出面值的溢价
//! 不建模资本公积科目（登记边界，见 docs/trading-rules.md）。

use super::*;

impl SimpleFinanceState {
    pub fn rights_offering_facts(
        &self,
    ) -> Result<Vec<crate::company::rights_offering::RightsOfferingFinanceFact>, SimpleFinanceError>
    {
        Ok(self.rights_offerings.values().cloned().collect())
    }

    /// 冻结一次配股／增发的账面声明事实；同一事件身份重复声明幂等，内容不同显式冲突。
    pub fn declare_rights_offering(
        &mut self,
        declaration: crate::company::rights_offering::RightsOfferingDeclaration,
    ) -> Result<bool, SimpleFinanceError> {
        use crate::company::rights_offering::RightsOfferingDeclaration;
        let RightsOfferingDeclaration {
            ref event_id,
            ref approval_reference,
            approved_on,
            price_per_share,
            par_value_per_share,
            registered_capital_at_approval,
        } = declaration;
        if let Some(existing) = self.rights_offerings.get(event_id) {
            let same = existing.event_id == *event_id
                && existing.approval_reference == *approval_reference
                && existing.approved_on == approved_on
                && existing.price_per_share == price_per_share
                && existing.par_value_per_share == par_value_per_share
                && existing.registered_capital_at_approval == registered_capital_at_approval;
            if same {
                return Ok(true);
            }
            return Err(SimpleFinanceError::StockDistributionConflict(
                event_id.clone(),
            ));
        }
        if event_id.trim().is_empty()
            || approval_reference.trim().is_empty()
            || price_per_share <= crate::money::Money::ZERO
            || par_value_per_share <= crate::money::Money::ZERO
        {
            return Err(SimpleFinanceError::StockDistributionInvalid(
                "配股事件身份、批准引用、正数发行价与正数每股面值必须明确".into(),
            ));
        }
        if price_per_share < par_value_per_share {
            return Err(SimpleFinanceError::StockDistributionInvalid(
                "配股发行价不得低于每股面值（《公司法》第 148 条）".into(),
            ));
        }
        if approved_on <= self.as_of {
            return Err(SimpleFinanceError::StockDistributionInvalid(
                "配股批准日期必须晚于最后结算日，避免向已封账期间回写".into(),
            ));
        }
        let legal_facts = self.legal_facts.0.as_ref().ok_or_else(|| {
            SimpleFinanceError::StockDistributionInvalid(
                "配股面值口径需要显式绑定的公司注册资本来源事实".into(),
            )
        })?;
        if registered_capital_at_approval != legal_facts.registered_capital {
            return Err(SimpleFinanceError::StockDistributionInvalid(
                "配股声明的注册资本与公司已绑定法律事实不一致".into(),
            ));
        }
        let mut candidate = self.clone();
        candidate.rights_offerings.insert(
            event_id.clone(),
            crate::company::rights_offering::RightsOfferingFinanceFact {
                event_id: event_id.clone(),
                approval_reference: approval_reference.clone(),
                approved_on,
                price_per_share,
                par_value_per_share,
                registered_capital_at_approval,
                settled_on: None,
                issued_shares: 0,
                proceeds: None,
                capital_increase: None,
            },
        );
        candidate.validate()?;
        *self = candidate;
        Ok(false)
    }

    /// 结算回填：实际认购股数与募集资金；同一事件只允许回填一次。
    ///
    /// 注册资本按 面值×实际新增股数 演进（`source_evidence` 不变；显式 bind-once
    /// 入口仍拒绝直接改写注册资本）。发行失败（`issued_shares == 0` 且
    /// `proceeds == 0`）不演进注册资本。
    pub fn record_rights_offering_settlement(
        &mut self,
        event_id: &str,
        settled_on: CivilDate,
        issued_shares: u64,
        proceeds: crate::accounting::AccountingAmount,
    ) -> Result<bool, SimpleFinanceError> {
        let fact = self.rights_offerings.get(event_id).ok_or_else(|| {
            SimpleFinanceError::StockDistributionInvalid(format!("未声明的配股事件 {event_id}"))
        })?;
        if let Some(existing) = fact.settled_on {
            if existing == settled_on
                && fact.issued_shares == issued_shares
                && fact.proceeds == Some(proceeds)
            {
                return Ok(true);
            }
            return Err(SimpleFinanceError::StockDistributionConflict(format!(
                "{event_id} 已按不同日期或数量回填结算"
            )));
        }
        if settled_on < fact.approved_on {
            return Err(SimpleFinanceError::StockDistributionInvalid(
                "配股结算日期早于批准日期".into(),
            ));
        }
        let capital_increase = if issued_shares == 0 {
            crate::accounting::AccountingAmount::from_cents(0)
        } else {
            let cents = i128::from(fact.par_value_per_share.cents())
                .checked_mul(i128::from(issued_shares))
                .ok_or_else(|| {
                    SimpleFinanceError::StockDistributionInvalid("配股股本增加金额溢出".into())
                })?;
            crate::accounting::AccountingAmount::from_cents(cents)
        };
        let proceeds_cents = i128::from(fact.price_per_share.cents())
            .checked_mul(i128::from(issued_shares))
            .ok_or_else(|| {
                SimpleFinanceError::StockDistributionInvalid("配股募集资金金额溢出".into())
            })?;
        if proceeds != crate::accounting::AccountingAmount::from_cents(proceeds_cents) {
            return Err(SimpleFinanceError::StockDistributionInvalid(
                "配股募集资金必须等于发行价乘以实际认购股数".into(),
            ));
        }
        let mut candidate = self.clone();
        let fact = candidate
            .rights_offerings
            .get_mut(event_id)
            .expect("fact existence was checked above");
        fact.settled_on = Some(settled_on);
        fact.issued_shares = issued_shares;
        fact.proceeds = Some(proceeds);
        fact.capital_increase = Some(capital_increase);
        if issued_shares > 0 {
            let evolved = candidate
                .legal_facts
                .0
                .as_ref()
                .ok_or_else(|| {
                    SimpleFinanceError::StockDistributionInvalid(
                        "配股结算演进注册资本缺少已绑定的法定事实".into(),
                    )
                })?
                .registered_capital
                .add(capital_increase)?;
            if let Some(facts) = candidate.legal_facts.0.as_mut() {
                facts.registered_capital = evolved;
            }
        }
        candidate.validate()?;
        *self = candidate;
        Ok(false)
    }
}
