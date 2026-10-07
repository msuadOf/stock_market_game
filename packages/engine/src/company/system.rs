use super::{
    CompanyId, CompanySpec, api::*, config::CompanySystemConfig, identity::IssuerRegistry,
    simple::SimpleFundamentals,
};
use crate::calendar::CivilDate;

#[derive(Debug, thiserror::Error)]
pub enum CompanySystemError {
    #[error("公司系统输入非法：{0}")]
    Invalid(String),
    /// 非法输入（四分类收口后的准确落点；display 与 `Invalid` 保持同形，
    /// 「不改变行为只改分类面」）。存量调用点逐步迁移，未迁移的留在 `Invalid`。
    #[error("公司系统输入非法：{0}")]
    InvalidInput(String),
    /// 业务条件拒绝（display 与 `Invalid` 保持同形，理由同上）。
    #[error("公司系统输入非法：{0}")]
    BusinessCondition(String),
    /// 系统状态错误：恢复勾稽／不变量校验发现状态不一致（display 与 `Invalid`
    /// 保持同形，理由同上）。
    #[error("公司系统输入非法：{0}")]
    SystemState(String),
    #[error("公司系统尚不支持：{0}")]
    Unsupported(String),
    #[error(transparent)]
    Accounting(#[from] crate::accounting::AccountingError),
    #[error(transparent)]
    Date(#[from] crate::calendar::CivilDateError),
    #[error(transparent)]
    Finance(#[from] super::simple::SimpleFinanceError),
}

impl CompanySystemError {
    /// 公司域公共错误的四分类附加面（R4 最小公共合同）。分类不改变任何错误
    /// 信息或控制流；映射依据见各变体注释与
    /// `packages/engine/src/company/error_classification_tests.rs`。
    pub fn classification(&self) -> super::error::CompanyErrorClass {
        use super::error::CompanyErrorClass;
        match self {
            Self::Invalid(_) | Self::InvalidInput(_) | Self::Date(_) => {
                CompanyErrorClass::InvalidInput
            }
            Self::BusinessCondition(_) => CompanyErrorClass::BusinessCondition,
            Self::SystemState(_) | Self::Accounting(_) => CompanyErrorClass::SystemState,
            Self::Unsupported(_) => CompanyErrorClass::UnsupportedOperation,
            Self::Finance(source) => source.classification(),
        }
    }
}

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "mode", content = "state", deny_unknown_fields)]
pub(crate) enum CompanyImplementation {
    Simple(SimpleFundamentals),
}

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize)]
pub struct CompanySystem {
    pub(crate) issuers: IssuerRegistry,
    pub(crate) implementation: CompanyImplementation,
    #[serde(skip)]
    pub(crate) hash_cache: CompanySystemHashCache,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub(crate) struct CompanySystemHashProjection {
    serialized_len: usize,
    digest: u64,
}

#[derive(Default, Debug)]
pub(crate) struct CompanySystemHashCache(std::sync::OnceLock<CompanySystemHashProjection>);

impl Clone for CompanySystemHashCache {
    fn clone(&self) -> Self {
        let clone = Self::default();
        if let Some(projection) = self.0.get() {
            let _ = clone.0.set(*projection);
        }
        clone
    }
}

impl PartialEq for CompanySystemHashCache {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}
impl Eq for CompanySystemHashCache {}

impl CompanySystem {
    pub fn distributable_profit(
        &self,
        company: &CompanyId,
    ) -> Result<super::dividend::DistributableProfit, CompanySystemError> {
        Ok(self.finance(company)?.distributable_profit()?)
    }

    pub fn define_dividend_legal_facts(
        &mut self,
        company: &CompanyId,
        registered_capital: crate::accounting::AccountingAmount,
        source_evidence: String,
    ) -> Result<(), CompanySystemError> {
        self.finance_mut(company)?.define_dividend_legal_facts(
            super::dividend::DividendLegalFacts {
                registered_capital,
                source_evidence,
            },
        )?;
        self.hash_cache = CompanySystemHashCache::default();
        Ok(())
    }

    pub fn declare_dividend(
        &mut self,
        company: &CompanyId,
        declaration: super::dividend::DividendDeclaration,
    ) -> Result<super::dividend::DividendPlanReceipt, CompanySystemError> {
        let result = self.finance_mut(company)?.declare_dividend(declaration)?;
        self.hash_cache = CompanySystemHashCache::default();
        Ok(result)
    }

    pub fn pay_dividend(
        &mut self,
        company: &CompanyId,
        plan_id: &str,
        payment_id: &str,
        paid_on: CivilDate,
        amount: crate::accounting::AccountingAmount,
    ) -> Result<super::dividend::DividendPaymentReceipt, CompanySystemError> {
        let result = self
            .finance_mut(company)?
            .pay_dividend(plan_id, payment_id, paid_on, amount)?;
        self.hash_cache = CompanySystemHashCache::default();
        Ok(result)
    }

    pub fn dividend_plan_facts(
        &self,
        company: &CompanyId,
    ) -> Result<Vec<super::dividend::DividendPlanFact>, CompanySystemError> {
        Ok(self.finance(company)?.dividend_plan_facts()?)
    }

    pub fn dividend_legal_facts(
        &self,
        company: &CompanyId,
    ) -> Result<Option<super::dividend::DividendLegalFacts>, CompanySystemError> {
        Ok(self.finance(company)?.legal_facts().clone())
    }

    pub fn stock_distribution_facts(
        &self,
        company: &CompanyId,
    ) -> Result<Vec<super::stock_distribution::StockDistributionFinanceFact>, CompanySystemError>
    {
        Ok(self.finance(company)?.stock_distribution_facts()?)
    }

    /// 账面展示字段查询（2026-10-08 用户决策）：现金=累计净利润−累计已付分红、
    /// 投资额=累计收入×配置比例；均为**账面展示值**，不代表真实公司资金。
    pub fn simple_book_display(
        &self,
        company: &CompanyId,
    ) -> Result<super::simple::finance::SimpleBookDisplay, CompanySystemError> {
        Ok(self.finance(company)?.book_display())
    }

    /// 冻结 Simple 账面送转声明事实；真实入账回填时过账权益内部结转分录并
    /// 计提法定公积金（N3 批起进 `Books`），全程不产生现金。
    pub fn declare_stock_distribution(
        &mut self,
        company: &CompanyId,
        declaration: super::stock_distribution::StockDistributionDeclaration,
    ) -> Result<bool, CompanySystemError> {
        let already_declared = self
            .finance_mut(company)?
            .declare_stock_distribution(declaration)?;
        Ok(already_declared)
    }

    /// 真实新股入账后回填账面事实并同步发行人已发行股数；两者在同一候选上变更。
    pub fn record_stock_distribution_credit(
        &mut self,
        company: &CompanyId,
        event_id: &str,
        credited_on: CivilDate,
        new_shares: u64,
    ) -> Result<bool, CompanySystemError> {
        let already_credited = self
            .finance_mut(company)?
            .record_stock_distribution_credit(event_id, credited_on, new_shares)?;
        if !already_credited {
            self.issuers.record_share_issuance(company, new_shares)?;
        }
        Ok(already_credited)
    }

    pub fn share_split_facts(
        &self,
        company: &CompanyId,
    ) -> Result<Vec<super::share_split::ShareSplitFinanceFact>, CompanySystemError> {
        Ok(self.finance(company)?.share_split_facts()?)
    }

    /// 当前每股面值权威（拆股／缩股重锚链；无重新计值事实时沿用送转 bind-once）。
    pub fn current_par_value(
        &self,
        company: &CompanyId,
    ) -> Result<Option<crate::money::Money>, CompanySystemError> {
        Ok(self.finance(company)?.current_par_value()?)
    }

    /// 冻结 Simple 账面拆股／缩股声明事实；缩股真实入账回填时按消灭面值过账
    /// 核减分录（拆股面值总额不变、无分录），全程不产生现金。
    pub fn declare_share_split(
        &mut self,
        company: &CompanyId,
        declaration: super::share_split::ShareSplitDeclaration,
    ) -> Result<bool, CompanySystemError> {
        let already_declared = self.finance_mut(company)?.declare_share_split(declaration)?;
        Ok(already_declared)
    }

    /// 真实换算入账后回填账面事实、演进注册资本并同步发行人已发行股数。
    ///
    /// 拆股按增量调 `record_share_issuance`；缩股按消灭股数（可为零：整除缩股
    /// 无碎股消灭）调 `record_share_cancellation`；两者在同一候选上变更。
    pub fn record_share_split_credit(
        &mut self,
        company: &CompanyId,
        event_id: &str,
        settled_on: CivilDate,
        issued_shares_before: u64,
        issued_shares_after: u64,
    ) -> Result<bool, CompanySystemError> {
        let already_credited = self.finance_mut(company)?.record_share_split_credit(
            event_id,
            settled_on,
            issued_shares_before,
            issued_shares_after,
        )?;
        if !already_credited {
            if issued_shares_after > issued_shares_before {
                self.issuers.record_share_issuance(
                    company,
                    issued_shares_after - issued_shares_before,
                )?;
            } else if issued_shares_after < issued_shares_before {
                self.issuers.record_share_cancellation(
                    company,
                    issued_shares_before - issued_shares_after,
                )?;
            }
        }
        Ok(already_credited)
    }

    /// 校验 Session 拆股／缩股账簿与 Simple 账面事实的跨域对应。
    pub fn validate_share_split_books(
        &self,
        books: &[super::share_split::ShareSplitBook],
    ) -> Result<(), CompanySystemError> {
        use std::collections::BTreeSet;
        let mut seen_events = BTreeSet::new();
        for book in books {
            let plan = book.plan();
            if !seen_events.insert(plan.event_id.clone()) {
                return Err(CompanySystemError::SystemState(format!(
                    "拆股／缩股事件 {} 重复出现账簿",
                    plan.event_id
                )));
            }
            let issuer = self.issuers.get(&plan.issuer).ok_or_else(|| {
                CompanySystemError::SystemState(format!(
                    "拆股／缩股账簿引用未知发行人 {}",
                    plan.issuer.0
                ))
            })?;
            if issuer.listed_stock.as_ref() != Some(&plan.stock) {
                return Err(CompanySystemError::SystemState(format!(
                    "拆股／缩股事件 {} 的证券与发行人不匹配",
                    plan.event_id
                )));
            }
            let fact = self
                .share_split_facts(&plan.issuer)?
                .into_iter()
                .find(|fact| fact.event_id == plan.event_id)
                .ok_or_else(|| {
                    CompanySystemError::SystemState(format!(
                        "拆股／缩股事件 {} 缺少 Simple 声明事实",
                        plan.event_id
                    ))
                })?;
            if fact.approved_on != plan.approved_on
                || fact.approval_reference != plan.approval_reference
                || fact.direction != plan.direction
                || fact.ratio != plan.ratio
            {
                return Err(CompanySystemError::SystemState(format!(
                    "拆股／缩股事件 {} 与 Simple 声明的日期、引用、方向或比例不一致",
                    plan.event_id
                )));
            }
            match (book.settled_on(), fact.settled_on) {
                (Some(book_date), Some(fact_date)) if book_date == fact_date => {}
                (None, None) => {}
                _ => {
                    return Err(CompanySystemError::SystemState(format!(
                        "拆股／缩股事件 {} 的入账事实与 Simple 账面回填不一致",
                        plan.event_id
                    )));
                }
            }
        }
        // 反向勾稽：每份账面拆股／缩股声明必须绑定账簿。
        let mut bound_events = BTreeSet::new();
        for book in books {
            bound_events.insert(book.plan().event_id.clone());
        }
        for (company, _) in self.issuers.iter() {
            let Ok(finance) = self.finance(company) else {
                continue;
            };
            for fact in finance.share_split_facts()? {
                if !bound_events.contains(&fact.event_id) {
                    return Err(CompanySystemError::SystemState(format!(
                        "公司 {} 存在未绑定账簿的拆股／缩股声明 {}",
                        company.0, fact.event_id
                    )));
                }
            }
        }
        Ok(())
    }

    /// 校验 Session 送转账簿与 Simple 账面事实的跨域对应。
    pub fn validate_stock_distribution_books(
        &self,
        books: &[super::stock_distribution::StockDistributionBook],
    ) -> Result<(), CompanySystemError> {
        use std::collections::{BTreeMap, BTreeSet};
        let mut seen_events = BTreeSet::new();
        for book in books {
            let plan = book.plan();
            if !seen_events.insert(plan.event_id.clone()) {
                return Err(CompanySystemError::SystemState(format!(
                    "送转事件 {} 重复出现账簿",
                    plan.event_id
                )));
            }
            let issuer = self
                .issuers
                .get(&plan.issuer)
                .ok_or_else(|| {
                    CompanySystemError::SystemState(format!(
                        "送转账簿引用未知发行人 {}",
                        plan.issuer.0
                    ))
                })?;
            if issuer.listed_stock.as_ref() != Some(&plan.stock) {
                return Err(CompanySystemError::SystemState(format!(
                    "送转事件 {} 的证券与发行人不匹配",
                    plan.event_id
                )));
            }
            let fact = self
                .stock_distribution_facts(&plan.issuer)?
                .into_iter()
                .find(|fact| fact.event_id == plan.event_id)
                .ok_or_else(|| {
                    CompanySystemError::SystemState(format!(
                        "送转事件 {} 缺少 Simple 声明事实",
                        plan.event_id
                    ))
                })?;
            if fact.approved_on != plan.approved_on
                || fact.approval_reference != plan.approval_reference
                || fact.kind != plan.kind
                || fact.new_shares != plan.approved_total_new_shares
            {
                return Err(CompanySystemError::SystemState(format!(
                    "送转事件 {} 与 Simple 声明的日期、引用、类别或股数不一致",
                    plan.event_id
                )));
            }
            match (book.credited_on(), fact.credited_on) {
                (Some(book_date), Some(fact_date)) if book_date == fact_date => {}
                (None, None) => {}
                _ => {
                    return Err(CompanySystemError::SystemState(format!(
                        "送转事件 {} 的入账事实与 Simple 账面回填不一致",
                        plan.event_id
                    )));
                }
            }
        }
        let mut declared = BTreeMap::new();
        for (company, _) in self.issuers.iter() {
            // 仅对「该公司确无 finance 状态」的合法缺省走过滤；读取送转事实的
            // 其他错误显式传播，不静默吞掉。
            let Ok(finance) = self.finance(company) else {
                continue;
            };
            let facts = finance.stock_distribution_facts()?;
            declared.insert(company.clone(), facts);
        }
        let mut bound_events = BTreeSet::new();
        for book in books {
            bound_events.insert(book.plan().event_id.clone());
        }
        for (company, facts) in declared {
            for fact in facts {
                if !bound_events.contains(&fact.event_id) {
                    return Err(CompanySystemError::SystemState(format!(
                        "公司 {} 存在未绑定账簿的送转声明 {}",
                        company.0, fact.event_id
                    )));
                }
            }
        }
        Ok(())
    }

    pub fn dividend_payment_facts(
        &self,
        company: &CompanyId,
        plan_id: &str,
    ) -> Result<Option<Vec<super::dividend::DividendPaymentFact>>, CompanySystemError> {
        Ok(self.finance(company)?.dividend_payment_facts(plan_id))
    }

    pub fn rights_offering_facts(
        &self,
        company: &CompanyId,
    ) -> Result<Vec<super::rights_offering::RightsOfferingFinanceFact>, CompanySystemError>
    {
        Ok(self.finance(company)?.rights_offering_facts()?)
    }

    /// 冻结 Simple 账面配股／增发声明事实；不产生公司或投资者现金。
    pub fn declare_rights_offering(
        &mut self,
        company: &CompanyId,
        declaration: super::rights_offering::RightsOfferingDeclaration,
    ) -> Result<bool, CompanySystemError> {
        let already_declared = self
            .finance_mut(company)?
            .declare_rights_offering(declaration)?;
        Ok(already_declared)
    }

    /// 结算回填账面事实并同步发行人已发行股数（成功路径）。
    pub fn record_rights_offering_settlement(
        &mut self,
        company: &CompanyId,
        event_id: &str,
        settled_on: CivilDate,
        issued_shares: u64,
        proceeds: crate::accounting::AccountingAmount,
    ) -> Result<bool, CompanySystemError> {
        let already_settled = self.finance_mut(company)?.record_rights_offering_settlement(
            event_id,
            settled_on,
            issued_shares,
            proceeds,
        )?;
        if !already_settled && issued_shares > 0 {
            self.issuers.record_share_issuance(company, issued_shares)?;
        }
        Ok(already_settled)
    }

    pub fn issuer_repurchase_facts(
        &self,
        company: &CompanyId,
    ) -> Result<Vec<super::issuer_repurchase::IssuerRepurchaseFinanceFact>, CompanySystemError>
    {
        Ok(self.finance(company)?.issuer_repurchase_facts()?)
    }

    /// 冻结回购方案的账面合成资金来源事实（ADR-0038）。
    pub fn declare_issuer_repurchase(
        &mut self,
        company: &CompanyId,
        fact: super::issuer_repurchase::IssuerRepurchaseFinanceFact,
    ) -> Result<bool, CompanySystemError> {
        let already = self
            .finance_mut(company)?
            .declare_issuer_repurchase(fact)?;
        Ok(already)
    }

    /// 回购计划完成回填（实际支出与回收差额）。
    pub fn record_issuer_repurchase_completion(
        &mut self,
        company: &CompanyId,
        event_id: &str,
        completed_on: CivilDate,
        spent: crate::accounting::AccountingAmount,
        withdrawn_remainder: crate::accounting::AccountingAmount,
    ) -> Result<bool, CompanySystemError> {
        let already = self.finance_mut(company)?.record_issuer_repurchase_completion(
            event_id,
            completed_on,
            spent,
            withdrawn_remainder,
        )?;
        Ok(already)
    }

    /// 回购注销回填：核减注册资本并同步发行人已发行股数。
    pub fn record_issuer_repurchase_cancellation(
        &mut self,
        company: &CompanyId,
        event_id: &str,
        cancelled_on: CivilDate,
        cancelled_shares: u64,
        issued_shares_before: u64,
    ) -> Result<bool, CompanySystemError> {
        let already = self.finance_mut(company)?.record_issuer_repurchase_cancellation(
            event_id,
            cancelled_on,
            cancelled_shares,
            issued_shares_before,
        )?;
        if !already {
            self.issuers
                .record_share_cancellation(company, cancelled_shares)?;
        }
        Ok(already)
    }

    /// 校验 Session 回购账簿与 Simple 账面事实的跨域对应。
    pub fn validate_issuer_repurchase_books(
        &self,
        books: &[super::issuer_repurchase::IssuerRepurchaseBook],
    ) -> Result<(), CompanySystemError> {
        use std::collections::BTreeSet;
        let mut seen_events = BTreeSet::new();
        for book in books {
            let plan = book.plan();
            if !seen_events.insert(plan.event_id.clone()) {
                return Err(CompanySystemError::SystemState(format!(
                    "回购方案 {} 重复出现账簿",
                    plan.event_id
                )));
            }
            let issuer = self.issuers.get(&plan.issuer).ok_or_else(|| {
                CompanySystemError::InvalidInput(format!(
                    "回购账簿引用未知发行人 {}",
                    plan.issuer.0
                ))
            })?;
            if issuer.listed_stock.as_ref() != Some(&plan.stock) {
                return Err(CompanySystemError::SystemState(format!(
                    "回购方案 {} 的证券与发行人不匹配",
                    plan.event_id
                )));
            }
            let fact = self
                .issuer_repurchase_facts(&plan.issuer)?
                .into_iter()
                .find(|fact| fact.event_id == plan.event_id)
                .ok_or_else(|| {
                    CompanySystemError::SystemState(format!(
                        "回购方案 {} 缺少 Simple 声明事实",
                        plan.event_id
                    ))
                })?;
            if fact.approved_on != plan.approved_on
                || fact.approval_reference != plan.approval_reference
                || fact.synthetic_funding
                    != crate::accounting::AccountingAmount::from_money(plan.total_budget)
            {
                return Err(CompanySystemError::SystemState(format!(
                    "回购方案 {} 与 Simple 声明的日期、引用或额度不一致",
                    plan.event_id
                )));
            }
            match (book.completed_on(), fact.completed_on, fact.spent, fact.withdrawn_remainder)
            {
                (None, None, None, None) => {}
                (
                    Some(book_date),
                    Some(fact_date),
                    Some(spent),
                    Some(withdrawn),
                ) if book_date == fact_date
                    && spent
                        == crate::accounting::AccountingAmount::from_money(
                            book.total_spent()
                                .map_err(|error| {
                                    CompanySystemError::Invalid(error.to_string())
                                })?,
                        )
                    && withdrawn
                        == crate::accounting::AccountingAmount::from_money(
                            book.withdrawn_remainder().unwrap_or(crate::money::Money::ZERO),
                        ) => {}
                _ => {
                    return Err(CompanySystemError::SystemState(format!(
                        "回购方案 {} 的完成事实与 Simple 账面回填不一致",
                        plan.event_id
                    )));
                }
            }
            match (book.cancelled_on(), fact.cancelled_on) {
                (None, None) => {}
                (Some(book_date), Some(fact_date)) if book_date == fact_date => {
                    if book.cancelled_shares() != fact.cancelled_shares {
                        return Err(CompanySystemError::SystemState(format!(
                            "回购方案 {} 的注销股数与 Simple 账面回填不一致",
                            plan.event_id
                        )));
                    }
                }
                _ => {
                    return Err(CompanySystemError::SystemState(format!(
                        "回购方案 {} 的注销事实与 Simple 账面回填不一致",
                        plan.event_id
                    )));
                }
            }
        }
        let mut bound_events = BTreeSet::new();
        for book in books {
            bound_events.insert(book.plan().event_id.clone());
        }
        for (company, _) in self.issuers.iter() {
            let Ok(finance) = self.finance(company) else {
                continue;
            };
            for fact in finance.issuer_repurchase_facts()? {
                if !bound_events.contains(&fact.event_id) {
                    return Err(CompanySystemError::SystemState(format!(
                        "公司 {} 存在未绑定账簿的回购声明 {}",
                        company.0, fact.event_id
                    )));
                }
            }
        }
        Ok(())
    }

    /// 校验 Session 配股／增发账簿与 Simple 账面事实的跨域对应。
    pub fn validate_rights_offering_books(
        &self,
        books: &[super::rights_offering::RightsOfferingBook],
    ) -> Result<(), CompanySystemError> {
        use std::collections::BTreeSet;
        let mut seen_events = BTreeSet::new();
        for book in books {
            let plan = book.plan();
            if !seen_events.insert(plan.event_id.clone()) {
                return Err(CompanySystemError::SystemState(format!(
                    "配股事件 {} 重复出现账簿",
                    plan.event_id
                )));
            }
            let issuer = self.issuers.get(&plan.issuer).ok_or_else(|| {
                CompanySystemError::InvalidInput(format!(
                    "配股账簿引用未知发行人 {}",
                    plan.issuer.0
                ))
            })?;
            if issuer.listed_stock.as_ref() != Some(&plan.stock) {
                return Err(CompanySystemError::SystemState(format!(
                    "配股事件 {} 的证券与发行人不匹配",
                    plan.event_id
                )));
            }
            let fact = self
                .rights_offering_facts(&plan.issuer)?
                .into_iter()
                .find(|fact| fact.event_id == plan.event_id)
                .ok_or_else(|| {
                    CompanySystemError::SystemState(format!(
                        "配股事件 {} 缺少 Simple 声明事实",
                        plan.event_id
                    ))
                })?;
            if fact.approved_on != plan.approved_on
                || fact.approval_reference != plan.approval_reference
                || fact.price_per_share != plan.price_per_share
            {
                return Err(CompanySystemError::SystemState(format!(
                    "配股事件 {} 与 Simple 声明的日期、引用或价格不一致",
                    plan.event_id
                )));
            }
            let book_settled = book.settlement().map(|settlement| {
                (
                    settlement.settlement_on,
                    settlement.total_paid_shares,
                    settlement.total_paid_amount,
                    settlement.failed,
                )
            });
            let fact_settled = fact.settled_on.map(|settled_on| {
                (
                    settled_on,
                    fact.issued_shares,
                    fact
                        .proceeds
                        .unwrap_or(crate::accounting::AccountingAmount::from_cents(0)),
                    fact.issued_shares == 0,
                )
            });
            match (book_settled, fact_settled) {
                (None, None) => {}
                (Some((book_date, book_shares, book_amount, failed)), Some((fact_date, fact_shares, fact_proceeds, fact_failed)))
                    if book_date == fact_date
                        && book_shares == fact_shares
                        && !failed
                        && !fact_failed
                        && crate::accounting::AccountingAmount::from_money(book_amount)
                            == fact_proceeds => {}
                // 失败路径：账簿保留「已缴款后全额退款」事实，账面按零发行登记。
                (
                    Some((book_date, _book_shares, _book_amount, true)),
                    Some((fact_date, 0, _fact_proceeds, true)),
                ) if book_date == fact_date => {}
                _ => {
                    return Err(CompanySystemError::SystemState(format!(
                        "配股事件 {} 的结算事实与 Simple 账面回填不一致",
                        plan.event_id
                    )));
                }
            }
        }
        let mut bound_events = BTreeSet::new();
        for book in books {
            bound_events.insert(book.plan().event_id.clone());
        }
        for (company, _) in self.issuers.iter() {
            // 仅对「该公司确无 finance 状态」的合法缺省走过滤；读取配股事实的
            // 其他错误显式传播，不静默吞掉。
            let Ok(finance) = self.finance(company) else {
                continue;
            };
            for fact in finance.rights_offering_facts()? {
                if !bound_events.contains(&fact.event_id) {
                    return Err(CompanySystemError::SystemState(format!(
                        "公司 {} 存在未绑定账簿的配股声明 {}",
                        company.0, fact.event_id
                    )));
                }
            }
        }
        Ok(())
    }

    pub fn validate_cash_dividend_books(
        &self,
        books: &[super::cash_dividend::CashDividendBook],
    ) -> Result<(), CompanySystemError> {
        use std::collections::{BTreeMap, BTreeSet};
        let mut seen = BTreeSet::new();
        if books
            .iter()
            .any(|book| self.issuers.get(&book.plan().issuer).is_none())
        {
            return Err(CompanySystemError::InvalidInput(
                "现金分红账簿引用未知发行人".into(),
            ));
        }
        for (company, _) in self.issuers.iter() {
            let plans = self.dividend_plan_facts(company)?;
            let plan_ids = plans
                .iter()
                .map(|plan| plan.plan_id.clone())
                .collect::<BTreeSet<_>>();
            let issuer_books = books
                .iter()
                .filter(|book| &book.plan().issuer == company)
                .collect::<Vec<_>>();
            let by_plan = issuer_books
                .iter()
                .map(|book| (book.plan().plan_id.as_str(), *book))
                .collect::<BTreeMap<_, _>>();
            if by_plan.len() != issuer_books.len() {
                return Err(CompanySystemError::SystemState(format!(
                    "公司 {} 存在重复现金分红计划",
                    company.0
                )));
            }
            for plan in plans {
                let book = by_plan.get(plan.plan_id.as_str()).ok_or_else(|| {
                    CompanySystemError::SystemState(format!(
                        "Simple 分红计划 {} 缺少对应现金分红账簿",
                        plan.plan_id
                    ))
                })?;
                let cash_plan = book.plan();
                let cash_limit =
                    crate::accounting::AccountingAmount::from_money(cash_plan.distributable_amount);
                let registered_gross = if book.registration().is_some() {
                    Some(crate::accounting::AccountingAmount::from_money(
                        book.total_gross()
                            .map_err(|error| CompanySystemError::Invalid(error.to_string()))?,
                    ))
                } else {
                    None
                };
                if cash_plan.approved_on != plan.approved_on
                    || plan.total_gross > cash_limit
                    || registered_gross.is_some_and(|gross| gross != plan.total_gross)
                    || plan.registered_capital_source_evidence.trim().is_empty()
                    || cash_plan.issuer != *company
                {
                    return Err(CompanySystemError::SystemState(format!(
                        "现金分红计划 {} 与 Simple 批准金额、日期或法律事实来源不一致",
                        plan.plan_id
                    )));
                }
                let finance_payments = plan
                    .payments
                    .iter()
                    .map(|payment| (payment.payment_id.as_str(), payment))
                    .collect::<BTreeMap<_, _>>();
                if finance_payments.len() != plan.payments.len() {
                    return Err(CompanySystemError::SystemState(format!(
                        "Simple 分红计划 {} 存在重复付款批次",
                        plan.plan_id
                    )));
                }
                for receipt in book.payments() {
                    let successful = receipt
                        .outcomes()
                        .iter()
                        .try_fold(crate::money::Money::ZERO, |total, outcome| match outcome {
                            super::cash_dividend::HolderPaymentOutcome::Paid { amount, .. } => {
                                total.add(*amount)
                            }
                            super::cash_dividend::HolderPaymentOutcome::Failed { .. } => Ok(total),
                        })
                        .map_err(|error| CompanySystemError::Invalid(error.to_string()))?;
                    let Some(payment) = finance_payments.get(receipt.payment_id()) else {
                        if successful.cents() == 0 {
                            continue;
                        }
                        return Err(CompanySystemError::SystemState(format!(
                            "成功现金到账批次 {} 缺少 Simple 付款凭证",
                            receipt.payment_id()
                        )));
                    };
                    if payment.paid_on != receipt.paid_on()
                        || crate::accounting::AccountingAmount::from_money(successful)
                            != payment.amount
                    {
                        return Err(CompanySystemError::SystemState(format!(
                            "付款批次 {} 的 Simple 账簿与持有人到账不一致",
                            receipt.payment_id()
                        )));
                    }
                    seen.insert((
                        company.clone(),
                        plan.plan_id.clone(),
                        payment.payment_id.clone(),
                    ));
                }
                for payment in plan.payments {
                    if !seen.contains(&(
                        company.clone(),
                        plan.plan_id.clone(),
                        payment.payment_id.clone(),
                    )) {
                        return Err(CompanySystemError::SystemState(format!(
                            "Simple 付款批次 {} 缺少匹配的持有人到账回执",
                            payment.payment_id
                        )));
                    }
                }
            }
            if by_plan.keys().any(|plan_id| !plan_ids.contains(*plan_id)) {
                return Err(CompanySystemError::SystemState(format!(
                    "公司 {} 存在未绑定 Simple 声明的现金分红计划",
                    company.0
                )));
            }
        }
        Ok(())
    }

    pub fn create(
        issuers: Vec<CompanySpec>,
        config: CompanySystemConfig,
        start_date: CivilDate,
        seed: u64,
    ) -> Result<Self, CompanySystemError> {
        let issuers = IssuerRegistry::new(issuers)?;
        let implementation = match config {
            CompanySystemConfig::Simple(config) => CompanyImplementation::Simple(
                SimpleFundamentals::create(&issuers, config, start_date, seed)?,
            ),
            CompanySystemConfig::Simulation => {
                return Err(CompanySystemError::Unsupported(
                    "Simulation 在独立分支实现；当前不能创建".into(),
                ));
            }
        };
        Ok(Self {
            issuers,
            implementation,
            hash_cache: CompanySystemHashCache::default(),
        })
    }
    pub fn issuers(&self) -> &IssuerRegistry {
        &self.issuers
    }
    pub fn advanced_through(&self) -> CivilDate {
        match &self.implementation {
            CompanyImplementation::Simple(state) => state.advanced_through,
        }
    }
    /// 该日期推进是否恰为一个结算周期的完成（周期末日结算）。偏好提案只在
    /// 这种日期的日结候选事务内评估（ADR-0037），本方法即共同层的最小评估钩子。
    pub fn settlement_completed_on(&self, date: CivilDate) -> Result<bool, CompanySystemError> {
        match &self.implementation {
            CompanyImplementation::Simple(state) => {
                Ok(state.config.settlement_cycle.containing(date)?.1 == date)
            }
        }
    }
    /// 按公司读取 Simple 行为偏好（ADR-0037；模型内部配置，共同层只透出读取）。
    /// 未配置的公司返回空偏好（两项皆 `None`，不自动产生方案）。
    pub fn simple_preferences(
        &self,
        company: &CompanyId,
    ) -> Result<super::simple::preferences::SimpleCompanyPreferences, CompanySystemError> {
        match &self.implementation {
            CompanyImplementation::Simple(state) => state
                .config
                .companies
                .iter()
                .find(|config| &config.company == company)
                .map(|config| config.preferences.clone())
                .ok_or_else(|| CompanySystemError::InvalidInput(format!("未知公司 {}", company.0))),
        }
    }
    /// 如实记录一笔被制度拒绝的偏好提案；同键同因幂等，同键异因显式报错。
    pub fn record_preference_rejection(
        &mut self,
        company: &CompanyId,
        kind: super::simple::preferences::SimplePreferenceProposalKind,
        evaluated_on: CivilDate,
        detail: String,
    ) -> Result<(), CompanySystemError> {
        let result = (|| {
            let state = match &mut self.implementation {
                CompanyImplementation::Simple(state) => state,
            };
            let entry = state
                .companies
                .get_mut(company)
                .ok_or_else(|| CompanySystemError::InvalidInput(format!("未知公司 {}", company.0)))?;
            entry
                .preference_ledger
                .record_rejection(company, kind, evaluated_on, detail)
        })();
        if result.is_ok() {
            self.hash_cache = CompanySystemHashCache::default();
        }
        result
    }
    /// 该公司该类别最近一次偏好提案被拒的评估日（台账的最小只读投影）。
    pub fn last_preference_rejection_on(
        &self,
        company: &CompanyId,
        kind: super::simple::preferences::SimplePreferenceProposalKind,
    ) -> Result<Option<CivilDate>, CompanySystemError> {
        match &self.implementation {
            CompanyImplementation::Simple(state) => state
                .companies
                .get(company)
                .map(|entry| entry.preference_ledger.last_rejection_on(company, kind))
                .ok_or_else(|| CompanySystemError::InvalidInput(format!("未知公司 {}", company.0))),
        }
    }
    /// 该公司的偏好提案拒绝台账（只读；宿主/UI 呈现由后续批次接线）。
    pub fn preference_rejections(
        &self,
        company: &CompanyId,
    ) -> Result<&[super::simple::preferences::SimplePreferenceRejection], CompanySystemError> {
        match &self.implementation {
            CompanyImplementation::Simple(state) => state
                .companies
                .get(company)
                .map(|entry| entry.preference_ledger.rejections.as_slice())
                .ok_or_else(|| CompanySystemError::InvalidInput(format!("未知公司 {}", company.0))),
        }
    }
    pub fn config(&self) -> CompanySystemConfig {
        match &self.implementation {
            CompanyImplementation::Simple(state) => {
                CompanySystemConfig::Simple(state.config.clone())
            }
        }
    }
    pub fn advance_day(
        &mut self,
        date: CivilDate,
    ) -> Result<Vec<SimpleDisclosureCandidate>, CompanySystemError> {
        let result = match &mut self.implementation {
            CompanyImplementation::Simple(state) => state.advance_day(date),
        };
        if result.is_ok() {
            self.hash_cache = CompanySystemHashCache::default();
        }
        result
    }
    pub(crate) fn history(&self) -> &[SimpleDisclosureCandidate] {
        match &self.implementation {
            CompanyImplementation::Simple(state) => &state.history,
        }
    }
    pub(crate) fn finance(
        &self,
        company: &CompanyId,
    ) -> Result<&super::simple::SimpleFinanceState, CompanySystemError> {
        match &self.implementation {
            CompanyImplementation::Simple(state) => state
                .companies
                .get(company)
                .map(|company| &company.finance)
                .ok_or_else(|| CompanySystemError::InvalidInput(format!("未知公司 {}", company.0))),
        }
    }
    pub(crate) fn finance_mut(
        &mut self,
        company: &CompanyId,
    ) -> Result<&mut super::simple::SimpleFinanceState, CompanySystemError> {
        self.hash_cache = CompanySystemHashCache::default();
        match &mut self.implementation {
            CompanyImplementation::Simple(state) => state
                .companies
                .get_mut(company)
                .map(|company| &mut company.finance)
                .ok_or_else(|| CompanySystemError::InvalidInput(format!("未知公司 {}", company.0))),
        }
    }
    pub(crate) fn closed_reports(
        &self,
        company: &CompanyId,
    ) -> Result<Vec<crate::accounting::reports::ReportSet>, CompanySystemError> {
        let finance = self.finance(company)?;
        Ok(finance.closing().latest_reports_for_scope(
            &crate::accounting::consolidation::ScopeId::Standalone(finance.member_id()),
        ))
    }
    pub(crate) fn closing_for(
        &self,
        company: &CompanyId,
    ) -> Result<&crate::accounting::closing::ClosingEngine, CompanySystemError> {
        Ok(self.finance(company)?.closing())
    }
    pub(crate) fn accounting_policy_for(
        &self,
        company: &CompanyId,
    ) -> Result<crate::information::AccountingPolicyRef, CompanySystemError> {
        Ok(crate::information::AccountingPolicyRef {
            chart_version: self.finance(company)?.books().ledger().chart().version(),
        })
    }
    pub(crate) fn report_availability(
        &self,
        company: &CompanyId,
        period: crate::accounting::AccountingPeriod,
        kind: crate::accounting::reports::ReportKind,
    ) -> Result<CompanyReportAvailability, CompanySystemError> {
        let finance = self.finance(company)?;
        let scope = crate::accounting::consolidation::ScopeId::Standalone(finance.member_id());
        if !finance.closing().versions(&scope, period, kind).is_empty() {
            return Ok(CompanyReportAvailability::Available);
        }
        let ((first, last), _) = kind
            .resolve(period)
            .map_err(|error| CompanySystemError::Invalid(error.to_string()))?;
        let first_date = CivilDate::from_ymd(first.year(), first.month(), 1)?;
        let end_date = if last.month() == 12 {
            CivilDate::from_ymd(last.year(), 12, 31)?
        } else {
            CivilDate::from_ymd(last.year(), last.month() + 1, 1)?.prev()?
        };
        if first_date <= finance.opening_date() {
            Ok(CompanyReportAvailability::BeforeOpening)
        } else if end_date > finance.as_of() {
            Ok(CompanyReportAvailability::NotYetSettled)
        } else {
            Ok(CompanyReportAvailability::PeriodNotRepresented)
        }
    }
    /// 公司侧当前事实（共同契约能力面的公司部分；方案/条件/本人权利由
    /// session 层补充组装成完整 `CompanyCapabilities`）。不可用的字段显式给
    /// reason，不填零（铁律三）。
    pub fn company_facts(
        &self,
        company: &CompanyId,
    ) -> Result<super::capabilities::CompanyFacts, CompanySystemError> {
        use super::capabilities::{
            CapabilityAmount, CapabilityMoney, CompanyFacts, DistributableProfitSnapshot,
        };
        let finance = self.finance(company)?;
        let issued_shares = self
            .issuers
            .get(company)
            .map(|spec| spec.issued_shares)
            .ok_or_else(|| {
                CompanySystemError::SystemState(format!(
                    "发行人登记表缺失公司 {}，与财务状态不一致",
                    company.0
                ))
            })?;
        let par_value_per_share = match finance.current_par_value()? {
            Some(par) => CapabilityMoney::Available {
                cents: par.cents().to_string(),
            },
            None => CapabilityMoney::Unavailable {
                reason: "尚无送转／拆股绑定的每股面值事实，面值不可用".into(),
            },
        };
        let registered_capital = match finance.legal_facts() {
            Some(facts) if facts.registered_capital.is_positive() => {
                CapabilityAmount::Available {
                    amount_yuan: facts.registered_capital.to_yuan_string(),
                }
            }
            Some(_) => CapabilityAmount::Unavailable {
                reason: "注册资本法定事实为非正数，不可用".into(),
            },
            None => CapabilityAmount::Unavailable {
                reason: "未绑定注册资本法定事实（define_dividend_legal_facts）".into(),
            },
        };
        let distributable_profit = match finance.distributable_profit() {
            Ok(profit) => DistributableProfitSnapshot::Available {
                accumulated_after_loss_yuan: profit.accumulated_after_loss.to_yuan_string(),
                statutory_reserve_yuan: profit.statutory_reserve.to_yuan_string(),
                available_for_distribution_yuan: profit.available_for_distribution.to_yuan_string(),
                reserve_basis_year: profit.reserve_basis_year,
            },
            Err(super::simple::SimpleFinanceError::DividendUnsupported(reason)) => {
                DistributableProfitSnapshot::Unavailable { reason }
            }
            Err(other) => return Err(other.into()),
        };
        Ok(CompanyFacts {
            revenue: true,
            net_income: true,
            equity: true,
            cash_flow: true,
            full_financial_statements: true,
            cash_settlement: false,
            unsupported_reason:
                "汇总财务已接通；共同股本行为的实际投资者结算仍待接线，不能按模式缺资金拒绝或冒称已支持"
                    .into(),
            par_value_per_share,
            issued_shares,
            registered_capital,
            distributable_profit,
        })
    }

    /// 按公司+期间读取期间变化解释（复用既有内部 history 的 explanation 数据，
    /// 只读、不新建状态；上层读取入口见 Q14 §3 `query_explanation` 最小落点）。
    pub fn period_change_explanation(
        &self,
        company: &CompanyId,
        period_end: CivilDate,
    ) -> Result<super::simple::period::PeriodChangeExplanation, CompanySystemError> {
        let state = match &self.implementation {
            CompanyImplementation::Simple(state) => state,
        };
        if !state.companies.contains_key(company) {
            return Err(CompanySystemError::InvalidInput(format!(
                "未知公司 {}",
                company.0
            )));
        }
        let (_, cycle_end) = state.config.settlement_cycle.containing(period_end)?;
        if cycle_end != period_end {
            return Err(CompanySystemError::InvalidInput(format!(
                "{period_end} 不是结算周期末日，无法定位期间解释"
            )));
        }
        let advanced = state.advanced_through;
        let (current_start, current_end) = state.config.settlement_cycle.containing(advanced)?;
        let latest_settled = if advanced == current_end {
            current_end
        } else {
            current_start.prev()?
        };
        if period_end > latest_settled {
            return Err(CompanySystemError::BusinessCondition(format!(
                "期间 {period_end} 尚未结算，暂无解释材料（最近已结算期间末日 {latest_settled}）"
            )));
        }
        if period_end < state.history_start {
            return Err(CompanySystemError::BusinessCondition(format!(
                "期间 {period_end} 早于开局前史起点 {}，无解释材料",
                state.history_start
            )));
        }
        if let Some(entry) = state.history.iter().find(|entry| {
            &entry.company == company && entry.period_end == period_end
        }) {
            return Ok(entry.explanation.clone());
        }
        Err(CompanySystemError::SystemState(format!(
            "公司 {} 的期间 {period_end} 在已结算范围内但历史缺失解释",
            company.0
        )))
    }
    pub fn submit_command(&mut self, command: CompanyCommand) -> Result<(), CompanySystemError> {
        let result = match &mut self.implementation {
            CompanyImplementation::Simple(state) => state.submit_command(command),
        };
        if result.is_ok() {
            self.hash_cache = CompanySystemHashCache::default();
        }
        result
    }

    pub(crate) fn hash_projection(&self) -> Result<CompanySystemHashProjection, serde_json::Error> {
        if let Some(projection) = self.hash_cache.0.get() {
            return Ok(*projection);
        }
        let bytes = serde_json::to_vec(self)?;
        let digest = bytes.iter().fold(0xcbf29ce484222325_u64, |digest, byte| {
            (digest ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        });
        let projection = CompanySystemHashProjection {
            serialized_len: bytes.len(),
            digest,
        };
        let _ = self.hash_cache.0.set(projection);
        Ok(projection)
    }
}
