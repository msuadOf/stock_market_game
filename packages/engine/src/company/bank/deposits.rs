//! 存款子账状态与存取处理器（K3 银行，任务 9；计提/付息在 `interest.rs`）。
//! 客户存款是**摊余成本金融负债**（CAS 22 §21）——存入贷记负债（2011/2601）
//! 而非收入（K3 红线）；提取是负债减少；付息是利息支出（6411）现金结清。
//!
//! 现金流分类：存/取均经营活动（CAS 30 (2026) §45–§47——「向客户提供融资」
//! 为主要业务活动的归类选择，游戏固定选经营）。
//!
//! 简化登记：定期存款允许提前提取（客户流动性事件，按原利率计息至提取日，
//! 无活期/定期利率切换与自动转存模型）；到期后停止计息（interest.rs 守卫）。

use std::collections::BTreeMap;

use crate::accounting::{
    AccountingAmount, BusinessEventId, BusinessKind, CashFlowClass, FractionUnits, JournalEntry,
    PostingSide,
};
use crate::calendar::CivilDate;
use crate::company::bank::loans::validate_terms;
use crate::company::bank::{chart, BankBooks, BankError, BankProductKind};
use crate::company::contracts::ContractId;
use crate::company::counterparty::{CounterpartyId, FlowDirection};

/// 存款子账容器类型别名。
pub type DepositMap = BTreeMap<ContractId, DepositState>;

/// 单笔存款状态：本金（负债）、已提未付利息、计息余数、上次计提日。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct DepositState {
    principal: AccountingAmount,
    accrued_payable: AccountingAmount,
    /// ACT/365F 计息余数（单位 1/3_650_000 分；随合同累计，落分守恒）。
    carried: FractionUnits,
    last_accrual_date: CivilDate,
    rate_bp: i32,
    counterparty: CounterpartyId,
    start_date: CivilDate,
    maturity_date: CivilDate,
}

impl DepositState {
    fn new(
        principal: AccountingAmount,
        rate_bp: i32,
        counterparty: CounterpartyId,
        start: CivilDate,
        maturity: CivilDate,
    ) -> Self {
        Self {
            principal,
            accrued_payable: AccountingAmount::ZERO,
            carried: FractionUnits::ZERO,
            last_accrual_date: start,
            rate_bp,
            counterparty,
            start_date: start,
            maturity_date: maturity,
        }
    }

    pub fn principal(&self) -> AccountingAmount {
        self.principal
    }

    pub fn accrued_payable(&self) -> AccountingAmount {
        self.accrued_payable
    }

    pub fn carried(&self) -> FractionUnits {
        self.carried
    }

    pub fn last_accrual_date(&self) -> CivilDate {
        self.last_accrual_date
    }

    pub fn counterparty(&self) -> &CounterpartyId {
        &self.counterparty
    }

    pub fn rate_bp(&self) -> i32 {
        self.rate_bp
    }

    pub fn start_date(&self) -> CivilDate {
        self.start_date
    }

    pub fn maturity_date(&self) -> CivilDate {
        self.maturity_date
    }

    /// 存款过账科目：期限 ≤ 365 天 → 短期（2011）；否则长期（2601）。
    pub(super) fn deposit_account(&self) -> &'static str {
        if self.maturity_date.days_since(self.start_date) <= 365 {
            chart::acct::ST_DEPOSIT
        } else {
            chart::acct::LT_DEPOSIT
        }
    }

    /// 落地一条计提（金额为零也推进余数与计提日——守恒所需）。
    pub(super) fn apply_accrual(
        &mut self,
        amount: AccountingAmount,
        remaining: FractionUnits,
        through: CivilDate,
    ) -> Result<(), crate::accounting::AccountingError> {
        self.accrued_payable = self.accrued_payable.add(amount)?;
        self.carried = remaining;
        self.last_accrual_date = through;
        Ok(())
    }

    /// 提取本金（调用方已验证金额 ≤ 未偿本金）。
    pub(super) fn withdraw(
        &mut self,
        amount: AccountingAmount,
    ) -> Result<(), crate::accounting::AccountingError> {
        self.principal = self.principal.sub(amount)?;
        Ok(())
    }

    /// 付清已提未付利息。
    pub(super) fn settle_accrued(&mut self) {
        self.accrued_payable = AccountingAmount::ZERO;
    }
}

/// 单笔存款计提结果（`accrued_through` = 有效计提截止 = min(计提日, 到期日)）。
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct DepositAccrualItem {
    pub deposit: ContractId,
    pub days: i64,
    pub amount: AccountingAmount,
    pub remaining_carried: FractionUnits,
    pub accrued_through: CivilDate,
}

impl BankBooks {
    /// 吸收存款（CAS 22 §21 摊余成本金融负债）：Dr 1003 / Cr 2011|2601
    /// （经营）。**存款不是收入**（K3 红线）。
    #[allow(clippy::too_many_arguments)]
    pub fn accept_deposit(
        &mut self,
        kind: BankProductKind,
        deposit: ContractId,
        depositor: &CounterpartyId,
        principal: AccountingAmount,
        annual_rate_bp: i32,
        start: CivilDate,
        maturity: CivilDate,
    ) -> Result<BusinessEventId, BankError> {
        kind.require_deposit()?;
        validate_terms(&deposit, principal, annual_rate_bp, start, maturity)?;
        self.ensure_counterparty(depositor)?;
        if self.deposits.contains_key(&deposit) || self.loans.contains_key(&deposit) {
            return Err(BankError::DuplicateContract { contract: deposit });
        }
        let event = BusinessEventId::new(self.next_event_id);
        let state = DepositState::new(
            principal,
            annual_rate_bp,
            depositor.clone(),
            start,
            maturity,
        );
        let account = state.deposit_account();
        self.post_with_commit(
            self.next_event_id + 1,
            vec![JournalEntry {
                source: event,
                date: start,
                kind: BusinessKind::CustomerDeposit,
                cash_flow: CashFlowClass::Operating,
                lines: vec![
                    super::line(chart::acct::CASH, PostingSide::Debit, principal),
                    super::line(account, PostingSide::Credit, principal),
                ],
            }],
        )?;
        self.deposits.insert(deposit.clone(), state);
        self.record_flow(
            start,
            depositor,
            FlowDirection::Inbound,
            principal,
            "customer deposit",
        )?;
        Ok(event)
    }

    /// 存款提取（客户流动性约束）：Dr 2011|2601 / Cr 1003（经营）。
    /// 超存款本金 → `WithdrawalBeyondPrincipal`；超可支付现金 →
    /// `PaymentFailed`（负现金禁令，银行继续运行——K2 不透支不补钱）。
    pub fn withdraw_deposit(
        &mut self,
        deposit: &ContractId,
        amount: AccountingAmount,
        date: CivilDate,
    ) -> Result<BusinessEventId, BankError> {
        let state = self.deposit(deposit).ok_or(BankError::UnknownDeposit {
            contract: deposit.clone(),
        })?;
        if !amount.is_positive() {
            return Err(BankError::NonPositiveAmount {
                what: "withdrawal",
                amount,
            });
        }
        if amount > state.principal() {
            return Err(BankError::WithdrawalBeyondPrincipal {
                contract: deposit.clone(),
                requested: amount,
                outstanding: state.principal(),
            });
        }
        let account = state.deposit_account();
        let depositor = state.counterparty().clone();
        let event = BusinessEventId::new(self.next_event_id);
        self.post_with_commit(
            self.next_event_id + 1,
            vec![JournalEntry {
                source: event,
                date,
                kind: BusinessKind::CustomerWithdrawal,
                cash_flow: CashFlowClass::Operating,
                lines: vec![
                    super::line(account, PostingSide::Debit, amount),
                    super::line(chart::acct::CASH, PostingSide::Credit, amount),
                ],
            }],
        )?;
        if let Some(state) = self.deposits.get_mut(deposit) {
            state.withdraw(amount)?;
        }
        self.record_flow(
            date,
            &depositor,
            FlowDirection::Outbound,
            amount,
            "customer withdrawal",
        )?;
        Ok(event)
    }
}
