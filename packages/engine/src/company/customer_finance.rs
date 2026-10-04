//! 客户有限现金与债务事实；此基础模块不接入公司总账或经营调度。

use std::collections::BTreeMap;

use crate::accounting::AccountingAmount;
use crate::calendar::CivilDate;
use crate::company::counterparty::CounterpartyId;
use thiserror::Error;

/// 客户侧财务事件身份；同一身份只能对应相同命令载荷。
#[derive(
    Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize,
)]
pub struct CustomerFinanceEventId(pub String);

/// 收支端点。`External` 是模拟边界外的实名资金端点，不代表无限现金账户。
#[derive(
    Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize,
)]
pub enum CashEndpoint {
    Customer(CounterpartyId),
    External(String),
}

/// 客户现金流事实；金额恒正，资金方向由来源和去向明确表达。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct CustomerCashFlow {
    pub event: CustomerFinanceEventId,
    pub date: CivilDate,
    pub source: CashEndpoint,
    pub destination: CashEndpoint,
    pub amount: AccountingAmount,
}

/// 客户债务 id。
#[derive(
    Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize,
)]
pub struct DebtId(pub String);

/// 核销决策调用方提供的事实依据；本模块不根据账龄或随机结果判定核销。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize)]
pub struct WriteOffEvidence(String);

impl WriteOffEvidence {
    pub fn new(description: String) -> Result<Self, CustomerFinanceError> {
        if description.trim().is_empty() {
            return Err(CustomerFinanceError::WriteOffEvidenceRequired);
        }
        Ok(Self(description))
    }

    pub fn description(&self) -> &str {
        &self.0
    }
}

/// 单项债务事实：普通账面余额、法律余额和核销后可回收余额分别派生。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize)]
pub struct CustomerDebt {
    id: DebtId,
    opened_on: CivilDate,
    due_on: CivilDate,
    principal: AccountingAmount,
    paid: AccountingAmount,
    written_off: AccountingAmount,
    recovered: AccountingAmount,
    registration_sequence: u64,
}

impl CustomerDebt {
    pub fn id(&self) -> &DebtId {
        &self.id
    }
    pub fn opened_on(&self) -> CivilDate {
        self.opened_on
    }
    pub fn due_on(&self) -> CivilDate {
        self.due_on
    }
    pub fn principal(&self) -> AccountingAmount {
        self.principal
    }
    pub fn ordinary_balance(&self) -> AccountingAmount {
        self.principal
            .sub(self.paid)
            .and_then(|value| value.sub(self.written_off))
            .expect("validated debt balances cannot underflow")
    }
    pub fn legal_balance(&self) -> AccountingAmount {
        self.principal
            .sub(self.paid)
            .and_then(|value| value.sub(self.recovered))
            .expect("validated legal debt balances cannot underflow")
    }
    pub fn written_off_recoverable(&self) -> AccountingAmount {
        self.written_off
            .sub(self.recovered)
            .expect("recovery cannot exceed write-off")
    }
    pub fn overdue_since(&self, as_of: CivilDate) -> Option<CivilDate> {
        (as_of > self.due_on && self.legal_balance().is_positive()).then_some(self.due_on)
    }
}

/// 一次普通到期付款在债务间的分配。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct DebtPayment {
    pub debt: DebtId,
    pub amount: AccountingAmount,
    pub original_due_on: CivilDate,
    pub overdue_since: Option<CivilDate>,
}

/// 一次核销事实。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct DebtWriteOff {
    pub debt: DebtId,
    pub amount: AccountingAmount,
    pub legal_balance_after: AccountingAmount,
}

/// 单个客户有限现金与其独立债务。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize)]
pub struct CustomerFinanceState {
    cash: AccountingAmount,
    debts: BTreeMap<DebtId, CustomerDebt>,
}

impl CustomerFinanceState {
    pub fn cash(&self) -> AccountingAmount {
        self.cash
    }
    pub fn debt(&self, id: &DebtId) -> Option<&CustomerDebt> {
        self.debts.get(id)
    }
    pub fn debts(&self) -> impl Iterator<Item = (&DebtId, &CustomerDebt)> {
        self.debts.iter()
    }
}

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize)]
enum FinanceCommand {
    CashFlow {
        source: CashEndpoint,
        destination: CashEndpoint,
        amount: AccountingAmount,
        date: CivilDate,
    },
    RepayDue {
        customer: CounterpartyId,
        destination: CashEndpoint,
        date: CivilDate,
    },
    WriteOff {
        customer: CounterpartyId,
        debt: DebtId,
        amount: AccountingAmount,
        date: CivilDate,
        evidence: WriteOffEvidence,
    },
    Recover {
        customer: CounterpartyId,
        debt: DebtId,
        destination: CashEndpoint,
        amount: AccountingAmount,
        date: CivilDate,
    },
}

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize)]
enum FinanceResult {
    Flow,
    Payments(Vec<DebtPayment>),
    WriteOff(DebtWriteOff),
    Recovered,
}

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize)]
struct AppliedEvent {
    command: FinanceCommand,
    result: FinanceResult,
}

/// 客户财务事实簿；所有现金与债务互不共享。
#[derive(Clone, Eq, PartialEq, Debug, Default, serde::Serialize)]
pub struct CustomerFinanceBook {
    customers: BTreeMap<CounterpartyId, CustomerFinanceState>,
    cash_flows: Vec<CustomerCashFlow>,
    events: BTreeMap<CustomerFinanceEventId, AppliedEvent>,
    next_debt_sequence: u64,
    last_event_date: Option<CivilDate>,
}

impl CustomerFinanceBook {
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记客户并显式给定其有限开局现金；不从欠款或概率推算现金。
    pub fn register_customer(
        &mut self,
        id: CounterpartyId,
        initial_cash: AccountingAmount,
    ) -> Result<(), CustomerFinanceError> {
        validate_id(&id.0, "customer")?;
        if initial_cash.is_negative() {
            return Err(CustomerFinanceError::NegativeCash {
                customer: id,
                cash: initial_cash,
            });
        }
        if self.customers.contains_key(&id) {
            return Err(CustomerFinanceError::DuplicateCustomer { customer: id });
        }
        self.customers.insert(
            id,
            CustomerFinanceState {
                cash: initial_cash,
                debts: BTreeMap::new(),
            },
        );
        Ok(())
    }

    /// 按稳定客户 id 查询独立财务状态。
    pub fn customer(&self, id: &CounterpartyId) -> Option<&CustomerFinanceState> {
        self.customers.get(id)
    }

    /// 查询带来源、去向、日期和金额的已发生客户现金流。
    pub fn cash_flows(&self) -> &[CustomerCashFlow] {
        &self.cash_flows
    }

    /// 为已登记客户开立独立债务；登记序号用于同到期日的普通付款顺序。
    pub fn register_debt(
        &mut self,
        customer: &CounterpartyId,
        id: DebtId,
        opened_on: CivilDate,
        due_on: CivilDate,
        principal: AccountingAmount,
    ) -> Result<(), CustomerFinanceError> {
        validate_id(&id.0, "debt")?;
        let state =
            self.customers
                .get(customer)
                .ok_or_else(|| CustomerFinanceError::UnknownCustomer {
                    customer: customer.clone(),
                })?;
        self.validate_event_date(opened_on)?;
        if !principal.is_positive() {
            return Err(CustomerFinanceError::NonPositiveAmount { amount: principal });
        }
        if due_on < opened_on {
            return Err(CustomerFinanceError::DueBeforeOpen { opened_on, due_on });
        }
        if state.debts.contains_key(&id) {
            return Err(CustomerFinanceError::DuplicateDebt {
                customer: customer.clone(),
                debt: id,
            });
        }
        let next = self
            .next_debt_sequence
            .checked_add(1)
            .ok_or(CustomerFinanceError::SequenceOverflow)?;
        self.customers
            .get_mut(customer)
            .expect("customer validated")
            .debts
            .insert(
                id.clone(),
                CustomerDebt {
                    id,
                    opened_on,
                    due_on,
                    principal,
                    paid: AccountingAmount::ZERO,
                    written_off: AccountingAmount::ZERO,
                    recovered: AccountingAmount::ZERO,
                    registration_sequence: self.next_debt_sequence,
                },
            );
        self.next_debt_sequence = next;
        self.last_event_date = Some(opened_on);
        Ok(())
    }

    /// 记录一笔真实现金流；客户端余额不足或任一金额运算失败时整笔不变。
    pub fn record_cash_flow(
        &mut self,
        event: CustomerFinanceEventId,
        source: CashEndpoint,
        destination: CashEndpoint,
        amount: AccountingAmount,
        date: CivilDate,
    ) -> Result<(), CustomerFinanceError> {
        let command = FinanceCommand::CashFlow {
            source: source.clone(),
            destination: destination.clone(),
            amount,
            date,
        };
        if let Some(result) = self.replay(&event, &command)? {
            return expect_result(result, "cash flow");
        }
        validate_event_id(&event)?;
        self.validate_event_date(date)?;
        validate_endpoints(&self.customers, &source, &destination, amount)?;
        let updates = self.cash_updates(&source, &destination, amount)?;
        self.apply_cash_updates(updates);
        self.cash_flows.push(CustomerCashFlow {
            event: event.clone(),
            date,
            source,
            destination,
            amount,
        });
        self.events.insert(
            event,
            AppliedEvent {
                command,
                result: FinanceResult::Flow,
            },
        );
        self.last_event_date = Some(date);
        Ok(())
    }

    /// 按原到期日、再按登记序号偿付；只使用客户现有现金，允许部分支付。
    pub fn repay_due(
        &mut self,
        customer: &CounterpartyId,
        destination: CashEndpoint,
        date: CivilDate,
        event: CustomerFinanceEventId,
    ) -> Result<Vec<DebtPayment>, CustomerFinanceError> {
        let command = FinanceCommand::RepayDue {
            customer: customer.clone(),
            destination: destination.clone(),
            date,
        };
        if let Some(result) = self.replay(&event, &command)? {
            return expect_result(result, "repayment");
        }
        validate_event_id(&event)?;
        self.validate_event_date(date)?;
        let state =
            self.customers
                .get(customer)
                .ok_or_else(|| CustomerFinanceError::UnknownCustomer {
                    customer: customer.clone(),
                })?;
        validate_endpoint_pair(
            &self.customers,
            &CashEndpoint::Customer(customer.clone()),
            &destination,
        )?;
        validate_payment_destination(&destination)?;
        let mut due: Vec<_> = state
            .debts
            .values()
            .filter(|debt| debt.due_on <= date && debt.ordinary_balance().is_positive())
            .collect();
        due.sort_by_key(|debt| (debt.due_on, debt.registration_sequence));
        let mut available = state.cash;
        let mut payments = Vec::new();
        let mut paid_balances = Vec::new();
        for debt in due {
            if available.is_zero() {
                break;
            }
            let amount = if available < debt.ordinary_balance() {
                available
            } else {
                debt.ordinary_balance()
            };
            available = available.sub(amount)?;
            paid_balances.push((debt.id.clone(), debt.paid.add(amount)?));
            payments.push(DebtPayment {
                debt: debt.id.clone(),
                amount,
                original_due_on: debt.due_on,
                overdue_since: (date > debt.due_on).then_some(debt.due_on),
            });
        }
        if !payments.is_empty() {
            let total = payments
                .iter()
                .try_fold(AccountingAmount::ZERO, |sum, payment| {
                    sum.add(payment.amount)
                })?;
            let updates = self.cash_updates(
                &CashEndpoint::Customer(customer.clone()),
                &destination,
                total,
            )?;
            for (debt_id, paid) in paid_balances {
                let debt = self
                    .customers
                    .get_mut(customer)
                    .expect("customer validated")
                    .debts
                    .get_mut(&debt_id)
                    .expect("debt captured");
                debt.paid = paid;
            }
            for payment in &payments {
                self.cash_flows.push(CustomerCashFlow {
                    event: event.clone(),
                    date,
                    source: CashEndpoint::Customer(customer.clone()),
                    destination: destination.clone(),
                    amount: payment.amount,
                });
            }
            self.apply_cash_updates(updates);
        }
        self.events.insert(
            event,
            AppliedEvent {
                command,
                result: FinanceResult::Payments(payments.clone()),
            },
        );
        self.last_event_date = Some(date);
        Ok(payments)
    }

    /// 按调用方提供的事实依据部分核销普通余额；不免除法律债务，不自动判定标准。
    pub fn write_off(
        &mut self,
        customer: &CounterpartyId,
        debt_id: &DebtId,
        amount: AccountingAmount,
        date: CivilDate,
        evidence: WriteOffEvidence,
        event: CustomerFinanceEventId,
    ) -> Result<DebtWriteOff, CustomerFinanceError> {
        let command = FinanceCommand::WriteOff {
            customer: customer.clone(),
            debt: debt_id.clone(),
            amount,
            date,
            evidence,
        };
        if let Some(result) = self.replay(&event, &command)? {
            return expect_result(result, "write-off");
        }
        validate_event_id(&event)?;
        self.validate_event_date(date)?;
        let debt = self.debt_or_err(customer, debt_id)?;
        if date < debt.opened_on {
            return Err(CustomerFinanceError::EventBeforeDebtOpened {
                date,
                opened_on: debt.opened_on,
                debt: debt_id.clone(),
            });
        }
        if !amount.is_positive() {
            return Err(CustomerFinanceError::NonPositiveAmount { amount });
        }
        if amount > debt.ordinary_balance() {
            return Err(CustomerFinanceError::WriteOffExceedsOrdinary {
                debt: debt_id.clone(),
                requested: amount,
                ordinary: debt.ordinary_balance(),
            });
        }
        let result = DebtWriteOff {
            debt: debt_id.clone(),
            amount,
            legal_balance_after: debt.legal_balance(),
        };
        let written_off = debt.written_off.add(amount)?;
        self.customers
            .get_mut(customer)
            .expect("customer validated")
            .debts
            .get_mut(debt_id)
            .expect("debt validated")
            .written_off = written_off;
        self.events.insert(
            event,
            AppliedEvent {
                command,
                result: FinanceResult::WriteOff(result.clone()),
            },
        );
        self.last_event_date = Some(date);
        Ok(result)
    }

    /// 以独立收款事件收回核销部分；金额不能超过尚未回收的核销余额。
    pub fn recover_written_off(
        &mut self,
        customer: &CounterpartyId,
        debt_id: &DebtId,
        destination: CashEndpoint,
        amount: AccountingAmount,
        date: CivilDate,
        event: CustomerFinanceEventId,
    ) -> Result<(), CustomerFinanceError> {
        let command = FinanceCommand::Recover {
            customer: customer.clone(),
            debt: debt_id.clone(),
            destination: destination.clone(),
            amount,
            date,
        };
        if let Some(result) = self.replay(&event, &command)? {
            return expect_result(result, "recovery");
        }
        validate_event_id(&event)?;
        self.validate_event_date(date)?;
        let state =
            self.customers
                .get(customer)
                .ok_or_else(|| CustomerFinanceError::UnknownCustomer {
                    customer: customer.clone(),
                })?;
        let debt = state
            .debts
            .get(debt_id)
            .ok_or_else(|| CustomerFinanceError::UnknownDebt {
                customer: customer.clone(),
                debt: debt_id.clone(),
            })?;
        if date < debt.opened_on {
            return Err(CustomerFinanceError::EventBeforeDebtOpened {
                date,
                opened_on: debt.opened_on,
                debt: debt_id.clone(),
            });
        }
        if !amount.is_positive() {
            return Err(CustomerFinanceError::NonPositiveAmount { amount });
        }
        if amount > debt.written_off_recoverable() {
            return Err(CustomerFinanceError::RecoveryExceedsWrittenOff {
                debt: debt_id.clone(),
                requested: amount,
                recoverable: debt.written_off_recoverable(),
            });
        }
        validate_endpoints(
            &self.customers,
            &CashEndpoint::Customer(customer.clone()),
            &destination,
            amount,
        )?;
        validate_payment_destination(&destination)?;
        let updates = self.cash_updates(
            &CashEndpoint::Customer(customer.clone()),
            &destination,
            amount,
        )?;
        let recovered = debt.recovered.add(amount)?;
        let debt = self
            .customers
            .get_mut(customer)
            .expect("customer validated")
            .debts
            .get_mut(debt_id)
            .expect("debt validated");
        debt.recovered = recovered;
        self.apply_cash_updates(updates);
        self.cash_flows.push(CustomerCashFlow {
            event: event.clone(),
            date,
            source: CashEndpoint::Customer(customer.clone()),
            destination,
            amount,
        });
        self.events.insert(
            event,
            AppliedEvent {
                command,
                result: FinanceResult::Recovered,
            },
        );
        self.last_event_date = Some(date);
        Ok(())
    }

    fn replay(
        &self,
        event: &CustomerFinanceEventId,
        command: &FinanceCommand,
    ) -> Result<Option<&FinanceResult>, CustomerFinanceError> {
        if let Some(applied) = self.events.get(event) {
            if &applied.command != command {
                return Err(CustomerFinanceError::EventPayloadConflict {
                    event: event.clone(),
                });
            }
            return Ok(Some(&applied.result));
        }
        Ok(None)
    }

    fn validate_event_date(&self, date: CivilDate) -> Result<(), CustomerFinanceError> {
        if let Some(previous) = self.last_event_date {
            if date < previous {
                return Err(CustomerFinanceError::EventDateRegression {
                    previous,
                    attempted: date,
                });
            }
        }
        Ok(())
    }

    fn debt_or_err(
        &self,
        customer: &CounterpartyId,
        debt: &DebtId,
    ) -> Result<&CustomerDebt, CustomerFinanceError> {
        self.customers
            .get(customer)
            .ok_or_else(|| CustomerFinanceError::UnknownCustomer {
                customer: customer.clone(),
            })?
            .debts
            .get(debt)
            .ok_or_else(|| CustomerFinanceError::UnknownDebt {
                customer: customer.clone(),
                debt: debt.clone(),
            })
    }

    fn cash_updates(
        &self,
        source: &CashEndpoint,
        destination: &CashEndpoint,
        amount: AccountingAmount,
    ) -> Result<Vec<(CounterpartyId, AccountingAmount)>, CustomerFinanceError> {
        let mut updates = Vec::new();
        if let CashEndpoint::Customer(id) = source {
            let state =
                self.customers
                    .get(id)
                    .ok_or_else(|| CustomerFinanceError::UnknownCustomer {
                        customer: id.clone(),
                    })?;
            if state.cash < amount {
                return Err(CustomerFinanceError::InsufficientCash {
                    customer: id.clone(),
                    available: state.cash,
                    requested: amount,
                });
            }
            updates.push((id.clone(), state.cash.sub(amount)?));
        }
        if let CashEndpoint::Customer(id) = destination {
            let state =
                self.customers
                    .get(id)
                    .ok_or_else(|| CustomerFinanceError::UnknownCustomer {
                        customer: id.clone(),
                    })?;
            let balance = state.cash.add(amount)?;
            if let Some((_, value)) = updates.iter_mut().find(|(existing, _)| existing == id) {
                *value = balance;
            } else {
                updates.push((id.clone(), balance));
            }
        }
        Ok(updates)
    }

    fn apply_cash_updates(&mut self, updates: Vec<(CounterpartyId, AccountingAmount)>) {
        for (id, cash) in updates {
            self.customers
                .get_mut(&id)
                .expect("customer validated")
                .cash = cash;
        }
    }
}

fn validate_id(value: &str, what: &'static str) -> Result<(), CustomerFinanceError> {
    if value.trim().is_empty() {
        return Err(CustomerFinanceError::InvalidId { what });
    }
    Ok(())
}

fn validate_event_id(event: &CustomerFinanceEventId) -> Result<(), CustomerFinanceError> {
    validate_id(&event.0, "event")
}

fn validate_endpoints(
    customers: &BTreeMap<CounterpartyId, CustomerFinanceState>,
    source: &CashEndpoint,
    destination: &CashEndpoint,
    amount: AccountingAmount,
) -> Result<(), CustomerFinanceError> {
    if !amount.is_positive() {
        return Err(CustomerFinanceError::NonPositiveAmount { amount });
    }
    validate_endpoint_pair(customers, source, destination)
}

fn validate_endpoint_pair(
    customers: &BTreeMap<CounterpartyId, CustomerFinanceState>,
    source: &CashEndpoint,
    destination: &CashEndpoint,
) -> Result<(), CustomerFinanceError> {
    if source == destination {
        return Err(CustomerFinanceError::SelfTransfer);
    }
    if let (CashEndpoint::Customer(source), CashEndpoint::Customer(destination)) =
        (source, destination)
    {
        if source == destination {
            return Err(CustomerFinanceError::SelfTransfer);
        }
    }
    for endpoint in [source, destination] {
        match endpoint {
            CashEndpoint::Customer(id) if !customers.contains_key(id) => {
                return Err(CustomerFinanceError::UnknownCustomer {
                    customer: id.clone(),
                });
            }
            CashEndpoint::External(id) => validate_id(id, "external endpoint")?,
            CashEndpoint::Customer(_) => {}
        }
    }
    if !matches!(source, CashEndpoint::Customer(_))
        && !matches!(destination, CashEndpoint::Customer(_))
    {
        return Err(CustomerFinanceError::NoCustomerEndpoint);
    }
    Ok(())
}

fn validate_payment_destination(destination: &CashEndpoint) -> Result<(), CustomerFinanceError> {
    if !matches!(destination, CashEndpoint::External(_)) {
        return Err(CustomerFinanceError::PaymentDestinationMustBeExternal);
    }
    Ok(())
}

fn expect_result<T>(
    result: &FinanceResult,
    operation: &'static str,
) -> Result<T, CustomerFinanceError>
where
    T: TryFrom<FinanceResult, Error = ()>,
{
    T::try_from(result.clone())
        .map_err(|()| CustomerFinanceError::EventResultMismatch { operation })
}

impl TryFrom<FinanceResult> for Vec<DebtPayment> {
    type Error = ();
    fn try_from(value: FinanceResult) -> Result<Self, Self::Error> {
        if let FinanceResult::Payments(value) = value {
            Ok(value)
        } else {
            Err(())
        }
    }
}
impl TryFrom<FinanceResult> for DebtWriteOff {
    type Error = ();
    fn try_from(value: FinanceResult) -> Result<Self, Self::Error> {
        if let FinanceResult::WriteOff(value) = value {
            Ok(value)
        } else {
            Err(())
        }
    }
}
impl TryFrom<FinanceResult> for () {
    type Error = ();
    fn try_from(value: FinanceResult) -> Result<Self, Self::Error> {
        if matches!(value, FinanceResult::Flow | FinanceResult::Recovered) {
            Ok(())
        } else {
            Err(())
        }
    }
}

/// 客户财务事实错误。
#[derive(Clone, Eq, PartialEq, Debug, Error)]
pub enum CustomerFinanceError {
    #[error("invalid empty {what} id")]
    InvalidId { what: &'static str },
    #[error("customer {customer:?} already registered")]
    DuplicateCustomer { customer: CounterpartyId },
    #[error("unknown customer {customer:?}")]
    UnknownCustomer { customer: CounterpartyId },
    #[error("customer {customer:?} has negative opening cash {cash:?}")]
    NegativeCash {
        customer: CounterpartyId,
        cash: AccountingAmount,
    },
    #[error("debt {debt:?} already exists for customer {customer:?}")]
    DuplicateDebt {
        customer: CounterpartyId,
        debt: DebtId,
    },
    #[error("unknown debt {debt:?} for customer {customer:?}")]
    UnknownDebt {
        customer: CounterpartyId,
        debt: DebtId,
    },
    #[error("amount must be positive: {amount:?}")]
    NonPositiveAmount { amount: AccountingAmount },
    #[error("debt due date {due_on} precedes opening date {opened_on}")]
    DueBeforeOpen {
        opened_on: CivilDate,
        due_on: CivilDate,
    },
    #[error("financial event date {attempted} precedes prior event date {previous}")]
    EventDateRegression {
        previous: CivilDate,
        attempted: CivilDate,
    },
    #[error("event date {date} precedes opening date {opened_on} for debt {debt:?}")]
    EventBeforeDebtOpened {
        debt: DebtId,
        date: CivilDate,
        opened_on: CivilDate,
    },
    #[error("customer {customer:?} has {available:?} cash, cannot pay {requested:?}")]
    InsufficientCash {
        customer: CounterpartyId,
        available: AccountingAmount,
        requested: AccountingAmount,
    },
    #[error("cash transfer requires a customer endpoint")]
    NoCustomerEndpoint,
    #[error("cash transfer source and destination are identical")]
    SelfTransfer,
    #[error("customer debt payment destination must be an external company endpoint")]
    PaymentDestinationMustBeExternal,
    #[error("event {event:?} was already used with a different payload")]
    EventPayloadConflict { event: CustomerFinanceEventId },
    #[error("event result does not match requested operation {operation}")]
    EventResultMismatch { operation: &'static str },
    #[error("write-off {requested:?} exceeds ordinary balance {ordinary:?} for debt {debt:?}")]
    WriteOffExceedsOrdinary {
        debt: DebtId,
        requested: AccountingAmount,
        ordinary: AccountingAmount,
    },
    #[error("write-off requires a non-empty factual basis")]
    WriteOffEvidenceRequired,
    #[error(
        "recovery {requested:?} exceeds unrecovered written-off balance {recoverable:?} for debt {debt:?}"
    )]
    RecoveryExceedsWrittenOff {
        debt: DebtId,
        requested: AccountingAmount,
        recoverable: AccountingAmount,
    },
    #[error("debt registration sequence overflow")]
    SequenceOverflow,
    #[error(transparent)]
    Accounting(#[from] crate::accounting::AccountingError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::CivilDate;

    fn date(value: &str) -> CivilDate {
        CivilDate::from_iso(value).expect("有效日期")
    }

    fn id(value: &str) -> CounterpartyId {
        CounterpartyId(value.to_string())
    }

    fn amount(cents: i128) -> AccountingAmount {
        AccountingAmount::from_cents(cents)
    }

    fn debt_book(cash: i128) -> (CustomerFinanceBook, CounterpartyId) {
        let customer = id("customer-a");
        let mut book = CustomerFinanceBook::new();
        book.register_customer(customer.clone(), amount(cash))
            .expect("登记客户");
        book.register_debt(
            &customer,
            DebtId("debt-a".to_string()),
            date("2030-01-01"),
            date("2030-01-02"),
            amount(100),
        )
        .expect("登记债务");
        (book, customer)
    }

    #[test]
    fn customers_have_separate_cash_and_flows_name_both_endpoints() {
        let mut book = CustomerFinanceBook::new();
        let first = id("first");
        let second = id("second");
        book.register_customer(first.clone(), amount(100))
            .expect("登记客户");
        book.register_customer(second.clone(), amount(20))
            .expect("登记客户");

        book.record_cash_flow(
            CustomerFinanceEventId("transfer-1".to_string()),
            CashEndpoint::Customer(first.clone()),
            CashEndpoint::Customer(second.clone()),
            amount(70),
            date("2030-01-01"),
        )
        .expect("客户间转账");

        assert_eq!(book.customer(&first).expect("客户").cash(), amount(30));
        assert_eq!(book.customer(&second).expect("客户").cash(), amount(90));
        let flow = book.cash_flows().last().expect("收支事实");
        assert_eq!(flow.source, CashEndpoint::Customer(first));
        assert_eq!(flow.destination, CashEndpoint::Customer(second));
        let before = book.clone();
        let insufficient = book.record_cash_flow(
            CustomerFinanceEventId("transfer-too-large".to_string()),
            CashEndpoint::Customer(id("first")),
            CashEndpoint::External("supplier-1".to_string()),
            amount(31),
            date("2030-01-01"),
        );
        assert!(matches!(
            insufficient,
            Err(CustomerFinanceError::InsufficientCash { .. })
        ));
        assert_eq!(book, before);

        let mut overflow_book = CustomerFinanceBook::new();
        let source = id("source");
        let destination = id("destination");
        overflow_book
            .register_customer(source.clone(), amount(10))
            .expect("登记客户");
        overflow_book
            .register_customer(destination.clone(), AccountingAmount::MAX)
            .expect("登记客户");
        let before = overflow_book.clone();
        let overflow = overflow_book.record_cash_flow(
            CustomerFinanceEventId("destination-overflow".to_string()),
            CashEndpoint::Customer(source),
            CashEndpoint::Customer(destination),
            amount(1),
            date("2030-01-01"),
        );
        assert!(matches!(overflow, Err(CustomerFinanceError::Accounting(_))));
        assert_eq!(overflow_book, before);
    }

    #[test]
    fn due_payments_follow_original_due_date_then_registration_order_and_stop_at_cash() {
        let customer = id("customer-a");
        let mut book = CustomerFinanceBook::new();
        book.register_customer(customer.clone(), amount(150))
            .expect("登记客户");
        for (debt, due) in [
            ("later", "2030-01-03"),
            ("first", "2030-01-02"),
            ("second", "2030-01-02"),
        ] {
            book.register_debt(
                &customer,
                DebtId(debt.to_string()),
                date("2030-01-01"),
                date(due),
                amount(100),
            )
            .expect("登记债务");
        }

        let payments = book
            .repay_due(
                &customer,
                CashEndpoint::External("company-1".to_string()),
                date("2030-01-02"),
                CustomerFinanceEventId("pay-1".to_string()),
            )
            .expect("按期偿付");

        assert_eq!(payments.len(), 2);
        assert_eq!(payments[0].debt, DebtId("first".to_string()));
        assert_eq!(payments[0].amount, amount(100));
        assert_eq!(payments[1].debt, DebtId("second".to_string()));
        assert_eq!(payments[1].amount, amount(50));
        let state = book.customer(&customer).expect("客户");
        assert_eq!(state.cash(), amount(0));
        let second = state.debt(&DebtId("second".to_string())).expect("债务");
        assert_eq!(second.ordinary_balance(), amount(50));
        assert_eq!(
            second.overdue_since(date("2030-01-05")),
            Some(date("2030-01-02"))
        );
        assert_eq!(second.due_on(), date("2030-01-02"));
    }

    #[test]
    fn retry_is_idempotent_but_reused_event_with_different_payload_is_rejected() {
        let (mut book, customer) = debt_book(100);
        let event = CustomerFinanceEventId("payment-1".to_string());
        let recipient = CashEndpoint::External("company-1".to_string());
        let first = book
            .repay_due(
                &customer,
                recipient.clone(),
                date("2030-01-02"),
                event.clone(),
            )
            .expect("付款");
        let retry = book
            .repay_due(&customer, recipient, date("2030-01-02"), event.clone())
            .expect("幂等重试");
        assert_eq!(retry, first);
        assert_eq!(book.cash_flows().len(), 1);

        let changed = book.repay_due(
            &customer,
            CashEndpoint::External("company-2".to_string()),
            date("2030-01-02"),
            event,
        );
        assert!(matches!(
            changed,
            Err(CustomerFinanceError::EventPayloadConflict { .. })
        ));
        assert_eq!(book.customer(&customer).expect("客户").cash(), amount(0));
        assert_eq!(book.cash_flows().len(), 1);
    }

    #[test]
    fn empty_due_payment_retry_cannot_create_cash_or_repay_later() {
        let (mut book, customer) = debt_book(0);
        let recipient = CashEndpoint::External("company-1".to_string());
        let event = CustomerFinanceEventId("no-cash-at-maturity".to_string());
        assert!(book
            .repay_due(
                &customer,
                recipient.clone(),
                date("2030-01-02"),
                event.clone()
            )
            .expect("无现金到期评估")
            .is_empty());
        assert!(book.cash_flows().is_empty());

        book.record_cash_flow(
            CustomerFinanceEventId("later-operating-income".to_string()),
            CashEndpoint::External("customer-income-source".to_string()),
            CashEndpoint::Customer(customer.clone()),
            amount(100),
            date("2030-01-03"),
        )
        .expect("显式经营现金流入");
        assert!(book
            .repay_due(&customer, recipient.clone(), date("2030-01-02"), event)
            .expect("相同到期事件重试返回既有结果")
            .is_empty());
        assert_eq!(book.customer(&customer).expect("客户").cash(), amount(100));
        assert_eq!(book.cash_flows().len(), 1);
        assert_eq!(
            book.repay_due(
                &customer,
                recipient,
                date("2030-01-03"),
                CustomerFinanceEventId("later-payment-attempt".to_string()),
            )
            .expect("新付款事件")
            .len(),
            1
        );
    }

    #[test]
    fn out_of_order_cashflow_date_is_rejected_without_mutation() {
        let (mut book, customer) = debt_book(100);
        book.repay_due(
            &customer,
            CashEndpoint::External("company-1".to_string()),
            date("2030-01-03"),
            CustomerFinanceEventId("later-payment".to_string()),
        )
        .expect("较晚付款");
        let before = book.clone();
        let out_of_order = book.record_cash_flow(
            CustomerFinanceEventId("earlier-income".to_string()),
            CashEndpoint::External("income-source".to_string()),
            CashEndpoint::Customer(customer),
            amount(25),
            date("2030-01-02"),
        );
        assert!(matches!(
            out_of_order,
            Err(CustomerFinanceError::EventDateRegression { .. })
        ));
        assert_eq!(book, before);
    }

    #[test]
    fn debt_openings_are_registered_in_fact_date_order() {
        let customer = id("customer-a");
        let mut book = CustomerFinanceBook::new();
        book.register_customer(customer.clone(), amount(100))
            .expect("登记客户");
        book.register_debt(
            &customer,
            DebtId("february-debt".to_string()),
            date("2030-02-01"),
            date("2030-02-10"),
            amount(25),
        )
        .expect("较晚开立");
        let before = book.clone();
        let out_of_order = book.register_debt(
            &customer,
            DebtId("january-debt".to_string()),
            date("2030-01-01"),
            date("2030-01-10"),
            amount(25),
        );
        assert!(matches!(
            out_of_order,
            Err(CustomerFinanceError::EventDateRegression { .. })
        ));
        assert_eq!(book, before);
        assert!(WriteOffEvidence::new("  ".to_string()).is_err());
    }

    #[test]
    fn write_off_keeps_legal_balance_and_recovery_is_bounded_and_idempotent() {
        let (mut book, customer) = debt_book(100);
        let write_off_event = CustomerFinanceEventId("write-off-1".to_string());
        let written_off = book
            .write_off(
                &customer,
                &DebtId("debt-a".to_string()),
                amount(40),
                date("2030-01-03"),
                WriteOffEvidence::new("已有事实支持无合理回收预期".to_string()).expect("核销依据"),
                write_off_event.clone(),
            )
            .expect("部分核销");
        assert_eq!(
            book.write_off(
                &customer,
                &DebtId("debt-a".to_string()),
                amount(40),
                date("2030-01-03"),
                WriteOffEvidence::new("已有事实支持无合理回收预期".to_string()).expect("核销依据"),
                write_off_event,
            )
            .expect("核销重试"),
            written_off
        );
        let debt = book
            .customer(&customer)
            .expect("客户")
            .debt(&DebtId("debt-a".to_string()))
            .expect("债务");
        assert_eq!(debt.ordinary_balance(), amount(60));
        assert_eq!(debt.legal_balance(), amount(100));
        assert_eq!(debt.written_off_recoverable(), amount(40));

        let recovery_event = CustomerFinanceEventId("recovery-1".to_string());
        book.recover_written_off(
            &customer,
            &DebtId("debt-a".to_string()),
            CashEndpoint::External("company-1".to_string()),
            amount(25),
            date("2030-01-04"),
            recovery_event.clone(),
        )
        .expect("核销后回收");
        book.recover_written_off(
            &customer,
            &DebtId("debt-a".to_string()),
            CashEndpoint::External("company-1".to_string()),
            amount(25),
            date("2030-01-04"),
            recovery_event,
        )
        .expect("回收重试");
        assert_eq!(book.cash_flows().len(), 1);
        let debt = book
            .customer(&customer)
            .expect("客户")
            .debt(&DebtId("debt-a".to_string()))
            .expect("债务");
        assert_eq!(debt.ordinary_balance(), amount(60));
        assert_eq!(debt.legal_balance(), amount(75));
        assert_eq!(debt.written_off_recoverable(), amount(15));

        book.repay_due(
            &customer,
            CashEndpoint::External("company-1".to_string()),
            date("2030-01-05"),
            CustomerFinanceEventId("ordinary-repayment".to_string()),
        )
        .expect("清偿未核销应收");
        let debt = book
            .customer(&customer)
            .expect("客户")
            .debt(&DebtId("debt-a".to_string()))
            .expect("债务");
        assert_eq!(debt.ordinary_balance(), amount(0));
        assert_eq!(debt.legal_balance(), amount(15));
        assert_eq!(debt.written_off_recoverable(), amount(15));
        assert_eq!(
            debt.overdue_since(date("2030-01-05")),
            Some(date("2030-01-02"))
        );
        let before = book.clone();
        let excessive = book.recover_written_off(
            &customer,
            &DebtId("debt-a".to_string()),
            CashEndpoint::External("company-1".to_string()),
            amount(16),
            date("2030-01-05"),
            CustomerFinanceEventId("recovery-2".to_string()),
        );
        assert!(matches!(
            excessive,
            Err(CustomerFinanceError::RecoveryExceedsWrittenOff { .. })
        ));
        assert_eq!(book, before);
    }
}
