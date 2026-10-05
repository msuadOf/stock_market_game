use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::{
    share_registry::{HolderId, RegistrationSnapshot, ShareRegistryError},
    CompanyId,
};
use crate::{
    account::StockCode,
    calendar::{CalendarExchange, CivilDate, TradingCalendar},
    money::Money,
};

/// 已经形成明确批准事实的单次现金分红输入。
///
/// `approved_on` 表示股东会分配决议日期，并用于检验《公司法》第212条的六个月期限。
/// `distributable_amount` 必须是上层校验亏损弥补、法定公积金等条件后提供的授权额度；它不是现金余额，也不在此推导。
/// 本模块采用方案输入顺序约束 `approved_on <= announced_on <= registered_on < ex_dividend_on <= payable_on`，该顺序本身不宣称为完整法定时序。
/// `gross_per_share` 只接受 `Money` 可表达的整数分，不推导或舍入不足一分的方案；调用方须在进入本模块前解决适用精度事实。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "CashDividendPlanState")]
#[serde(deny_unknown_fields)]
pub struct CashDividendPlan {
    pub plan_id: String,
    pub issuer: CompanyId,
    pub stock: StockCode,
    pub exchange: CalendarExchange,
    pub approved_on: CivilDate,
    pub announced_on: CivilDate,
    pub registered_on: CivilDate,
    pub ex_dividend_on: CivilDate,
    pub payable_on: CivilDate,
    pub gross_per_share: Money,
    pub distributable_amount: Money,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CashDividendPlanState {
    plan_id: String,
    issuer: CompanyId,
    stock: StockCode,
    exchange: CalendarExchange,
    approved_on: CivilDate,
    announced_on: CivilDate,
    registered_on: CivilDate,
    ex_dividend_on: CivilDate,
    payable_on: CivilDate,
    gross_per_share: Money,
    distributable_amount: Money,
}

impl TryFrom<CashDividendPlanState> for CashDividendPlan {
    type Error = CashDividendError;

    fn try_from(state: CashDividendPlanState) -> Result<Self, Self::Error> {
        let plan = Self {
            plan_id: state.plan_id,
            issuer: state.issuer,
            stock: state.stock,
            exchange: state.exchange,
            approved_on: state.approved_on,
            announced_on: state.announced_on,
            registered_on: state.registered_on,
            ex_dividend_on: state.ex_dividend_on,
            payable_on: state.payable_on,
            gross_per_share: state.gross_per_share,
            distributable_amount: state.distributable_amount,
        };
        plan.validate()?;
        Ok(plan)
    }
}

impl CashDividendPlan {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        plan_id: String,
        issuer: CompanyId,
        stock: StockCode,
        exchange: CalendarExchange,
        approved_on: CivilDate,
        announced_on: CivilDate,
        registered_on: CivilDate,
        ex_dividend_on: CivilDate,
        payable_on: CivilDate,
        gross_per_share: Money,
        distributable_amount: Money,
        calendar: &TradingCalendar,
    ) -> Result<Self, CashDividendError> {
        let plan = Self {
            plan_id,
            issuer,
            stock,
            exchange,
            approved_on,
            announced_on,
            registered_on,
            ex_dividend_on,
            payable_on,
            gross_per_share,
            distributable_amount,
        };
        plan.validate()?;
        plan.validate_calendar(calendar)?;
        Ok(plan)
    }

    pub fn validate(&self) -> Result<(), CashDividendError> {
        if self.plan_id.trim().is_empty()
            || self.issuer.0.trim().is_empty()
            || self.stock.0.trim().is_empty()
            || self.gross_per_share.cents() <= 0
            || self.distributable_amount.cents() < 0
            || self.approved_on > self.announced_on
            || self.announced_on > self.registered_on
            || self.registered_on >= self.ex_dividend_on
            || self.ex_dividend_on > self.payable_on
            || !within_six_months(self.approved_on, self.payable_on)
        {
            return Err(CashDividendError::InvalidPlan {
                detail: "identities, positive cent amount, nonnegative distributable amount and ordered dates are required".into(),
            });
        }
        Ok(())
    }

    pub fn validate_calendar(&self, calendar: &TradingCalendar) -> Result<(), CashDividendError> {
        let is_registration_day = calendar
            .is_trading_day(self.exchange, self.registered_on)
            .map_err(|error| CashDividendError::CalendarLookup {
                detail: error.to_string(),
            })?;
        if !is_registration_day {
            return Err(CashDividendError::InvalidTradingDate {
                date: self.registered_on,
                exchange: self.exchange,
            });
        }
        let next_trading_day = calendar
            .next_trading_day(self.exchange, self.registered_on)
            .map_err(|error| CashDividendError::CalendarLookup {
                detail: error.to_string(),
            })?;
        if next_trading_day != self.ex_dividend_on {
            return Err(CashDividendError::InvalidExDividendDate {
                expected: next_trading_day,
                actual: self.ex_dividend_on,
                exchange: self.exchange,
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CashDividendEntitlement {
    pub holder: HolderId,
    pub shares: u64,
    pub gross: Money,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum HolderPaymentOutcome {
    Paid { holder: HolderId, amount: Money },
    Failed { holder: HolderId, reason: String },
}

impl HolderPaymentOutcome {
    pub fn holder(&self) -> &HolderId {
        match self {
            Self::Paid { holder, .. } | Self::Failed { holder, .. } => holder,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CashDividendPaymentReceipt {
    payment_id: String,
    paid_on: CivilDate,
    within_six_month_deadline: bool,
    outcomes: Vec<HolderPaymentOutcome>,
}

impl CashDividendPaymentReceipt {
    pub fn payment_id(&self) -> &str {
        &self.payment_id
    }
    pub fn paid_on(&self) -> CivilDate {
        self.paid_on
    }
    pub fn within_six_month_deadline(&self) -> bool {
        self.within_six_month_deadline
    }
    pub fn outcomes(&self) -> &[HolderPaymentOutcome] {
        &self.outcomes
    }
    pub fn failed_holders(&self) -> Vec<HolderId> {
        self.outcomes
            .iter()
            .filter_map(|outcome| match outcome {
                HolderPaymentOutcome::Failed { holder, .. } => Some(holder.clone()),
                HolderPaymentOutcome::Paid { .. } => None,
            })
            .collect()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum CashDividendStatus {
    Approved,
    Announced,
    Registered,
    Payable,
    PartiallyPaid,
    Paid,
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum CashDividendError {
    #[error("cash dividend plan: {detail}")]
    InvalidPlan { detail: String },
    #[error("cash dividend operation is invalid in status {status:?}: {operation}")]
    WrongStage {
        status: CashDividendStatus,
        operation: &'static str,
    },
    #[error("registration snapshot does not match plan: {detail}")]
    SnapshotMismatch { detail: String },
    #[error("gross entitlement {required:?} exceeds distributable amount {available:?}")]
    ExceedsDistributableAmount { required: Money, available: Money },
    #[error("registration snapshot contains no dividend-eligible holders")]
    NoEligibleHolders,
    #[error("cash dividend arithmetic overflow while calculating {operation}")]
    Overflow { operation: &'static str },
    #[error("trading calendar lookup failed: {detail}")]
    CalendarLookup { detail: String },
    #[error("registration date {date} is not a {exchange:?} trading day")]
    InvalidTradingDate {
        date: CivilDate,
        exchange: CalendarExchange,
    },
    #[error("ex-dividend date {actual} is not the next {exchange:?} trading day {expected}")]
    InvalidExDividendDate {
        expected: CivilDate,
        actual: CivilDate,
        exchange: CalendarExchange,
    },
    #[error("payment result set is incomplete; missing {holders:?}")]
    IncompletePaymentResults { holders: Vec<HolderId> },
    #[error("payment result contains a holder with no outstanding entitlement: {holder:?}")]
    UnexpectedPaymentHolder { holder: HolderId },
    #[error("payment result amount for {holder:?} is {actual:?}, expected {expected:?}")]
    PaymentAmountMismatch {
        holder: HolderId,
        expected: Money,
        actual: Money,
    },
    #[error("failed payment for {holder:?} requires a nonempty reason")]
    MissingFailureReason { holder: HolderId },
    #[error("payment event identity reused with different input: {payment_id}")]
    PaymentIdentityConflict { payment_id: String },
    #[error("payment event date {actual} precedes prior recorded payment date {previous}")]
    PaymentDateRegression {
        previous: CivilDate,
        actual: CivilDate,
    },
    #[error("cash dividend has already been paid in full")]
    AlreadyPaid,
    #[error("share registry error: {0}")]
    ShareRegistry(#[from] ShareRegistryError),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "CashDividendBookState")]
pub struct CashDividendBook {
    plan: CashDividendPlan,
    status: CashDividendStatus,
    registration: Option<RegistrationSnapshot>,
    entitlements: Vec<CashDividendEntitlement>,
    paid: Vec<(HolderId, Money)>,
    failures: Vec<(HolderId, String)>,
    payments: Vec<CashDividendPaymentReceipt>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CashDividendBookState {
    plan: CashDividendPlan,
    status: CashDividendStatus,
    #[serde(deserialize_with = "required_nullable_registration")]
    registration: Option<RegistrationSnapshot>,
    entitlements: Vec<CashDividendEntitlement>,
    paid: Vec<(HolderId, Money)>,
    failures: Vec<(HolderId, String)>,
    payments: Vec<CashDividendPaymentReceipt>,
}

impl TryFrom<CashDividendBookState> for CashDividendBook {
    type Error = CashDividendError;

    fn try_from(state: CashDividendBookState) -> Result<Self, Self::Error> {
        let book = Self {
            plan: state.plan,
            status: state.status,
            registration: state.registration,
            entitlements: state.entitlements,
            paid: state.paid,
            failures: state.failures,
            payments: state.payments,
        };
        book.validate()?;
        Ok(book)
    }
}

impl CashDividendBook {
    pub fn new(plan: CashDividendPlan) -> Result<Self, CashDividendError> {
        plan.validate()?;
        Ok(Self {
            plan,
            status: CashDividendStatus::Approved,
            registration: None,
            entitlements: Vec::new(),
            paid: Vec::new(),
            failures: Vec::new(),
            payments: Vec::new(),
        })
    }

    pub fn plan(&self) -> &CashDividendPlan {
        &self.plan
    }
    pub fn status(&self) -> &CashDividendStatus {
        &self.status
    }
    pub fn registration(&self) -> Option<&RegistrationSnapshot> {
        self.registration.as_ref()
    }
    pub fn entitlements(&self) -> Result<&[CashDividendEntitlement], CashDividendError> {
        if self.registration.is_none() {
            return Err(CashDividendError::WrongStage {
                status: self.status.clone(),
                operation: "read entitlements before registration",
            });
        }
        Ok(&self.entitlements)
    }
    pub fn total_gross(&self) -> Result<Money, CashDividendError> {
        self.entitlements
            .iter()
            .try_fold(Money::ZERO, |total, item| {
                total
                    .add(item.gross)
                    .map_err(|_| CashDividendError::Overflow {
                        operation: "total gross entitlement",
                    })
            })
    }
    pub fn payments(&self) -> &[CashDividendPaymentReceipt] {
        &self.payments
    }
    pub fn successful_payments(&self) -> &[(HolderId, Money)] {
        &self.paid
    }
    pub fn unresolved_failures(&self) -> &[(HolderId, String)] {
        &self.failures
    }
    pub fn paid_amount(&self) -> Result<Money, CashDividendError> {
        self.paid
            .iter()
            .try_fold(Money::ZERO, |total, (_, amount)| {
                total.add(*amount).map_err(|_| CashDividendError::Overflow {
                    operation: "paid dividend total",
                })
            })
    }
    pub fn outstanding_amount(&self) -> Result<Money, CashDividendError> {
        self.total_gross()?
            .sub(self.paid_amount()?)
            .map_err(|_| CashDividendError::Overflow {
                operation: "outstanding dividend total",
            })
    }

    pub fn announce(&mut self, on: CivilDate) -> Result<(), CashDividendError> {
        if self.status != CashDividendStatus::Approved {
            return if on == self.plan.announced_on {
                Ok(())
            } else {
                Err(CashDividendError::WrongStage {
                    status: self.status.clone(),
                    operation: "announce on a conflicting date",
                })
            };
        }
        if self.status != CashDividendStatus::Approved || on != self.plan.announced_on {
            return Err(CashDividendError::WrongStage {
                status: self.status.clone(),
                operation: "announce",
            });
        }
        self.status = CashDividendStatus::Announced;
        Ok(())
    }

    pub fn register(
        &mut self,
        snapshot: RegistrationSnapshot,
        calendar: &TradingCalendar,
    ) -> Result<&[CashDividendEntitlement], CashDividendError> {
        self.plan.validate_calendar(calendar)?;
        if let Some(existing) = &self.registration {
            if existing == &snapshot {
                return Ok(&self.entitlements);
            }
            return Err(CashDividendError::SnapshotMismatch {
                detail: "registration was already frozen for this plan".into(),
            });
        }
        if self.status != CashDividendStatus::Announced {
            return Err(CashDividendError::WrongStage {
                status: self.status.clone(),
                operation: "register",
            });
        }
        snapshot.validate()?;
        if snapshot.event_id() != self.plan.plan_id
            || snapshot.stock() != &self.plan.stock
            || snapshot.issuer() != &self.plan.issuer
            || snapshot.registered_on() != self.plan.registered_on
        {
            return Err(CashDividendError::SnapshotMismatch {
                detail: "snapshot plan identity, issuer, security or registration date differs"
                    .into(),
            });
        }
        let entitlements = entitlements_for(&self.plan, &snapshot)?;
        if entitlements.is_empty() {
            return Err(CashDividendError::NoEligibleHolders);
        }
        let total = entitlements.iter().try_fold(Money::ZERO, |sum, item| {
            sum.add(item.gross)
                .map_err(|_| CashDividendError::Overflow {
                    operation: "total gross entitlement",
                })
        })?;
        if total.cents() > self.plan.distributable_amount.cents() {
            return Err(CashDividendError::ExceedsDistributableAmount {
                required: total,
                available: self.plan.distributable_amount,
            });
        }
        self.registration = Some(snapshot);
        self.entitlements = entitlements;
        self.status = CashDividendStatus::Registered;
        Ok(&self.entitlements)
    }

    pub fn mark_payable(&mut self, on: CivilDate) -> Result<(), CashDividendError> {
        if self.status != CashDividendStatus::Registered {
            return if matches!(
                self.status,
                CashDividendStatus::Payable
                    | CashDividendStatus::PartiallyPaid
                    | CashDividendStatus::Paid
            ) && on >= self.plan.payable_on
            {
                Ok(())
            } else {
                Err(CashDividendError::WrongStage {
                    status: self.status.clone(),
                    operation: "mark payable",
                })
            };
        }
        if self.status != CashDividendStatus::Registered || on < self.plan.payable_on {
            return Err(CashDividendError::WrongStage {
                status: self.status.clone(),
                operation: "mark payable",
            });
        }
        self.status = CashDividendStatus::Payable;
        Ok(())
    }

    /// 记录外部付款执行者逐持有人返回的结果；本方法不转移公司或账户现金。
    /// 付款执行者须把成功收款与本状态变更放入同一候选事务后原子提交。
    pub fn settle(
        &mut self,
        payment_id: String,
        paid_on: CivilDate,
        mut outcomes: Vec<HolderPaymentOutcome>,
    ) -> Result<CashDividendPaymentReceipt, CashDividendError> {
        if payment_id.trim().is_empty() {
            return Err(CashDividendError::PaymentIdentityConflict { payment_id });
        }
        outcomes.sort_by(|left, right| left.holder().cmp(right.holder()));
        if let Some(receipt) = self
            .payments
            .iter()
            .find(|item| item.payment_id == payment_id)
        {
            if receipt.paid_on == paid_on && receipt.outcomes == outcomes {
                return Ok(receipt.clone());
            }
            return Err(CashDividendError::PaymentIdentityConflict { payment_id });
        }
        if let Some(previous) = self.payments.last().map(|receipt| receipt.paid_on) {
            if paid_on < previous {
                return Err(CashDividendError::PaymentDateRegression {
                    previous,
                    actual: paid_on,
                });
            }
        }
        if self.status == CashDividendStatus::Paid {
            return Err(CashDividendError::AlreadyPaid);
        }
        if !matches!(
            self.status,
            CashDividendStatus::Payable | CashDividendStatus::PartiallyPaid
        ) || paid_on < self.plan.payable_on
        {
            return Err(CashDividendError::WrongStage {
                status: self.status.clone(),
                operation: "settle before payable",
            });
        }
        let outstanding: BTreeMap<_, _> = self
            .entitlements
            .iter()
            .filter(|item| !self.paid.iter().any(|(holder, _)| holder == &item.holder))
            .map(|item| (item.holder.clone(), item.gross))
            .collect();
        let mut seen = BTreeSet::new();
        for outcome in &outcomes {
            let holder = outcome.holder();
            if !seen.insert(holder.clone()) {
                return Err(CashDividendError::UnexpectedPaymentHolder {
                    holder: holder.clone(),
                });
            }
            let expected = outstanding.get(holder).ok_or_else(|| {
                CashDividendError::UnexpectedPaymentHolder {
                    holder: holder.clone(),
                }
            })?;
            match outcome {
                HolderPaymentOutcome::Paid { amount, .. } if amount != expected => {
                    return Err(CashDividendError::PaymentAmountMismatch {
                        holder: holder.clone(),
                        expected: *expected,
                        actual: *amount,
                    });
                }
                HolderPaymentOutcome::Failed { reason, .. } if reason.trim().is_empty() => {
                    return Err(CashDividendError::MissingFailureReason {
                        holder: holder.clone(),
                    });
                }
                _ => {}
            }
        }
        let missing: Vec<_> = outstanding
            .keys()
            .filter(|holder| !seen.contains(*holder))
            .cloned()
            .collect();
        if !missing.is_empty() {
            return Err(CashDividendError::IncompletePaymentResults { holders: missing });
        }

        let receipt = CashDividendPaymentReceipt {
            payment_id,
            paid_on,
            within_six_month_deadline: within_six_months(self.plan.approved_on, paid_on),
            outcomes,
        };
        for outcome in &receipt.outcomes {
            match outcome {
                HolderPaymentOutcome::Paid { holder, amount } => {
                    self.paid.push((holder.clone(), *amount));
                    self.failures
                        .retain(|(failed_holder, _)| failed_holder != holder);
                }
                HolderPaymentOutcome::Failed { holder, reason } => {
                    self.failures
                        .retain(|(failed_holder, _)| failed_holder != holder);
                    self.failures.push((holder.clone(), reason.clone()));
                }
            }
        }
        self.payments.push(receipt.clone());
        self.status = if self.paid.len() == self.entitlements.len() {
            CashDividendStatus::Paid
        } else if self.paid.is_empty() {
            CashDividendStatus::Payable
        } else {
            CashDividendStatus::PartiallyPaid
        };
        Ok(receipt)
    }

    pub fn validate(&self) -> Result<(), CashDividendError> {
        self.plan.validate()?;
        let registered = self.registration.is_some();
        if registered
            != !matches!(
                self.status,
                CashDividendStatus::Approved | CashDividendStatus::Announced
            )
        {
            return Err(CashDividendError::SnapshotMismatch {
                detail: "status and frozen registration do not agree".into(),
            });
        }
        if let Some(snapshot) = &self.registration {
            snapshot.validate()?;
            if snapshot.event_id() != self.plan.plan_id
                || snapshot.stock() != &self.plan.stock
                || snapshot.issuer() != &self.plan.issuer
                || snapshot.registered_on() != self.plan.registered_on
            {
                return Err(CashDividendError::SnapshotMismatch {
                    detail: "restored registration does not match the plan".into(),
                });
            }
            let expected = entitlements_for(&self.plan, snapshot)?;
            if expected.is_empty() {
                return Err(CashDividendError::NoEligibleHolders);
            }
            if expected != self.entitlements {
                return Err(CashDividendError::SnapshotMismatch {
                    detail: "restored entitlement does not match immutable registration".into(),
                });
            }
            let total = self.total_gross()?;
            if total.cents() > self.plan.distributable_amount.cents() {
                return Err(CashDividendError::ExceedsDistributableAmount {
                    required: total,
                    available: self.plan.distributable_amount,
                });
            }
        } else if !self.entitlements.is_empty()
            || !self.paid.is_empty()
            || !self.failures.is_empty()
            || !self.payments.is_empty()
        {
            return Err(CashDividendError::SnapshotMismatch {
                detail: "unregistered dividend contains derived settlement facts".into(),
            });
        }
        let entitlement_by_holder: BTreeMap<_, _> = self
            .entitlements
            .iter()
            .map(|item| (&item.holder, item.gross))
            .collect();
        let mut serialized_paid_holders = BTreeSet::new();
        for (holder, amount) in &self.paid {
            if entitlement_by_holder.get(holder) != Some(amount)
                || !serialized_paid_holders.insert(holder)
                || self
                    .failures
                    .iter()
                    .any(|(failed_holder, _)| failed_holder == holder)
            {
                return Err(CashDividendError::SnapshotMismatch {
                    detail: "paid holder amount or outstanding failure is inconsistent".into(),
                });
            }
        }
        let mut serialized_failure_holders = BTreeSet::new();
        if self.failures.iter().any(|(holder, reason)| {
            !entitlement_by_holder.contains_key(holder)
                || serialized_paid_holders.contains(holder)
                || reason.trim().is_empty()
                || !serialized_failure_holders.insert(holder)
        }) {
            return Err(CashDividendError::SnapshotMismatch {
                detail: "failure facts require an outstanding entitled holder and reason".into(),
            });
        }
        let mut payment_ids = BTreeSet::new();
        let mut replayed_paid = BTreeMap::new();
        let mut replayed_failures = BTreeMap::new();
        let mut previous_payment_date = None;
        for payment in &self.payments {
            if payment.payment_id.trim().is_empty()
                || payment.paid_on < self.plan.payable_on
                || previous_payment_date.is_some_and(|previous| payment.paid_on < previous)
                || payment.within_six_month_deadline
                    != within_six_months(self.plan.approved_on, payment.paid_on)
                || !payment_ids.insert(&payment.payment_id)
                || payment
                    .outcomes
                    .windows(2)
                    .any(|pair| pair[0].holder() >= pair[1].holder())
            {
                return Err(CashDividendError::SnapshotMismatch {
                    detail: "payment receipts require unique identities and payable dates".into(),
                });
            }
            previous_payment_date = Some(payment.paid_on);
            let outstanding: BTreeMap<_, _> = self
                .entitlements
                .iter()
                .filter(|item| !replayed_paid.contains_key(&item.holder))
                .map(|item| (item.holder.clone(), item.gross))
                .collect();
            if payment.outcomes.len() != outstanding.len() {
                return Err(CashDividendError::SnapshotMismatch {
                    detail: "payment receipt does not cover each outstanding holder".into(),
                });
            }
            let mut receipt_holders = BTreeSet::new();
            for outcome in &payment.outcomes {
                let holder = outcome.holder();
                if !receipt_holders.insert(holder.clone()) {
                    return Err(CashDividendError::SnapshotMismatch {
                        detail: "payment receipt repeats a holder".into(),
                    });
                }
                let expected =
                    outstanding
                        .get(holder)
                        .ok_or_else(|| CashDividendError::SnapshotMismatch {
                            detail:
                                "payment receipt includes a holder without outstanding entitlement"
                                    .into(),
                        })?;
                match outcome {
                    HolderPaymentOutcome::Paid { amount, .. } if amount == expected => {
                        replayed_paid.insert(holder.clone(), *amount);
                        replayed_failures.remove(holder);
                    }
                    HolderPaymentOutcome::Paid { .. } => {
                        return Err(CashDividendError::SnapshotMismatch {
                            detail: "payment receipt amount differs from frozen entitlement".into(),
                        });
                    }
                    HolderPaymentOutcome::Failed { reason, .. } if !reason.trim().is_empty() => {
                        replayed_failures.insert(holder.clone(), reason.clone());
                    }
                    HolderPaymentOutcome::Failed { .. } => {
                        return Err(CashDividendError::SnapshotMismatch {
                            detail: "failed payment receipt has no reason".into(),
                        });
                    }
                }
            }
        }
        let replayed_paid: Vec<_> = replayed_paid.into_iter().collect();
        let replayed_failures: Vec<_> = replayed_failures.into_iter().collect();
        let mut actual_paid = self.paid.clone();
        let mut actual_failures = self.failures.clone();
        actual_paid.sort_by(|left, right| left.0.cmp(&right.0));
        actual_failures.sort_by(|left, right| left.0.cmp(&right.0));
        if replayed_paid != actual_paid || replayed_failures != actual_failures {
            return Err(CashDividendError::SnapshotMismatch {
                detail: "payment receipts do not reproduce paid and failed holder facts".into(),
            });
        }
        let expected_status = if !self.payments.is_empty()
            && self.paid.len() == self.entitlements.len()
            && registered
        {
            CashDividendStatus::Paid
        } else if !self.paid.is_empty() {
            CashDividendStatus::PartiallyPaid
        } else if !self.payments.is_empty() || self.status == CashDividendStatus::Payable {
            CashDividendStatus::Payable
        } else if registered {
            CashDividendStatus::Registered
        } else if self.status == CashDividendStatus::Announced {
            CashDividendStatus::Announced
        } else {
            CashDividendStatus::Approved
        };
        if self.status != expected_status {
            return Err(CashDividendError::SnapshotMismatch {
                detail: "status does not match recorded payment facts".into(),
            });
        }
        Ok(())
    }

    /// 使用调用方选择的权威日历校验恢复状态；日历不能从方案日期推断或静默采用默认值。
    pub fn validate_with_calendar(
        &self,
        calendar: &TradingCalendar,
    ) -> Result<(), CashDividendError> {
        self.validate()?;
        self.plan.validate_calendar(calendar)
    }
}

fn within_six_months(start: CivilDate, end: CivilDate) -> bool {
    let month_delta =
        (end.year() - start.year()) * 12 + i32::from(end.month()) - i32::from(start.month());
    month_delta < 6 || (month_delta == 6 && end.day() <= start.day())
}

fn required_nullable_registration<'de, D>(
    deserializer: D,
) -> Result<Option<RegistrationSnapshot>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<RegistrationSnapshot>::deserialize(deserializer)
}

fn entitlements_for(
    plan: &CashDividendPlan,
    snapshot: &RegistrationSnapshot,
) -> Result<Vec<CashDividendEntitlement>, CashDividendError> {
    let mut result = Vec::new();
    for holding in snapshot.entitled_holdings() {
        let shares = holding.lots.iter().try_fold(0u64, |total, lot| {
            total
                .checked_add(lot.qty)
                .ok_or(CashDividendError::Overflow {
                    operation: "holder registered share quantity",
                })
        })?;
        result.push(CashDividendEntitlement {
            holder: holding.holder.clone(),
            shares,
            gross: multiply_cents(plan.gross_per_share, shares)?,
        });
    }
    result.sort_by(|left, right| left.holder.cmp(&right.holder));
    Ok(result)
}

fn multiply_cents(amount: Money, shares: u64) -> Result<Money, CashDividendError> {
    let cents = i128::from(amount.cents())
        .checked_mul(i128::from(shares))
        .ok_or(CashDividendError::Overflow {
            operation: "gross entitlement multiplication",
        })?;
    let cents = i64::try_from(cents).map_err(|_| CashDividendError::Overflow {
        operation: "gross entitlement cents",
    })?;
    Ok(Money::from_cents(cents))
}

#[cfg(test)]
#[path = "cash_dividend/tests.rs"]
mod tests;
