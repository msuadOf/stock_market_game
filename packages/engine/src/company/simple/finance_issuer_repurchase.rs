//! Simple 账面发行人回购事实登记（ADR-0038，2026-10-07 M 批）。
//!
//! 回购资金是显式授权的合成事实：获批计划额度凭空生成发行人结算资金（专款
//! 语义），经真实委托与成交买入；卖方投资者真实收到资金。发行人侧只登记合成
//! 来源与去向的账面事实（额度、实际支出、回收差额），不追踪真实公司资金链、
//! 不伪造公司现金科目。注销按 面值×股数 核减注册资本（面值口径与送转/配股一致）。

use super::*;

impl SimpleFinanceState {
    pub fn issuer_repurchase_facts(
        &self,
    ) -> Result<Vec<crate::company::issuer_repurchase::IssuerRepurchaseFinanceFact>, SimpleFinanceError>
    {
        Ok(self.issuer_repurchases.values().cloned().collect())
    }

    /// 冻结一次回购方案的账面合成资金来源事实；同一事件幂等，内容不同显式冲突。
    pub fn declare_issuer_repurchase(
        &mut self,
        fact: crate::company::issuer_repurchase::IssuerRepurchaseFinanceFact,
    ) -> Result<bool, SimpleFinanceError> {
        use crate::company::issuer_repurchase::IssuerRepurchaseFinanceFact;
        let IssuerRepurchaseFinanceFact {
            ref event_id,
            ref approval_reference,
            approved_on,
            synthetic_funding,
            ref purpose,
            ref spent,
            ref withdrawn_remainder,
            completed_on,
            cancelled_shares,
            cancelled_on,
            ref capital_reduction,
        } = &fact;
        if let Some(existing) = self.issuer_repurchases.get(event_id) {
            let same = existing.event_id == *event_id
                && existing.approval_reference == *approval_reference
                && existing.approved_on == *approved_on
                && existing.synthetic_funding == *synthetic_funding
                && existing.purpose == *purpose
                && existing.spent == *spent
                && existing.withdrawn_remainder == *withdrawn_remainder
                && existing.completed_on == *completed_on
                && existing.cancelled_shares == *cancelled_shares
                && existing.cancelled_on == *cancelled_on
                && existing.capital_reduction == *capital_reduction;
            if same {
                return Ok(true);
            }
            return Err(SimpleFinanceError::StockDistributionConflict(
                event_id.clone(),
            ));
        }
        if event_id.trim().is_empty()
            || approval_reference.trim().is_empty()
            || !synthetic_funding.is_positive()
            || spent.is_some()
            || withdrawn_remainder.is_some()
            || completed_on.is_some()
            || *cancelled_shares != 0
            || cancelled_on.is_some()
            || capital_reduction.is_some()
        {
            return Err(SimpleFinanceError::StockDistributionInvalid(
                "回购声明必须是正数额度且不得携带回填字段".into(),
            ));
        }
        if *approved_on <= self.as_of {
            return Err(SimpleFinanceError::StockDistributionInvalid(
                "回购批准日期必须晚于最后结算日，避免向已封账期间回写".into(),
            ));
        }
        let mut candidate = self.clone();
        candidate.issuer_repurchases.insert(event_id.clone(), fact.clone());
        candidate.validate()?;
        *self = candidate;
        Ok(false)
    }

    /// 计划完成回填：实际支出与回收差额；同一事件只允许回填一次。
    pub fn record_issuer_repurchase_completion(
        &mut self,
        event_id: &str,
        completed_on: CivilDate,
        spent: crate::accounting::AccountingAmount,
        withdrawn_remainder: crate::accounting::AccountingAmount,
    ) -> Result<bool, SimpleFinanceError> {
        let fact = self.issuer_repurchases.get(event_id).ok_or_else(|| {
            SimpleFinanceError::StockDistributionInvalid(format!("未声明的回购方案 {event_id}"))
        })?;
        if let Some(existing) = fact.completed_on {
            if existing == completed_on
                && fact.spent == Some(spent)
                && fact.withdrawn_remainder == Some(withdrawn_remainder)
            {
                return Ok(true);
            }
            return Err(SimpleFinanceError::StockDistributionConflict(format!(
                "{event_id} 已按不同事实回填完成"
            )));
        }
        if spent
            .add(withdrawn_remainder)
            .map_err(|error| SimpleFinanceError::StockDistributionInvalid(error.to_string()))?
            != fact.synthetic_funding
        {
            return Err(SimpleFinanceError::StockDistributionInvalid(
                "回购实际支出与回收差额之和必须等于获批额度".into(),
            ));
        }
        // 零支出（从未成交）也合法：回收全额；日期只须不早于批准日。
        if completed_on < fact.approved_on {
            return Err(SimpleFinanceError::StockDistributionInvalid(
                "回购完成日期早于批准日期".into(),
            ));
        }
        let mut candidate = self.clone();
        let fact = candidate
            .issuer_repurchases
            .get_mut(event_id)
            .expect("fact existence was checked above");
        fact.completed_on = Some(completed_on);
        fact.spent = Some(spent);
        fact.withdrawn_remainder = Some(withdrawn_remainder);
        candidate.validate()?;
        *self = candidate;
        Ok(false)
    }

    /// 注销回填：按面值×股数核减注册资本并过账汇总分录。面值优先沿用送转/配股
    /// 已绑定口径，否则由「当前法定注册资本 ÷ 当前已发行股数」整除推导
    ///（不可整除则显式拒绝）。
    ///
    /// 注销分录（简化口径，N3 批 2026-10-08，登记于 docs/company-accounting.md）：
    /// 借 4001（股本按面值核减）、贷 simple_capital_reserve（核减额等额归集）。
    /// 回购资金为 ADR-0038 合成来源、从未进入 Simple 账面权益，因此注销不按
    /// 实际成交成本核减权益总额（权益总额不变）；真实 A 股按库存股成本注销并
    /// 核减股本与资本公积/留存收益的口径登记为简化差异。
    pub fn record_issuer_repurchase_cancellation(
        &mut self,
        event_id: &str,
        cancelled_on: CivilDate,
        cancelled_shares: u64,
        issued_shares_before: u64,
    ) -> Result<bool, SimpleFinanceError> {
        let fact = self.issuer_repurchases.get(event_id).ok_or_else(|| {
            SimpleFinanceError::StockDistributionInvalid(format!("未声明的回购方案 {event_id}"))
        })?;
        if fact.completed_on.is_none() {
            return Err(SimpleFinanceError::StockDistributionInvalid(
                "回购注销必须在计划完成后回填".into(),
            ));
        }
        if let Some(existing) = fact.cancelled_on {
            if existing == cancelled_on && fact.cancelled_shares == cancelled_shares {
                return Ok(true);
            }
            return Err(SimpleFinanceError::StockDistributionConflict(format!(
                "{event_id} 已按不同事实回填注销"
            )));
        }
        if cancelled_shares == 0 || cancelled_on < fact.approved_on {
            return Err(SimpleFinanceError::StockDistributionInvalid(
                "回购注销股数必须为正数且日期不早于批准日".into(),
            ));
        }
        // 面值绑定：优先沿用送转/配股已绑定口径，否则由当前法定事实整除推导。
        let par_cents = if let Some(first) = self.stock_distributions.values().next() {
            i128::from(first.par_value_per_share.cents())
        } else if let Some(first) = self.rights_offerings.values().next() {
            i128::from(first.par_value_per_share.cents())
        } else {
            let legal = self.legal_facts.0.as_ref().ok_or_else(|| {
                SimpleFinanceError::StockDistributionInvalid(
                    "回购注销核减注册资本缺少已绑定的法定事实".into(),
                )
            })?;
            let capital = i128::from(legal.registered_capital.to_money()?.cents());
            let shares = i128::from(issued_shares_before);
            if shares <= 0 || capital % shares != 0 {
                return Err(SimpleFinanceError::StockDistributionInvalid(
                    "注册资本与发行股数不能整除为每股面值；需先绑定可整除的法定事实".into(),
                ));
            }
            capital / shares
        };
        let reduction = crate::accounting::AccountingAmount::from_cents(
            par_cents
                .checked_mul(i128::from(cancelled_shares))
                .ok_or_else(|| {
                    SimpleFinanceError::StockDistributionInvalid("回购股本核减金额溢出".into())
                })?,
        );
        let mut candidate = self.clone();
        // 注销分录：借 4001（面值核减）、贷资本公积（等额归集）；权益内部结转。
        candidate.post_capital_action(
            cancelled_on,
            BusinessKind::CompanyRepurchaseCancellation,
            vec![
                line("4001", PostingSide::Debit, reduction),
                line(
                    crate::accounting::reports::simple_summary::CAPITAL_RESERVE,
                    PostingSide::Credit,
                    reduction,
                ),
            ],
        )?;
        let fact = candidate
            .issuer_repurchases
            .get_mut(event_id)
            .expect("fact existence was checked above");
        fact.cancelled_on = Some(cancelled_on);
        fact.cancelled_shares = cancelled_shares;
        fact.capital_reduction = Some(reduction);
        let evolved = candidate
            .legal_facts
            .0
            .as_ref()
            .ok_or_else(|| {
                SimpleFinanceError::StockDistributionInvalid(
                    "回购注销核减注册资本缺少已绑定的法定事实".into(),
                )
            })?
            .registered_capital
            .sub(reduction)?;
        if let Some(facts) = candidate.legal_facts.0.as_mut() {
            facts.registered_capital = evolved;
        }
        candidate.validate()?;
        *self = candidate;
        Ok(false)
    }
}
