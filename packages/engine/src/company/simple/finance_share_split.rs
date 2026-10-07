//! Simple 账面拆股／缩股（股份重新计值）展示登记。
//!
//! 面值口径（依据分级见 docs/trading-rules.md「拆股／缩股」节）：
//!
//! - **拆股（1 拆 N）**：每股面值 ÷ N（须整除为整数分），注册资本不变，
//!   股数 × N。A 股无先例，按标准股份拆细语义登记为游戏实现口径。
//! - **缩股（N 并 1）**：每股面值 × N，股数按碎股算法换算（略少于 ÷ N），
//!   注册资本按「旧股口径消灭面值：par_before×(S_before−ratio×S_after)」核减
//!   （形式减资，不向股东分配资产）。
//! - **面值权威链**：任一拆股／缩股声明把每股面值重锚到其 `par_value_after`；
//!   其后的送转声明与后续拆股／缩股声明必须沿用该面值，直到下一次重新计值。
//!   无拆股事实时沿用送转批次的 bind-once 面值口径。
//!
//! Simple 账面只冻结面值口径展示事实，不做借贷过账、不产生公司或投资者
//! 现金；真实股份换算由 Session 侧的股东名册（`MovementScope::
//! ShareReDenomination`）与账户结算完成。

use super::*;

impl SimpleFinanceState {
    pub fn share_split_facts(
        &self,
    ) -> Result<Vec<crate::company::share_split::ShareSplitFinanceFact>, SimpleFinanceError>
    {
        Ok(self.share_splits.values().cloned().collect())
    }

    /// 当前每股面值权威：最近的拆股／缩股声明重锚其后沿用；无重新计值事实时
    /// 沿用送转 bind-once 面值（送转声明须彼此一致，不一致返回 `None` 由调用方
    /// 显式处理，不静默取第一个）。
    pub fn current_par_value(
        &self,
    ) -> Result<Option<crate::money::Money>, SimpleFinanceError> {
        if let Some(latest) = self
            .share_splits
            .values()
            .max_by(|left, right| {
                (left.approved_on, &left.event_id).cmp(&(right.approved_on, &right.event_id))
            })
        {
            return Ok(Some(latest.par_value_after));
        }
        let mut pars = self
            .stock_distributions
            .values()
            .map(|fact| fact.par_value_per_share);
        let Some(first) = pars.next() else {
            return Ok(None);
        };
        if pars.all(|par| par == first) {
            Ok(Some(first))
        } else {
            Err(SimpleFinanceError::StockDistributionInvalid(
                "既有送转声明的每股面值不一致，无法确定后续送转沿用面值".into(),
            ))
        }
    }

    /// 冻结一次拆股／缩股声明的账面展示事实；同一事件身份重复声明幂等，
    /// 内容不同显式冲突。
    pub fn declare_share_split(
        &mut self,
        declaration: crate::company::share_split::ShareSplitDeclaration,
    ) -> Result<bool, SimpleFinanceError> {
        use crate::company::share_split::ShareSplitDirection;
        if let Some(existing) = self.share_splits.get(&declaration.event_id) {
            if existing.event_id == declaration.event_id
                && existing.approval_reference == declaration.approval_reference
                && existing.direction == declaration.direction
                && existing.approved_on == declaration.approved_on
                && existing.ratio == declaration.ratio
                && existing.par_value_before == declaration.par_value_before
                && existing.par_value_after == declaration.par_value_after
                && existing.registered_capital_at_approval
                    == declaration.registered_capital_at_approval
            {
                return Ok(true);
            }
            return Err(SimpleFinanceError::ShareSplitConflict(declaration.event_id));
        }
        if declaration.event_id.trim().is_empty()
            || declaration.approval_reference.trim().is_empty()
            || declaration.ratio < 2
            || declaration.par_value_before <= crate::money::Money::ZERO
            || declaration.par_value_after <= crate::money::Money::ZERO
        {
            return Err(SimpleFinanceError::ShareSplitInvalid(
                "拆股／缩股事件身份、批准引用、≥2 整数比例与正数面值必须明确".into(),
            ));
        }
        if declaration.approved_on <= self.as_of {
            return Err(SimpleFinanceError::ShareSplitInvalid(
                "拆股／缩股批准日期必须晚于最后结算日，避免向已封账期间回写".into(),
            ));
        }
        let legal_facts = self.legal_facts.0.as_ref().ok_or_else(|| {
            SimpleFinanceError::ShareSplitInvalid(
                "拆股／缩股面值口径需要显式绑定的公司注册资本来源事实".into(),
            )
        })?;
        if declaration.registered_capital_at_approval != legal_facts.registered_capital {
            return Err(SimpleFinanceError::ShareSplitInvalid(
                "拆股／缩股声明的注册资本与公司已绑定法律事实不一致".into(),
            ));
        }
        // 面值权威链：声明时点的当前面值必须与 par_value_before 一致。
        match self.current_par_value()? {
            Some(current) if current == declaration.par_value_before => {}
            Some(current) => {
                return Err(SimpleFinanceError::ShareSplitInvalid(format!(
                    "拆股／缩股声明面值 {} 分与当前面值权威 {} 分不一致",
                    declaration.par_value_before.cents(),
                    current.cents(),
                )));
            }
            None => {}
        }
        let before_cents = declaration.par_value_before.cents();
        let after_cents = declaration.par_value_after.cents();
        match declaration.direction {
            ShareSplitDirection::Split => {
                if before_cents % (declaration.ratio as i64) != 0
                    || after_cents != before_cents / (declaration.ratio as i64)
                {
                    return Err(SimpleFinanceError::ShareSplitInvalid(
                        "拆股面值必须按整数比例整除缩小（面值×股本守恒，注册资本不变）"
                            .into(),
                    ));
                }
            }
            ShareSplitDirection::Consolidate => {
                if after_cents != before_cents
                    .checked_mul(declaration.ratio as i64)
                    .ok_or_else(|| {
                        SimpleFinanceError::ShareSplitInvalid("缩股面值溢出".into())
                    })?
                {
                    return Err(SimpleFinanceError::ShareSplitInvalid(
                        "缩股面值必须按整数比例放大（面值×股本近似守恒，碎股面值核减）"
                            .into(),
                    ));
                }
            }
        }
        let mut candidate = self.clone();
        candidate.share_splits.insert(
            declaration.event_id.clone(),
            crate::company::share_split::ShareSplitFinanceFact {
                event_id: declaration.event_id,
                approval_reference: declaration.approval_reference,
                direction: declaration.direction,
                approved_on: declaration.approved_on,
                ratio: declaration.ratio,
                par_value_before: declaration.par_value_before,
                par_value_after: declaration.par_value_after,
                registered_capital_at_approval: declaration.registered_capital_at_approval,
                issued_shares_before: 0,
                issued_shares_after: 0,
                destroyed_shares: 0,
                registered_capital_reduction: AccountingAmount::from_cents(0),
                settled_on: None,
            },
        );
        candidate.validate()?;
        *self = candidate;
        Ok(false)
    }

    /// 真实换算入账后回填账面事实并演进注册资本法定事实；同一事件只允许回填一次。
    ///
    /// - 拆股：注册资本不变（面值÷N × 股数×N 恒等），`registered_capital_reduction`
    ///   为零。
    /// - 缩股：注册资本按「旧股口径消灭面值：par_before×(S_before−ratio×S_after)」
    ///   核减（`destroyed_shares` 字段另记 `before − after` 的股数差，非旧股等价量），
    ///   `source_evidence` 不变，显式 bind-once 入口仍拒绝直接改写注册资本。
    pub fn record_share_split_credit(
        &mut self,
        event_id: &str,
        settled_on: CivilDate,
        issued_shares_before: u64,
        issued_shares_after: u64,
    ) -> Result<bool, SimpleFinanceError> {
        let fact = self.share_splits.get(event_id).ok_or_else(|| {
            SimpleFinanceError::ShareSplitInvalid(format!("未声明的拆股／缩股事件 {event_id}"))
        })?;
        if let Some(existing) = fact.settled_on {
            if existing == settled_on
                && fact.issued_shares_before == issued_shares_before
                && fact.issued_shares_after == issued_shares_after
            {
                return Ok(true);
            }
            return Err(SimpleFinanceError::ShareSplitConflict(format!(
                "{event_id} 已按不同日期或股数回填入账"
            )));
        }
        if settled_on < fact.approved_on {
            return Err(SimpleFinanceError::ShareSplitInvalid(
                "拆股／缩股入账日期早于批准日期".into(),
            ));
        }
        if issued_shares_after > issued_shares_before {
            // 拆股：股数必须精确放大 ratio 倍。
            match fact.direction {
                crate::company::share_split::ShareSplitDirection::Split => {
                    let expected = issued_shares_before
                        .checked_mul(fact.ratio)
                        .ok_or_else(|| {
                            SimpleFinanceError::ShareSplitInvalid("拆股股数溢出".into())
                        })?;
                    if issued_shares_after != expected {
                        return Err(SimpleFinanceError::ShareSplitInvalid(format!(
                            "拆股事件 {event_id} 的入账股数与比例换算不一致"
                        )));
                    }
                }
                crate::company::share_split::ShareSplitDirection::Consolidate => {
                    return Err(SimpleFinanceError::ShareSplitInvalid(
                        "缩股事件的入账股数不得大于换算前股数".into(),
                    ));
                }
            }
        }
        let destroyed = match fact.direction {
            crate::company::share_split::ShareSplitDirection::Split => 0,
            crate::company::share_split::ShareSplitDirection::Consolidate => {
                issued_shares_before.checked_sub(issued_shares_after).ok_or_else(|| {
                    SimpleFinanceError::ShareSplitInvalid("缩股消灭股数下溢".into())
                })?
            }
        };
        // 注册资本核减按「旧股口径的消灭面值」计：每 N 股旧股（面值 par_before）
        // 换 1 股新股（面值 N×par_before），面值逐旧股守恒；碎股消灭部分的旧股
        // 等价量 = S_before − ratio × S_after，核减额 = par_before × 该等价量。
        let old_equivalent_destroyed = match fact.direction {
            crate::company::share_split::ShareSplitDirection::Split => 0,
            crate::company::share_split::ShareSplitDirection::Consolidate => issued_shares_before
                .checked_sub(
                    fact.ratio
                        .checked_mul(issued_shares_after)
                        .ok_or_else(|| {
                            SimpleFinanceError::ShareSplitInvalid("缩股换算等价量溢出".into())
                        })?,
                )
                .ok_or_else(|| {
                    SimpleFinanceError::ShareSplitInvalid("缩股换算等价量下溢".into())
                })?,
        };
        let reduction_cents = i128::from(fact.par_value_before.cents())
            .checked_mul(i128::from(old_equivalent_destroyed))
            .ok_or_else(|| SimpleFinanceError::ShareSplitInvalid("缩股核减金额溢出".into()))?;
        let mut candidate = self.clone();
        // 缩股核减分录（拆股面值总额不变、不产生分录）：借 4001（消灭面值核减）、
        // 贷资本公积（等额归集）；与回购注销同一简化口径（权益内部结转）。
        if reduction_cents > 0 {
            candidate.post_capital_action(
                settled_on,
                BusinessKind::CompanyShareReDenomination,
                vec![
                    line(
                        "4001",
                        PostingSide::Debit,
                        AccountingAmount::from_cents(reduction_cents),
                    ),
                    line(
                        crate::accounting::reports::simple_summary::CAPITAL_RESERVE,
                        PostingSide::Credit,
                        AccountingAmount::from_cents(reduction_cents),
                    ),
                ],
            )?;
        }
        let fact = candidate
            .share_splits
            .get_mut(event_id)
            .expect("fact existence was checked above");
        fact.issued_shares_before = issued_shares_before;
        fact.issued_shares_after = issued_shares_after;
        fact.destroyed_shares = destroyed;
        fact.registered_capital_reduction = AccountingAmount::from_cents(reduction_cents);
        fact.settled_on = Some(settled_on);
        if reduction_cents > 0 {
            let current = candidate
                .legal_facts
                .0
                .as_ref()
                .ok_or_else(|| {
                    SimpleFinanceError::ShareSplitInvalid(
                        "缩股核减注册资本缺少已绑定的法定事实".into(),
                    )
                })?
                .registered_capital;
            let reduced = current
                .sub(AccountingAmount::from_cents(reduction_cents))
                .map_err(|error| {
                    SimpleFinanceError::ShareSplitInvalid(format!("缩股核减注册资本失败：{error}"))
                })?;
            if let Some(facts) = candidate.legal_facts.0.as_mut() {
                facts.registered_capital = reduced;
            }
        }
        candidate.validate()?;
        *self = candidate;
        Ok(false)
    }
}

#[cfg(test)]
#[path = "finance_share_split_tests.rs"]
mod tests;
