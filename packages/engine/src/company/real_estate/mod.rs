//! 地产开发经营会计（K3，任务 11）。
//!
//! [`RealEstateBooks`] = 权威账套（[`Books`]，任务 6）+ 项目子账（成本轨迹
//! 与资本化窗口）+ 预售合同子账 + 项目借款子账（双余数链）+ 应收尾款开项
//! （任务 8 共享 `TradeOpenLedger`）+ 外部对手方 + 版本化资本化政策。
//! 与任务 8/9 `IndustrialBooks`/`BankBooks` 同一组合模式：**独立引擎**，
//! 不修改 `Company` 注册表壳；会话接线在任务 26。
//!
//! 事件处理不变量（每个处理器一致执行）：
//! 1. **validate → post → apply**：全部业务校验先于过账；过账走
//!    [`Books::post_batch`] 原子提交；子账变更仅在过账成功后落地——任何
//!    拒绝（含 `PaymentFailed`）账套与子账**字节不变**（测试逐一断言），
//!    事件 id 也不消耗。
//! 2. K3 红线：**预售不是交付收入**（收款进 2203 合同负债，CAS 14 §39
//!    已核验；收入只在交付=控制权转移时点确认，§4/§13）；**不无限资本化**
//!    （窗口终止/暂停强制生效，窗口外利息进 6603 损益——资本化政策是
//!    版本化游戏假设，CAS 17 原文取证受阻，不声称准则合规）。
//! 3. 现金流分类：购地/开发投入/预售收款/尾款回收 = 经营活动（开发存货
//!    是开发商品的原材料存货，非固定资产投资——CAS 31 归类选择，登记
//!    docs）；借款/付息/还本 = 筹资活动。
//! 4. 资金不足 → `PaymentFailed`（类型化，公司继续运行；K2 无透支无补钱）。
//!
//! [`Books::post_batch`]: crate::accounting::Books::post_batch

mod borrowing_costs;
mod chart;
mod config;
mod debt_service;
mod delivery;
mod development;
mod error;
mod impairment;
mod land;
mod loans;
mod presales;
mod projects;

pub use chart::real_estate_chart_v5;
pub use config::{CapitalizationPolicy, RealEstateConfig};
pub use delivery::DeliveryOutcome;
pub use error::RealEstateError;
pub use loans::{InterestSplitItem, ProjectLoanState};
pub use presales::PresaleContract;
pub use projects::{Interruption, ProjectId, ProjectState};

use std::collections::BTreeMap;

use crate::accounting::reports::real_estate::{
    real_estate_presentation_lines, RealEstatePresentationLines,
};
use crate::accounting::{
    AccountingAmount, AccountingError, Books, JournalLine, LedgerAccountId, TradeOpenLedger,
};
use crate::company::contracts::{ContractId, OperatingBudget};
use crate::company::counterparty::{
    CounterpartyFlow, CounterpartyId, CounterpartyLedger, FlowDirection,
};
use crate::company::opening::opening_event_id;

/// 地产账套：Books + 项目/预售/借款子账 + 应收尾款 + 对手方 + 政策
/// （全部随存档序列化；`Books` 恢复走重放路径，其余结构体 serde 直存）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct RealEstateBooks {
    books: Books,
    projects: BTreeMap<ProjectId, ProjectState>,
    presales: BTreeMap<ContractId, PresaleContract>,
    loans: BTreeMap<ContractId, ProjectLoanState>,
    receivables: TradeOpenLedger,
    counterparties: CounterpartyLedger,
    budget: OperatingBudget,
    capitalization_policy: CapitalizationPolicy,
    max_projects: usize,
    next_event_id: u64,
}

impl RealEstateBooks {
    /// 装配：政策/项目上限校验 → 开局行子账种子守卫 → 开局凭证过账
    /// （原子）→ 对手方登记。任一步失败 ⇒ 不产生半构造账套。
    pub fn new(config: RealEstateConfig) -> Result<Self, RealEstateError> {
        config.capitalization_policy.validate()?;
        if config.max_projects < 1 {
            return Err(RealEstateError::InvalidPolicy {
                detail: format!("max_projects must be >= 1, got {}", config.max_projects),
            });
        }
        config::check_opening_lines(&config.opening_lines)?;
        let mut books = Books::new(config.chart);
        books.post_batch(vec![crate::accounting::JournalEntry {
            source: opening_event_id(),
            date: config.as_of,
            kind: crate::accounting::BusinessKind::OpeningBalance,
            cash_flow: crate::accounting::CashFlowClass::Financing,
            lines: config.opening_lines,
        }])?;
        let mut counterparties = CounterpartyLedger::new();
        for counterparty in config.counterparties {
            counterparties.register(counterparty)?;
        }
        Ok(Self {
            books,
            projects: BTreeMap::new(),
            presales: BTreeMap::new(),
            loans: BTreeMap::new(),
            receivables: TradeOpenLedger::default(),
            counterparties,
            budget: config.budget,
            capitalization_policy: config.capitalization_policy,
            max_projects: config.max_projects,
            next_event_id: 2,
        })
    }

    // ===== 只读访问面（子账与状态）=====

    pub fn books(&self) -> &Books {
        &self.books
    }

    pub fn project(&self, id: &ProjectId) -> Option<&ProjectState> {
        self.projects.get(id)
    }

    pub fn projects(&self) -> &BTreeMap<ProjectId, ProjectState> {
        &self.projects
    }

    pub fn presale(&self, id: &ContractId) -> Option<&PresaleContract> {
        self.presales.get(id)
    }

    pub fn presales(&self) -> &BTreeMap<ContractId, PresaleContract> {
        &self.presales
    }

    pub fn loan(&self, id: &ContractId) -> Option<&ProjectLoanState> {
        self.loans.get(id)
    }

    pub fn loans(&self) -> impl Iterator<Item = (&ContractId, &ProjectLoanState)> {
        self.loans.iter()
    }

    pub fn receivables(&self) -> &TradeOpenLedger {
        &self.receivables
    }

    pub fn counterparties(&self) -> &CounterpartyLedger {
        &self.counterparties
    }

    pub fn budget(&self) -> &OperatingBudget {
        &self.budget
    }

    pub fn capitalization_policy(&self) -> &CapitalizationPolicy {
        &self.capitalization_policy
    }

    /// Σ项目结存成本（勾稽锚：应恒等于 1541 开发存货科目余额）。
    pub fn development_inventory_total(&self) -> Result<AccountingAmount, RealEstateError> {
        let mut total = AccountingAmount::ZERO;
        for state in self.projects.values() {
            total = total.add(state.remaining_cost())?;
        }
        Ok(total)
    }

    /// 报表分类层胶水：总账 → CAS 30 (2026) + CAS 14 地产列报行（任务 13 消费）。
    pub fn presentation_lines(&self) -> Result<RealEstatePresentationLines, AccountingError> {
        real_estate_presentation_lines(self.books.ledger())
    }

    // ===== 处理器共享内部 =====

    /// 原子过账 + 领域错误映射 + 事件 id 提交：批末负现金 → `PaymentFailed`。
    /// `next_event_id` 只在过账**成功**后推进——任何拒绝连事件 id 都不消耗。
    pub(super) fn post_with_commit(
        &mut self,
        next_event_id: u64,
        entries: Vec<crate::accounting::JournalEntry>,
    ) -> Result<(), RealEstateError> {
        self.books
            .post_batch(entries)
            .map_err(error::map_post_error)?;
        self.next_event_id = next_event_id;
        Ok(())
    }

    /// 对手方已登记（处理器前置校验；资金流跨边界留痕的 K2 要求）。
    pub(super) fn ensure_counterparty(&self, id: &CounterpartyId) -> Result<(), RealEstateError> {
        if self.counterparties.get(id).is_none() {
            return Err(RealEstateError::Company(
                crate::company::CompanyError::UnknownCounterparty {
                    counterparty: id.clone(),
                },
            ));
        }
        Ok(())
    }

    /// 记录跨边界资金流（过账成功后；对手方已在 validate 段校验登记）。
    pub(super) fn record_flow(
        &mut self,
        date: crate::calendar::CivilDate,
        counterparty: &CounterpartyId,
        direction: FlowDirection,
        amount: AccountingAmount,
        memo: &str,
    ) -> Result<(), RealEstateError> {
        self.counterparties.record_flow(CounterpartyFlow {
            date,
            counterparty: counterparty.clone(),
            direction,
            amount,
            memo: memo.to_string(),
        })?;
        Ok(())
    }

    pub(super) fn net_of(&self, code: &str) -> Result<AccountingAmount, RealEstateError> {
        Ok(self
            .books
            .ledger()
            .account_net_debit(&LedgerAccountId(code.to_string()))?)
    }

    /// Σ未偿借款本金（授信占用口径）。
    pub(super) fn loans_outstanding_total(&self) -> Result<AccountingAmount, RealEstateError> {
        let mut total = AccountingAmount::ZERO;
        for loan in self.loans.values() {
            total = total.add(loan.outstanding())?;
        }
        Ok(total)
    }

    pub(super) fn projects_mut(&mut self) -> &mut BTreeMap<ProjectId, ProjectState> {
        &mut self.projects
    }

    pub(super) fn presales_mut(&mut self) -> &mut BTreeMap<ContractId, PresaleContract> {
        &mut self.presales
    }

    pub(super) fn loans_map(&self) -> &BTreeMap<ContractId, ProjectLoanState> {
        &self.loans
    }

    pub(super) fn loans_map_mut(&mut self) -> &mut BTreeMap<ContractId, ProjectLoanState> {
        &mut self.loans
    }

    pub(super) fn receivables_mut(&mut self) -> &mut TradeOpenLedger {
        &mut self.receivables
    }
}

/// 快速构造分录行。
pub(super) fn line(
    code: &str,
    side: crate::accounting::PostingSide,
    amount: AccountingAmount,
) -> JournalLine {
    JournalLine {
        account: LedgerAccountId(code.to_string()),
        side,
        amount,
    }
}
