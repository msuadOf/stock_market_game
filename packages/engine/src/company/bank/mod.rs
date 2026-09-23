//! 银行经营会计（K3，任务 9）。
//!
//! [`BankBooks`] = 权威账套（[`Books`]，任务 6）+ 银行子账（存款合同/贷款
//! 合同含 ECL 阶段跟踪）+ 外部客户对手方 + 版本化 ECL 政策。与任务 8
//! `IndustrialBooks` 同一组合模式：**独立引擎**，不修改 `Company` 注册表壳；
//! 会话接线在任务 26。
//!
//! 事件处理不变量（每个处理器一致执行）：
//! 1. **validate → post → apply**：全部业务校验（产品种类/合同条款/对手方/
//!    子账容量/ECL 情景）先于过账；过账走 [`Books::post_batch`] 原子提交；
//!    子账变更仅在过账成功后落地——任何拒绝（含 `PaymentFailed`）账套与
//!    子账**字节不变**（测试逐一断言），事件 id 也不消耗。
//! 2. K3 红线：存款是负债（2011/2601）不是收入；贷款发放是资产（1301）
//!    不是费用；PD/LGD/EAD 显式情景输入（Fixture 标注），不从股票跌幅推导。
//! 3. 现金流分类：存/贷/收息/付息/手续费均经营活动（CAS 30 (2026)
//!    §45–§47——「向客户提供融资」为主要业务活动的归类选择）。
//! 4. 客户流动性约束：提款/付息超可支付现金 → `PaymentFailed`（类型化，
//!    银行继续运行；无透支、无自动补钱、K2 无兜底）。
//!
//! [`Books::post_batch`]: crate::accounting::Books::post_batch

mod chart;
mod config;
mod deposits;
mod ecl;
mod error;
mod fees;
mod interest;
mod lending;
mod loans;
mod writeoff;

pub use chart::bank_chart_v3;
pub use config::BankConfig;
pub use deposits::{DepositAccrualItem, DepositState};
pub use ecl::{EclPolicy, EclScenario, EclStage, StageTransferRecord};
pub use error::BankError;
pub use interest::LoanAccrualItem;
pub use loans::BankLoanState;

use crate::accounting::reports::bank::{bank_presentation_lines, BankPresentationLines};
use crate::accounting::{AccountingAmount, AccountingError, Books, JournalLine, LedgerAccountId};
use crate::company::bank::deposits::DepositMap;
use crate::company::bank::error::map_post_error;
use crate::company::contracts::ContractId;
use crate::company::counterparty::{
    CounterpartyFlow, CounterpartyId, CounterpartyLedger, FlowDirection,
};
use crate::company::opening::opening_event_id;

/// 银行产品种类（入口显式分类；未支持种类一律类型化 `UnsupportedContract`，
/// docs/company-accounting.md §6——不冒充已实现、不静默 fallback）。
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize,
)]
pub enum BankProductKind {
    /// 定期存款（支持；摊余成本金融负债，CAS 22 §21）。
    TermDeposit,
    /// 定期贷款（支持；摊余成本、固定利率、明确合同现金流，CAS 22 §16/§17）。
    TermLoan,
    /// 结构化衍生品/合同挂钩工具（不支持：CAS 22 §23–§26 + 解释第20号）。
    StructuredDerivative,
    /// FVTPL 类投资（不支持）。
    FvtplInstrument,
    /// FVOCI 类投资（不支持）。
    FvociInstrument,
}

impl BankProductKind {
    fn require_deposit(self) -> Result<(), BankError> {
        match self {
            BankProductKind::TermDeposit => Ok(()),
            other => Err(BankError::UnsupportedContract {
                kind: other,
                detail: "only TermDeposit is supported on the deposit side (docs §6)",
            }),
        }
    }

    fn require_loan(self) -> Result<(), BankError> {
        match self {
            BankProductKind::TermLoan => Ok(()),
            other => Err(BankError::UnsupportedContract {
                kind: other,
                detail: "only TermLoan is supported on the lending side (docs §6)",
            }),
        }
    }
}

/// 银行账套：Books + 存款/贷款子账 + 对手方 + ECL 政策（全部随存档序列化；
/// `Books` 恢复走重放路径，其余结构体 serde 直存）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct BankBooks {
    books: Books,
    deposits: DepositMap,
    loans: std::collections::BTreeMap<ContractId, BankLoanState>,
    counterparties: CounterpartyLedger,
    ecl_policy: EclPolicy,
    next_event_id: u64,
}

impl BankBooks {
    /// 装配：ECL 政策校验 → 开局行子账种子守卫 → 开局凭证过账（原子）→
    /// 对手方登记。任一步失败 ⇒ 不产生半构造账套。
    pub fn new(config: BankConfig) -> Result<Self, BankError> {
        config.ecl_policy.validate()?;
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
            deposits: DepositMap::new(),
            loans: std::collections::BTreeMap::new(),
            counterparties,
            ecl_policy: config.ecl_policy,
            next_event_id: 2,
        })
    }

    // ===== 只读访问面（子账与状态）=====

    pub fn books(&self) -> &Books {
        &self.books
    }

    pub fn counterparties(&self) -> &CounterpartyLedger {
        &self.counterparties
    }

    pub fn deposit(&self, id: &ContractId) -> Option<&DepositState> {
        self.deposits.get(id)
    }

    pub fn deposits(&self) -> impl Iterator<Item = (&ContractId, &DepositState)> {
        self.deposits.iter()
    }

    pub fn loan(&self, id: &ContractId) -> Option<&BankLoanState> {
        self.loans.get(id)
    }

    pub fn loans(&self) -> impl Iterator<Item = (&ContractId, &BankLoanState)> {
        self.loans.iter()
    }

    pub fn ecl_policy(&self) -> &EclPolicy {
        &self.ecl_policy
    }

    /// 报表分类层胶水：总账 → CAS 30 (2026) 银行列报行（任务 13 消费）。
    pub fn presentation_lines(&self) -> Result<BankPresentationLines, AccountingError> {
        bank_presentation_lines(self.books.ledger())
    }

    // ===== 处理器共享内部 =====

    /// 原子过账 + 领域错误映射 + 事件 id 提交：批末负现金 → `PaymentFailed`。
    /// `next_event_id` 只在过账**成功**后推进——任何拒绝连事件 id 都不消耗。
    pub(super) fn post_with_commit(
        &mut self,
        next_event_id: u64,
        entries: Vec<crate::accounting::JournalEntry>,
    ) -> Result<(), BankError> {
        self.books.post_batch(entries).map_err(map_post_error)?;
        self.next_event_id = next_event_id;
        Ok(())
    }

    /// 对手方已登记（处理器前置校验；资金流跨边界留痕的 K2 要求）。
    pub(super) fn ensure_counterparty(&self, id: &CounterpartyId) -> Result<(), BankError> {
        if self.counterparties.get(id).is_none() {
            return Err(BankError::Company(
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
    ) -> Result<(), BankError> {
        self.counterparties.record_flow(CounterpartyFlow {
            date,
            counterparty: counterparty.clone(),
            direction,
            amount,
            memo: memo.to_string(),
        })?;
        Ok(())
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
