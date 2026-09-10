//! 工商经营与营运资金会计（K3，任务 8）。
//!
//! [`IndustrialBooks`] = 权威账套（[`Books`]，任务 6）+ 共享子账（存货/固定资产/
//! 应收应付开项，`accounting::{inventory,fixed_assets,receivables}`）+ 税务状态 +
//! 合同/对手方/授信（任务 7 类型面）。**独立引擎**：不修改 `Company`（注册表
//! 壳）——银行/保险/地产（任务 9–11）以同一模式组合共享子账；会话接线在任务 26。
//!
//! 事件处理不变量（每个处理器一致执行）：
//! 1. **validate → post → apply**：全部业务校验（子账预检 + 政策校验）先于过账；
//!    过账走 [`Books::post_batch`] 原子提交；子账变更仅在过账成功后落地——
//!    任何拒绝（含 `PaymentFailed`）账套与子账**字节不变**（测试逐一断言）。
//! 2. 现金/非现金严格分离：赊销不动现金；回款只清应收不重复计收入（CAS 14
//!    §4/§13 履约确认，官方依据已核验）；折旧/减值/坏账/计息为非现金。
//! 3. 利息 ACT/365F + 合同累计余数（`FractionUnits`，1/3_650_000 分单位）；
//!    付息/还本属筹资活动现金（CAS 31 口径）；资本开支属投资活动。
//! 4. 经营亏损是合法状态；资金不足 → `PaymentFailed`/逾期开项，不透支不补钱。
//!
//! [`Books::post_batch`]: crate::accounting::Books::post_batch

mod capex;
pub(crate) mod chart;
mod config;
mod error;
mod expenses;
mod interest;
mod loans;
mod production;
mod purchasing;
mod repayment;
mod sales;

pub use config::{IndustrialConfig, OpeningAssetItem, OpeningDebtTerms, OpeningInventoryItem};
pub use error::IndustrialError;
pub use expenses::{ExpenseKind, IncomeTaxOutcome};
pub use loans::{InterestAccrualItem, LoanState, RepaymentOutcome, OPENING_DEBT_CONTRACT_ID};
pub use purchasing::{PurchaseOutcome, Settlement};

pub use chart::industrial_chart_v2;

use std::collections::BTreeMap;

use crate::accounting::{
    AccountingAmount, Books, BusinessKind, CashFlowClass, FixedAssetRegister, InventoryLedger,
    JournalEntry, JournalLine, LedgerAccountId, LossEntry, PostingSide, TaxPolicy, TradeOpenLedger,
};
use crate::company::contracts::{ContractBook, ContractId, OperatingBudget};
use crate::company::counterparty::{
    CounterpartyFlow, CounterpartyId, CounterpartyLedger, FlowDirection,
};
use crate::company::opening::opening_event_id;
use error::map_post_error;

/// 工商账套：Books + 子账 + 税务/借款状态（全部随存档序列化；`Books` 恢复走
/// 重放路径，其余结构体 serde 直存）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct IndustrialBooks {
    books: Books,
    inventory: InventoryLedger,
    assets: FixedAssetRegister,
    receivables: TradeOpenLedger,
    payables: TradeOpenLedger,
    contracts: ContractBook,
    counterparties: CounterpartyLedger,
    budget: OperatingBudget,
    tax_policy: TaxPolicy,
    loss_pool: Vec<LossEntry>,
    loans: BTreeMap<ContractId, LoanState>,
    next_event_id: u64,
}

impl IndustrialBooks {
    /// 装配：税务政策校验 → 开局凭证过账（原子）→ 子账种子与总账逐科目对账
    /// （config.rs 守卫）→ 开局借款隐式合同登记（余额精确匹配）。任一步失败 ⇒
    /// 不产生半构造账套。
    pub fn new(config: IndustrialConfig) -> Result<Self, IndustrialError> {
        config.tax_policy.validate()?;
        let mut books = Books::new(config.chart);
        books.post_batch(vec![JournalEntry {
            source: opening_event_id(),
            date: config.as_of,
            kind: BusinessKind::OpeningBalance,
            cash_flow: CashFlowClass::Financing,
            lines: config.opening_lines,
        }])?;

        let inventory = config::seed_inventory(books.ledger(), &config.opening_inventory)?;
        let assets = config::seed_assets(books.ledger(), &config.opening_assets)?;

        let mut counterparties = CounterpartyLedger::new();
        for counterparty in config.counterparties {
            counterparties.register(counterparty)?;
        }

        let (contracts, loans) = match &config.opening_debt {
            Some(terms) => config::seed_opening_debt(
                books.ledger(),
                config.as_of,
                terms,
                counterparties.get(&terms.lender).is_some(),
            )?,
            None => (ContractBook::new(), BTreeMap::new()),
        };

        Ok(Self {
            books,
            inventory,
            assets,
            receivables: TradeOpenLedger::default(),
            payables: TradeOpenLedger::default(),
            contracts,
            counterparties,
            budget: config.budget,
            tax_policy: config.tax_policy,
            loss_pool: Vec::new(),
            loans,
            next_event_id: 2,
        })
    }

    // ===== 只读访问面（子账与状态）=====

    pub fn books(&self) -> &Books {
        &self.books
    }

    pub fn inventory(&self) -> &InventoryLedger {
        &self.inventory
    }

    pub fn assets(&self) -> &FixedAssetRegister {
        &self.assets
    }

    pub fn receivables(&self) -> &TradeOpenLedger {
        &self.receivables
    }

    pub fn payables(&self) -> &TradeOpenLedger {
        &self.payables
    }

    pub fn contracts(&self) -> &ContractBook {
        &self.contracts
    }

    pub fn counterparties(&self) -> &CounterpartyLedger {
        &self.counterparties
    }

    pub fn budget(&self) -> &OperatingBudget {
        &self.budget
    }

    pub fn tax_policy(&self) -> &TaxPolicy {
        &self.tax_policy
    }

    pub fn loss_pool(&self) -> &[LossEntry] {
        &self.loss_pool
    }

    pub fn loan(&self, contract: &ContractId) -> Option<&LoanState> {
        self.loans.get(contract)
    }

    pub fn loans(&self) -> impl Iterator<Item = (&ContractId, &LoanState)> {
        self.loans.iter()
    }

    /// 某贷款人剩余授信 = 限额 − Σ未偿本金（含开局隐式合同）；无授信 = None。
    pub fn available_credit(&self, lender: &CounterpartyId) -> Option<AccountingAmount> {
        let limit = self.budget.credit_line(lender)?;
        let outstanding = self
            .loans
            .values()
            .try_fold(AccountingAmount::ZERO, |acc, loan| {
                acc.add(loan.outstanding())
            })
            .ok()?;
        limit.sub(outstanding).ok()
    }

    // ===== 处理器共享内部 =====

    /// 原子过账 + 领域错误映射 + 事件 id 提交：批末负现金 → `PaymentFailed`
    /// （类型化，非崩溃）。`next_event_id` 只在过账**成功**后推进——任何拒绝
    /// （含 PaymentFailed）连事件 id 都不消耗，账套与子账字节不变。
    pub(super) fn post_with_commit(
        &mut self,
        next_event_id: u64,
        entries: Vec<JournalEntry>,
    ) -> Result<(), IndustrialError> {
        self.books.post_batch(entries).map_err(map_post_error)?;
        self.next_event_id = next_event_id;
        Ok(())
    }

    /// 科目净借方余额（处理器读派生态）。
    pub(super) fn net_of(&self, code: &str) -> Result<AccountingAmount, IndustrialError> {
        Ok(self
            .books
            .ledger()
            .account_net_debit(&LedgerAccountId(code.to_string()))?)
    }

    /// Σ未偿借款本金（含开局隐式合同；授信占用口径）。
    pub(super) fn loans_outstanding_total(&self) -> Result<AccountingAmount, IndustrialError> {
        let mut total = AccountingAmount::ZERO;
        for loan in self.loans.values() {
            total = total.add(loan.outstanding())?;
        }
        Ok(total)
    }

    pub(super) fn inventory_mut(&mut self) -> &mut InventoryLedger {
        &mut self.inventory
    }

    pub(super) fn receivables_mut(&mut self) -> &mut TradeOpenLedger {
        &mut self.receivables
    }

    pub(super) fn payables_mut(&mut self) -> &mut TradeOpenLedger {
        &mut self.payables
    }

    pub(super) fn counterparties_mut(&mut self) -> &mut CounterpartyLedger {
        &mut self.counterparties
    }

    pub(super) fn assets_mut(&mut self) -> &mut FixedAssetRegister {
        &mut self.assets
    }

    pub(super) fn contracts_mut(&mut self) -> &mut ContractBook {
        &mut self.contracts
    }

    pub(super) fn loans_mut(&mut self) -> &mut BTreeMap<ContractId, LoanState> {
        &mut self.loans
    }
}

/// 快速构造分录行。
pub(super) fn line(code: &str, side: PostingSide, amount: AccountingAmount) -> JournalLine {
    JournalLine {
        account: LedgerAccountId(code.to_string()),
        side,
        amount,
    }
}

/// 快速构造跨边界资金流记录（采购/回款/借款/付息/还本共用）。
pub(super) fn flow(
    date: crate::calendar::CivilDate,
    counterparty: &CounterpartyId,
    direction: FlowDirection,
    amount: AccountingAmount,
    memo: &str,
) -> CounterpartyFlow {
    CounterpartyFlow {
        date,
        counterparty: counterparty.clone(),
        direction,
        amount,
        memo: memo.to_string(),
    }
}
