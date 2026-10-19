//! Simple 账面送转登记与汇总分录（N3 批 2026-10-08 起过账进 `Books`）。
//!
//! 送转在真实入账回填时按已定口径做权益内部结转分录：送股（股票股利）借
//! `4103`（来自可分配利润）、转增借 `simple_capital_reserve`（转增来源），贷
//! `4001`（股本按面值增加）。分录不产生公司现金或投资者现金；真实股份入账由
//! Session 侧的股东名册与账户结算完成。现实语义为每股面值恒定、送转入账后
//! 注册资本按 面值×新增股数 演进：入账回填时同步演进注册资本法定事实（历史由
//! 各事实的 `registered_capital_at_approval` 冻结保留），发行股数事实由
//! `IssuerRegistry` 在真实入账时更新。

use super::*;

impl SimpleFinanceState {
    pub fn stock_distribution_facts(
        &self,
    ) -> Result<Vec<StockDistributionFinanceFact>, SimpleFinanceError> {
        Ok(self.stock_distributions.values().cloned().collect())
    }

    // 来源预留只在批准/入账时点判定（declare_* 路径显式校验），不作为跨期
    // validate 不变量——批准后利润变动（亏损月、更正）会使冻结占用额相对当期
    // 利润「超占」，此时必须仍可月结推进到入账日，否则已批准送转把日终卡死
    //（独立复核修复轮发现）。

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
                // 转增来源为资本公积科目：面值总额不得超过当前资本公积贷方余额扣减
                // 既有未入账转增声明的已占用额（预留语义，防止双批后日终入账卡死）。
                let mut reserve = self
                    .books
                    .ledger()
                    .account_net_debit(&LedgerAccountId(
                        crate::accounting::reports::simple_summary::CAPITAL_RESERVE.into(),
                    ))?
                    .neg()?;
                for existing in self.stock_distributions.values().filter(|f| {
                    f.credited_on.is_none()
                        && matches!(
                            f.kind,
                            crate::company::stock_distribution::StockDistributionKind::CapitalReserveConversion
                        )
                }) {
                    reserve = reserve.sub(existing.capital_increase)?;
                }
                if declaration.capital_increase > reserve {
                    return Err(SimpleFinanceError::StockDistributionInvalid(format!(
                        "转增面值总额 {} 超过资本公积科目可用余额 {}",
                        declaration.capital_increase.to_yuan_string(),
                        reserve.to_yuan_string()
                    )));
                }
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
                statutory_reserve: AccountingAmount::ZERO,
                reserve_basis_year: None,
            },
        );
        candidate.validate()?;
        *self = candidate;
        Ok(false)
    }

    /// 真实新股入账后回填账面事实并过账权益内部结转分录；同一事件只允许回填一次。
    ///
    /// 现实语义为每股面值恒定、送转后注册资本按面值增加：入账时把注册资本法定
    /// 事实演进为「当前注册资本 + 面值×新增股数」（即声明的 `capital_increase`），
    /// `source_evidence` 不变；显式 bind-once 入口仍拒绝直接改写注册资本。历史由
    /// 各 `StockDistributionFinanceFact.registered_capital_at_approval` 冻结保留。
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
        let capital_increase = fact.capital_increase;
        let mut candidate = self.clone();
        // 汇总分录：送股借 4103（可分配利润减少）并按年度计提法定公积金；
        // 转增借资本公积（来源减少），贷 4001（股本按面值增加）。送股入账是
        // 冻结义务：批准后利润变动不回溯阻断入账（否则日终卡死在入账日前）。
        let (source_lines, statutory_reserve, reserve_basis_year) = match fact.kind {
            crate::company::stock_distribution::StockDistributionKind::BonusShares => {
                let profit = candidate.distributable_profit()?;
                let mut lines = Vec::new();
                let retained = capital_increase.add(profit.statutory_reserve)?;
                lines.push(line("4103", PostingSide::Debit, retained));
                if profit.statutory_reserve.is_positive() {
                    lines.push(line(
                        crate::accounting::reports::simple_summary::STATUTORY_RESERVE,
                        PostingSide::Credit,
                        profit.statutory_reserve,
                    ));
                }
                (lines, profit.statutory_reserve, profit.reserve_basis_year)
            }
            crate::company::stock_distribution::StockDistributionKind::CapitalReserveConversion => {
                // 入账前再次核对转增来源余额（声明与入账之间余额可能已变动）。
                let reserve = candidate
                    .books
                    .ledger()
                    .account_net_debit(&LedgerAccountId(
                        crate::accounting::reports::simple_summary::CAPITAL_RESERVE.into(),
                    ))?
                    .neg()?;
                if capital_increase > reserve {
                    return Err(SimpleFinanceError::StockDistributionInvalid(format!(
                        "转增面值总额 {} 超过资本公积科目当前贷方余额 {}",
                        capital_increase.to_yuan_string(),
                        reserve.to_yuan_string()
                    )));
                }
                (
                    vec![line(
                        crate::accounting::reports::simple_summary::CAPITAL_RESERVE,
                        PostingSide::Debit,
                        capital_increase,
                    )],
                    AccountingAmount::ZERO,
                    None,
                )
            }
        };
        let mut post_lines = source_lines;
        post_lines.push(line("4001", PostingSide::Credit, capital_increase));
        candidate.post_capital_action(
            credited_on,
            BusinessKind::CompanyStockDistributionCredit,
            post_lines,
        )?;
        let fact = candidate
            .stock_distributions
            .get_mut(event_id)
            .expect("fact existence was checked above");
        fact.credited_on = Some(credited_on);
        fact.statutory_reserve = statutory_reserve;
        fact.reserve_basis_year = reserve_basis_year;
        let evolved = candidate
            .legal_facts
            .0
            .as_ref()
            .ok_or_else(|| {
                SimpleFinanceError::StockDistributionInvalid(
                    "送转入账演进注册资本缺少已绑定的法定事实".into(),
                )
            })?
            .registered_capital
            .add(capital_increase)?;
        if let Some(facts) = candidate.legal_facts.0.as_mut() {
            facts.registered_capital = evolved;
        }
        candidate.validate()?;
        *self = candidate;
        Ok(false)
    }
}
