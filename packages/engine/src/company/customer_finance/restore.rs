use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;

use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer};

use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BookFacts {
    #[serde(deserialize_with = "unique_map")]
    customers: BTreeMap<CounterpartyId, CustomerFacts>,
    cash_flows: Vec<CustomerCashFlow>,
    #[serde(deserialize_with = "unique_map")]
    events: BTreeMap<CustomerFinanceEventId, AppliedEvent>,
    next_fact_sequence: u64,
    #[serde(deserialize_with = "required_optional_date")]
    last_event_date: Option<CivilDate>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CustomerFacts {
    cash: AccountingAmount,
    #[serde(deserialize_with = "unique_map")]
    debts: BTreeMap<DebtId, DebtFacts>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DebtFacts {
    id: DebtId,
    creditor: DebtCreditor,
    opened_on: CivilDate,
    due_on: CivilDate,
    principal: AccountingAmount,
    paid: AccountingAmount,
    written_off: AccountingAmount,
    recovered: AccountingAmount,
    registration_sequence: u64,
}

impl<'de> Deserialize<'de> for CustomerFinanceBook {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let facts = BookFacts::deserialize(deserializer)?;
        let book = Self {
            customers: facts
                .customers
                .into_iter()
                .map(|(id, customer)| {
                    let debts = customer
                        .debts
                        .into_iter()
                        .map(|(id, debt)| {
                            (
                                id,
                                CustomerDebt {
                                    id: debt.id,
                                    creditor: debt.creditor,
                                    opened_on: debt.opened_on,
                                    due_on: debt.due_on,
                                    principal: debt.principal,
                                    paid: debt.paid,
                                    written_off: debt.written_off,
                                    recovered: debt.recovered,
                                    registration_sequence: debt.registration_sequence,
                                },
                            )
                        })
                        .collect();
                    (
                        id,
                        CustomerFinanceState {
                            cash: customer.cash,
                            debts,
                        },
                    )
                })
                .collect(),
            cash_flows: facts.cash_flows,
            events: facts.events,
            next_fact_sequence: facts.next_fact_sequence,
            last_event_date: facts.last_event_date,
        };
        book.validate().map_err(serde::de::Error::custom)?;
        Ok(book)
    }
}

impl CustomerFinanceBook {
    /// 验证当前完整账簿的继续运行不变量，不以历史流水反推当前现金。
    pub fn validate(&self) -> Result<(), CustomerFinanceError> {
        let mut sequences = BTreeMap::new();
        let mut latest_date = None;
        for (customer_id, customer) in &self.customers {
            validate_id(&customer_id.0, "customer")?;
            if customer.cash.is_negative() {
                return Err(CustomerFinanceError::NegativeCash {
                    customer: customer_id.clone(),
                    cash: customer.cash,
                });
            }
            for (debt_id, debt) in &customer.debts {
                validate_id(&debt_id.0, "debt")?;
                debt.creditor.validate()?;
                if debt_id != &debt.id
                    || !debt.principal.is_positive()
                    || debt.paid.is_negative()
                    || debt.written_off.is_negative()
                    || debt.recovered.is_negative()
                    || debt.paid > debt.principal
                    || debt.written_off > debt.principal.sub(debt.paid)?
                    || debt.recovered > debt.written_off
                {
                    return Err(invalid(format!(
                        "invalid balances or identity for {customer_id:?}/{debt_id:?}"
                    )));
                }
                if debt.due_on < debt.opened_on {
                    return Err(CustomerFinanceError::DueBeforeOpen {
                        opened_on: debt.opened_on,
                        due_on: debt.due_on,
                    });
                }
                if debt.registration_sequence >= self.next_fact_sequence
                    || sequences
                        .insert(debt.registration_sequence, debt.opened_on)
                        .is_some()
                {
                    return Err(invalid(format!(
                        "invalid debt sequence for {customer_id:?}/{debt_id:?}"
                    )));
                }
                latest_date = Some(latest_date.map_or(debt.opened_on, |previous: CivilDate| {
                    previous.max(debt.opened_on)
                }));
            }
        }
        let mut previous_opening = None;
        for opened_on in sequences.values() {
            if previous_opening.is_some_and(|previous| previous > *opened_on) {
                return Err(invalid("debt registration dates regress".to_string()));
            }
            previous_opening = Some(*opened_on);
        }
        let mut expected_flows = Vec::new();
        let mut debt_totals: BTreeMap<
            (CounterpartyId, DebtId),
            (AccountingAmount, AccountingAmount, AccountingAmount),
        > = BTreeMap::new();
        let mut ordered_events: Vec<_> = self.events.iter().collect();
        ordered_events.sort_by_key(|(_, applied)| applied.sequence);
        for (event_id, applied) in ordered_events {
            validate_event_id(event_id)?;
            let mut flows = Vec::new();
            let event_date = match (&applied.command, &applied.result) {
                (
                    FinanceCommand::CashFlow {
                        source,
                        destination,
                        amount,
                        date,
                    },
                    FinanceResult::Flow,
                ) => {
                    validate_endpoints(&self.customers, source, destination, *amount)?;
                    flows.push(flow(
                        event_id,
                        *date,
                        source.clone(),
                        destination.clone(),
                        *amount,
                    ));
                    *date
                }
                (
                    FinanceCommand::RepayDue { customer, date },
                    FinanceResult::Payments(payments),
                ) => {
                    let state = self.customers.get(customer).ok_or_else(|| {
                        CustomerFinanceError::UnknownCustomer {
                            customer: customer.clone(),
                        }
                    })?;
                    let mut eligible = Vec::new();
                    for debt in state.debts.values() {
                        let (paid, written_off, _) =
                            recorded_totals(&debt_totals, customer, &debt.id);
                        let ordinary = debt.principal.sub(paid)?.sub(written_off)?;
                        if debt.registration_sequence < applied.sequence
                            && debt.due_on <= *date
                            && ordinary.is_positive()
                        {
                            eligible.push((debt, ordinary));
                        }
                    }
                    eligible.sort_by_key(|(debt, _)| (debt.due_on, debt.registration_sequence));
                    if payments.len() > eligible.len() {
                        return Err(invalid(format!(
                            "too many repayment receipts for {event_id:?}"
                        )));
                    }
                    let mut payment_total = AccountingAmount::ZERO;
                    for (index, payment) in payments.iter().enumerate() {
                        let (debt, ordinary) = eligible[index];
                        if !payment.amount.is_positive()
                            || payment.debt != debt.id
                            || payment.creditor != debt.creditor
                            || payment.amount > ordinary
                            || (index + 1 < payments.len() && payment.amount != ordinary)
                            || payment.original_due_on != debt.due_on
                            || payment.overdue_since != (*date > debt.due_on).then_some(debt.due_on)
                        {
                            return Err(invalid(format!(
                                "invalid repayment receipt for {event_id:?}"
                            )));
                        }
                        payment_total = payment_total.add(payment.amount)?;
                        let totals = debt_totals
                            .entry((customer.clone(), payment.debt.clone()))
                            .or_insert((
                                AccountingAmount::ZERO,
                                AccountingAmount::ZERO,
                                AccountingAmount::ZERO,
                            ));
                        totals.0 = totals.0.add(payment.amount)?;
                        flows.push(flow(
                            event_id,
                            *date,
                            CashEndpoint::Customer(customer.clone()),
                            debt.creditor.cash_endpoint(),
                            payment.amount,
                        ));
                    }
                    *date
                }
                (
                    FinanceCommand::WriteOff {
                        customer,
                        debt: debt_id,
                        amount,
                        date,
                        evidence,
                    },
                    FinanceResult::WriteOff(result),
                ) => {
                    let debt = self.debt_or_err(customer, debt_id)?;
                    let (paid, written_off, recovered) =
                        recorded_totals(&debt_totals, customer, debt_id);
                    let ordinary = debt.principal.sub(paid)?.sub(written_off)?;
                    let legal = debt.principal.sub(paid)?.sub(recovered)?;
                    if !amount.is_positive()
                        || debt.registration_sequence >= applied.sequence
                        || *date < debt.opened_on
                        || *amount > ordinary
                        || result.debt != *debt_id
                        || result.amount != *amount
                        || result.legal_balance_after != legal
                    {
                        return Err(invalid(format!(
                            "invalid write-off receipt for {event_id:?}"
                        )));
                    }
                    WriteOffEvidence::new(evidence.description().to_string())?;
                    let totals = debt_totals
                        .entry((customer.clone(), debt_id.clone()))
                        .or_insert((
                            AccountingAmount::ZERO,
                            AccountingAmount::ZERO,
                            AccountingAmount::ZERO,
                        ));
                    totals.1 = totals.1.add(*amount)?;
                    *date
                }
                (
                    FinanceCommand::Recover {
                        customer,
                        debt: debt_id,
                        amount,
                        date,
                    },
                    FinanceResult::Recovered,
                ) => {
                    let debt = self.debt_or_err(customer, debt_id)?;
                    let (_, written_off, recovered) =
                        recorded_totals(&debt_totals, customer, debt_id);
                    if *date < debt.opened_on
                        || debt.registration_sequence >= applied.sequence
                        || *amount > written_off.sub(recovered)?
                    {
                        return Err(invalid(format!(
                            "recovery before debt opening for {event_id:?}"
                        )));
                    }
                    validate_endpoints(
                        &self.customers,
                        &CashEndpoint::Customer(customer.clone()),
                        &debt.creditor.cash_endpoint(),
                        *amount,
                    )?;
                    let totals = debt_totals
                        .entry((customer.clone(), debt_id.clone()))
                        .or_insert((
                            AccountingAmount::ZERO,
                            AccountingAmount::ZERO,
                            AccountingAmount::ZERO,
                        ));
                    totals.2 = totals.2.add(*amount)?;
                    flows.push(flow(
                        event_id,
                        *date,
                        CashEndpoint::Customer(customer.clone()),
                        debt.creditor.cash_endpoint(),
                        *amount,
                    ));
                    *date
                }
                _ => return Err(invalid(format!("command/result mismatch for {event_id:?}"))),
            };
            if applied.sequence >= self.next_fact_sequence
                || sequences.insert(applied.sequence, event_date).is_some()
            {
                return Err(invalid(format!("invalid event sequence for {event_id:?}")));
            }
            latest_date = Some(
                latest_date.map_or(event_date, |previous: CivilDate| previous.max(event_date)),
            );
            expected_flows.extend(flows);
        }
        let mut previous_date = None;
        for (expected_sequence, (sequence, fact_date)) in sequences.iter().enumerate() {
            let expected_sequence = u64::try_from(expected_sequence)
                .map_err(|_| CustomerFinanceError::SequenceOverflow)?;
            if *sequence != expected_sequence
                || previous_date.is_some_and(|previous| previous > *fact_date)
            {
                return Err(invalid(
                    "customer finance fact sequence has gaps or dates regress".to_string(),
                ));
            }
            previous_date = Some(*fact_date);
        }
        if u64::try_from(sequences.len()).map_err(|_| CustomerFinanceError::SequenceOverflow)?
            != self.next_fact_sequence
        {
            return Err(invalid(
                "next fact sequence does not match complete facts".to_string(),
            ));
        }
        for (customer_id, customer) in &self.customers {
            for (debt_id, debt) in &customer.debts {
                let totals = match debt_totals.get(&(customer_id.clone(), debt_id.clone())) {
                    Some(totals) => *totals,
                    None => (
                        AccountingAmount::ZERO,
                        AccountingAmount::ZERO,
                        AccountingAmount::ZERO,
                    ),
                };
                if totals != (debt.paid, debt.written_off, debt.recovered) {
                    return Err(invalid(format!(
                        "debt/event totals mismatch for {customer_id:?}/{debt_id:?}"
                    )));
                }
            }
        }
        if self.cash_flows != expected_flows {
            return Err(invalid(
                "cash flows do not match applied fact order and receipts".to_string(),
            ));
        }
        if self.last_event_date != latest_date {
            return Err(invalid(
                "last event date does not match latest fact".to_string(),
            ));
        }
        Ok(())
    }
}

fn recorded_totals(
    totals: &BTreeMap<
        (CounterpartyId, DebtId),
        (AccountingAmount, AccountingAmount, AccountingAmount),
    >,
    customer: &CounterpartyId,
    debt: &DebtId,
) -> (AccountingAmount, AccountingAmount, AccountingAmount) {
    match totals.get(&(customer.clone(), debt.clone())) {
        Some(totals) => *totals,
        None => (
            AccountingAmount::ZERO,
            AccountingAmount::ZERO,
            AccountingAmount::ZERO,
        ),
    }
}

fn invalid(detail: String) -> CustomerFinanceError {
    CustomerFinanceError::InvalidRestore { detail }
}

pub(super) fn required_optional_date<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<CivilDate>, D::Error> {
    Option::<CivilDate>::deserialize(deserializer)
}

fn flow(
    event: &CustomerFinanceEventId,
    date: CivilDate,
    source: CashEndpoint,
    destination: CashEndpoint,
    amount: AccountingAmount,
) -> CustomerCashFlow {
    CustomerCashFlow {
        event: event.clone(),
        date,
        source,
        destination,
        amount,
    }
}

fn unique_map<'de, D, Key, Value>(deserializer: D) -> Result<BTreeMap<Key, Value>, D::Error>
where
    D: Deserializer<'de>,
    Key: Deserialize<'de> + Ord + fmt::Debug,
    Value: Deserialize<'de>,
{
    struct UniqueMap<Key, Value>(PhantomData<(Key, Value)>);
    impl<'de, Key, Value> Visitor<'de> for UniqueMap<Key, Value>
    where
        Key: Deserialize<'de> + Ord + fmt::Debug,
        Value: Deserialize<'de>,
    {
        type Value = BTreeMap<Key, Value>;
        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a map with unique keys")
        }
        fn visit_map<Access: MapAccess<'de>>(
            self,
            mut access: Access,
        ) -> Result<Self::Value, Access::Error> {
            let mut entries = BTreeMap::new();
            while let Some((key, value)) = access.next_entry()? {
                if entries.contains_key(&key) {
                    return Err(serde::de::Error::custom(format!(
                        "duplicate customer finance key {key:?}"
                    )));
                }
                entries.insert(key, value);
            }
            Ok(entries)
        }
    }
    deserializer.deserialize_map(UniqueMap(PhantomData))
}
