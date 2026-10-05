use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{account::StockCode, calendar::CivilDate, money::Money, orderbook::AccountId};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum DividendTaxProfile {
    IndividualPublicMarket,
    ResidentEnterprise,
    SecuritiesFund,
    NonResident,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum StatutoryRestrictedBasis {
    FinanceTax2009167,
    FinanceTax201070,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum TaxShareClass {
    PublicMarket,
    StatutoryRestricted {
        release_on: CivilDate,
        basis: StatutoryRestrictedBasis,
        qualification_evidence: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum TaxAcquisitionSource {
    InitialAllocation { evidence: String },
    SecondaryMarket { settlement: String },
    CorporateAction { event: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DividendTaxLot {
    pub id: String,
    pub qty: u64,
    pub acquired_on: CivilDate,
    pub source: TaxAcquisitionSource,
    pub class: TaxShareClass,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ExactAmountState")]
pub struct ExactDividendTaxAmount {
    #[serde(with = "nonnegative_decimal")]
    numerator: i128,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    denominator: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExactAmountState {
    #[serde(with = "nonnegative_decimal")]
    numerator: i128,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    denominator: u64,
}

impl TryFrom<ExactAmountState> for ExactDividendTaxAmount {
    type Error = DividendTaxError;
    fn try_from(state: ExactAmountState) -> Result<Self, Self::Error> {
        let amount = Self::new(state.numerator, state.denominator)?;
        if amount.numerator != state.numerator || amount.denominator != state.denominator {
            return Err(invalid(
                "exact amount must be a reduced nonnegative fraction",
            ));
        }
        Ok(amount)
    }
}

impl ExactDividendTaxAmount {
    pub fn new(numerator: i128, denominator: u64) -> Result<Self, DividendTaxError> {
        if numerator < 0 || denominator == 0 {
            return Err(invalid("invalid exact tax amount"));
        }
        let divisor = gcd(numerator as u128, u128::from(denominator));
        Ok(Self {
            numerator: numerator / divisor as i128,
            denominator: denominator / divisor as u64,
        })
    }
    pub fn numerator(&self) -> i128 {
        self.numerator
    }
    pub fn denominator(&self) -> u64 {
        self.denominator
    }
    fn zero() -> Self {
        Self {
            numerator: 0,
            denominator: 1,
        }
    }
    fn add(&self, other: &Self) -> Result<Self, DividendTaxError> {
        let divisor = gcd(u128::from(self.denominator), u128::from(other.denominator)) as u64;
        let left_factor = other.denominator / divisor;
        let right_factor = self.denominator / divisor;
        let numerator = self
            .numerator
            .checked_mul(i128::from(left_factor))
            .and_then(|left| {
                other
                    .numerator
                    .checked_mul(i128::from(right_factor))
                    .and_then(|right| left.checked_add(right))
            })
            .ok_or_else(|| overflow("exact amount addition"))?;
        let denominator = self
            .denominator
            .checked_mul(left_factor)
            .ok_or_else(|| overflow("exact amount denominator"))?;
        Self::new(numerator, denominator)
    }
    fn subtract_cents(&self, cents: i128) -> Result<Self, DividendTaxError> {
        let numerator = self
            .numerator
            .checked_sub(
                cents
                    .checked_mul(i128::from(self.denominator))
                    .ok_or_else(|| overflow("paid tax scaling"))?,
            )
            .ok_or_else(|| overflow("paid tax subtraction"))?;
        Self::new(numerator, self.denominator)
    }
    fn multiply(&self, quantity: u64, percent: u8) -> Result<Self, DividendTaxError> {
        let mut numerator = self.numerator as u128;
        let mut quantity = u128::from(quantity);
        let mut percent = u128::from(percent);
        let mut denominator = u128::from(self.denominator);
        let mut hundred = 100u128;
        for factor in [&mut numerator, &mut quantity, &mut percent] {
            let divisor = gcd(*factor, denominator);
            *factor /= divisor;
            denominator /= divisor;
            let divisor = gcd(*factor, hundred);
            *factor /= divisor;
            hundred /= divisor;
        }
        let numerator = numerator
            .checked_mul(quantity)
            .and_then(|value| value.checked_mul(percent))
            .and_then(|value| i128::try_from(value).ok())
            .ok_or_else(|| overflow("exact tax multiplication"))?;
        let denominator = denominator
            .checked_mul(hundred)
            .and_then(|value| u64::try_from(value).ok())
            .ok_or_else(|| overflow("exact tax denominator"))?;
        Self::new(numerator, denominator)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum DividendTaxError {
    #[error("cash dividend tax: {0}")]
    InvalidFact(String),
    #[error("cash dividend tax: overflow in {operation}")]
    Overflow { operation: &'static str },
    #[error("cash dividend tax: unsupported taxpayer profile {profile:?}")]
    UnsupportedProfile { profile: DividendTaxProfile },
    #[error("cash dividend tax: mixed restricted tax lots require verified classification rules")]
    UnsupportedMixedRestrictedTaxLots,
    #[error("cash dividend tax: exact sub-cent liability needs verified rounding evidence")]
    NeedRoundingEvidence,
    #[error("cash dividend tax: natural month/year boundary has no corresponding calendar date")]
    NeedHoldingPeriodBoundaryEvidence,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TaxDisposition {
    lot: DividendTaxLot,
    disposed_on: CivilDate,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TaxDayReceipt {
    operation_seq: u64,
    event_id: String,
    day: CivilDate,
    net_change: i128,
    #[serde(deserialize_with = "required_lot")]
    acquisition: Option<DividendTaxLot>,
    dispositions: Vec<TaxDisposition>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DividendCashReceipt {
    operation_seq: u64,
    payment_id: String,
    paid_on: CivilDate,
    received_gross: Money,
    evidence: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegisteredTaxDividend {
    operation_seq: u64,
    event_id: String,
    registered_on: CivilDate,
    per_share: ExactDividendTaxAmount,
    lots: Vec<DividendTaxLot>,
    payments: Vec<DividendCashReceipt>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaxCollectionReceipt {
    pub operation_seq: u64,
    pub event_id: String,
    pub day: CivilDate,
    pub available_cash: Money,
    pub collected: Money,
    pub remaining_cash: Money,
    pub outstanding: ExactDividendTaxAmount,
    pub needs_funds: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "TaxBookState")]
pub struct CashDividendTaxBook {
    operation_seq: u64,
    account: AccountId,
    stock: StockCode,
    profile: DividendTaxProfile,
    opened_on: CivilDate,
    opening_lots: Vec<DividendTaxLot>,
    settled_on: CivilDate,
    lots: Vec<DividendTaxLot>,
    days: Vec<TaxDayReceipt>,
    dividends: Vec<RegisteredTaxDividend>,
    collections: Vec<TaxCollectionReceipt>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TaxBookState {
    operation_seq: u64,
    account: AccountId,
    stock: StockCode,
    profile: DividendTaxProfile,
    opened_on: CivilDate,
    opening_lots: Vec<DividendTaxLot>,
    settled_on: CivilDate,
    lots: Vec<DividendTaxLot>,
    days: Vec<TaxDayReceipt>,
    dividends: Vec<RegisteredTaxDividend>,
    collections: Vec<TaxCollectionReceipt>,
}

impl TryFrom<TaxBookState> for CashDividendTaxBook {
    type Error = DividendTaxError;
    fn try_from(state: TaxBookState) -> Result<Self, Self::Error> {
        let book = Self {
            operation_seq: state.operation_seq,
            account: state.account,
            stock: state.stock,
            profile: state.profile,
            opened_on: state.opened_on,
            opening_lots: state.opening_lots,
            settled_on: state.settled_on,
            lots: state.lots,
            days: state.days,
            dividends: state.dividends,
            collections: state.collections,
        };
        book.validate()?;
        Ok(book)
    }
}

impl CashDividendTaxBook {
    pub fn new(
        account: AccountId,
        stock: StockCode,
        profile: DividendTaxProfile,
        settled_on: CivilDate,
        lots: Vec<DividendTaxLot>,
    ) -> Result<Self, DividendTaxError> {
        let book = Self {
            operation_seq: 0,
            account,
            stock,
            profile,
            opened_on: settled_on,
            opening_lots: lots.clone(),
            settled_on,
            lots,
            days: vec![],
            dividends: vec![],
            collections: vec![],
        };
        book.validate()?;
        Ok(book)
    }
    pub fn register_dividend(
        &mut self,
        event_id: String,
        registered_on: CivilDate,
        per_share: ExactDividendTaxAmount,
    ) -> Result<(), DividendTaxError> {
        if let Some(existing) = self
            .dividends
            .iter()
            .find(|event| event.event_id == event_id)
        {
            if existing.registered_on == registered_on && existing.per_share == per_share {
                return Ok(());
            }
            return Err(invalid(
                "dividend identity reused with different registration",
            ));
        }
        if event_id.trim().is_empty()
            || registered_on != self.settled_on
            || per_share.numerator == 0
            || self.lots.is_empty()
        {
            return Err(invalid(
                "dividend registration needs current completed day, held shares and positive rate",
            ));
        }
        let mut candidate = self.clone();
        let operation_seq = candidate.advance_operation()?;
        candidate.dividends.push(RegisteredTaxDividend {
            operation_seq,
            event_id,
            registered_on,
            per_share,
            lots: self.lots.clone(),
            payments: vec![],
        });
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }
    pub fn record_net_day(
        &mut self,
        event_id: String,
        day: CivilDate,
        net_change: i128,
        acquisition: Option<DividendTaxLot>,
    ) -> Result<(), DividendTaxError> {
        if let Some(existing) = self
            .days
            .iter()
            .find(|receipt| receipt.event_id == event_id)
        {
            if existing.day == day
                && existing.net_change == net_change
                && existing.acquisition == acquisition
            {
                return Ok(());
            }
            return Err(invalid("tax day identity reused with different facts"));
        }
        if event_id.trim().is_empty() || day.days_since(self.settled_on) != 1 {
            return Err(invalid(
                "tax day must follow previous completed natural day",
            ));
        }
        if (net_change > 0) != acquisition.is_some() {
            return Err(invalid(
                "only positive daily net acquisition has a new tax lot",
            ));
        }
        let mut candidate = self.clone();
        if let Some(lot) = &acquisition {
            validate_lot(lot, day)?;
            if lot.acquired_on != day
                || i128::from(lot.qty) != net_change
                || candidate.has_lot_id(&lot.id)
            {
                return Err(invalid(
                    "new tax lot date, quantity or identity conflicts with daily net acquisition",
                ));
            }
        }
        let dispositions =
            apply_net_change(&mut candidate.lots, day, net_change, acquisition.as_ref())?;
        let operation_seq = candidate.advance_operation()?;
        candidate.days.push(TaxDayReceipt {
            operation_seq,
            event_id,
            day,
            net_change,
            acquisition,
            dispositions,
        });
        candidate.settled_on = day;
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }
    pub fn record_payment(
        &mut self,
        event_id: &str,
        payment_id: String,
        paid_on: CivilDate,
        received_gross: Money,
        evidence: String,
    ) -> Result<(), DividendTaxError> {
        if let Some((dividend, existing)) = self.dividends.iter().find_map(|dividend| {
            dividend
                .payments
                .iter()
                .find(|existing| existing.payment_id == payment_id)
                .map(|existing| (dividend, existing))
        }) {
            if dividend.event_id == event_id
                && existing.paid_on == paid_on
                && existing.received_gross == received_gross
                && existing.evidence == evidence
            {
                return Ok(());
            }
            return Err(invalid(
                "payment identity reused with different actual receipt",
            ));
        }
        if paid_on != self.settled_on
            || received_gross.cents() <= 0
            || payment_id.trim().is_empty()
            || evidence.trim().is_empty()
        {
            return Err(invalid(
                "payment requires current completed date, positive real cash and receipt evidence",
            ));
        }
        let mut candidate = self.clone();
        let operation_seq = candidate.advance_operation()?;
        let receipt = DividendCashReceipt {
            operation_seq,
            payment_id,
            paid_on,
            received_gross,
            evidence,
        };
        let dividend = candidate
            .dividends
            .iter_mut()
            .find(|dividend| dividend.event_id == event_id)
            .ok_or_else(|| invalid("unknown registered dividend"))?;
        dividend.payments.push(receipt);
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }
    pub fn outstanding(&self) -> Result<ExactDividendTaxAmount, DividendTaxError> {
        let collected = self.collections.iter().try_fold(0i128, |total, receipt| {
            total
                .checked_add(i128::from(receipt.collected.cents()))
                .ok_or_else(|| overflow("collected tax sum"))
        })?;
        self.assessed_through(self.settled_on, self.operation_seq)?
            .subtract_cents(collected)
    }
    pub fn collect_due(
        &mut self,
        event_id: String,
        day: CivilDate,
        available_cash: Money,
    ) -> Result<TaxCollectionReceipt, DividendTaxError> {
        if let Some(existing) = self
            .collections
            .iter()
            .find(|receipt| receipt.event_id == event_id)
        {
            if existing.day == day && existing.available_cash == available_cash {
                return Ok(existing.clone());
            }
            return Err(invalid(
                "tax collection identity reused with different available cash",
            ));
        }
        if event_id.trim().is_empty() || day != self.settled_on || available_cash.cents() < 0 {
            return Err(invalid(
                "collection requires current date and nonnegative real available cash",
            ));
        }
        let due = self.outstanding()?;
        if due.denominator != 1 {
            return Err(DividendTaxError::NeedRoundingEvidence);
        }
        let cents = i64::try_from(due.numerator.min(i128::from(available_cash.cents())))
            .map_err(|_| overflow("tax collection cash"))?;
        let receipt = TaxCollectionReceipt {
            operation_seq: self
                .operation_seq
                .checked_add(1)
                .ok_or_else(|| overflow("operation sequence"))?,
            event_id,
            day,
            available_cash,
            collected: Money::from_cents(cents),
            remaining_cash: Money::from_cents(available_cash.cents() - cents),
            outstanding: due.subtract_cents(i128::from(cents))?,
            needs_funds: due.numerator > i128::from(cents),
        };
        let mut candidate = self.clone();
        candidate.advance_operation()?;
        candidate.collections.push(receipt.clone());
        candidate.validate()?;
        *self = candidate;
        Ok(receipt)
    }
    pub fn lots(&self) -> &[DividendTaxLot] {
        &self.lots
    }
    pub fn account(&self) -> AccountId {
        self.account
    }
    pub fn stock(&self) -> &StockCode {
        &self.stock
    }
    pub fn settled_on(&self) -> CivilDate {
        self.settled_on
    }
    fn has_lot_id(&self, id: &str) -> bool {
        self.lots.iter().any(|lot| lot.id == id)
            || self.days.iter().any(|day| {
                day.acquisition.as_ref().is_some_and(|lot| lot.id == id)
                    || day
                        .dispositions
                        .iter()
                        .any(|disposed| disposed.lot.id == id)
            })
            || self
                .dividends
                .iter()
                .any(|event| event.lots.iter().any(|lot| lot.id == id))
    }
    fn assessed_through(
        &self,
        through: CivilDate,
        operation_seq: u64,
    ) -> Result<ExactDividendTaxAmount, DividendTaxError> {
        let mut assessed = ExactDividendTaxAmount::zero();
        for dividend in self
            .dividends
            .iter()
            .filter(|dividend| dividend.operation_seq <= operation_seq)
        {
            let quantity = total_quantity(&dividend.lots)?;
            for payment in dividend.payments.iter().filter(|payment| {
                payment.paid_on <= through && payment.operation_seq <= operation_seq
            }) {
                let per_share = ExactDividendTaxAmount::new(
                    i128::from(payment.received_gross.cents()),
                    quantity,
                )?;
                for lot in &dividend.lots {
                    if matches!(&lot.class, TaxShareClass::StatutoryRestricted { release_on, .. } if payment.paid_on < *release_on)
                    {
                        assessed = assessed.add(&per_share.multiply(lot.qty, 10)?)?;
                        continue;
                    }
                    let start = match &lot.class {
                        TaxShareClass::PublicMarket => lot.acquired_on,
                        TaxShareClass::StatutoryRestricted { release_on, .. } => *release_on,
                    };
                    for day in self.days.iter().filter(|day| {
                        day.day > dividend.registered_on
                            && day.day <= through
                            && day.operation_seq <= operation_seq
                    }) {
                        for disposition in day
                            .dispositions
                            .iter()
                            .filter(|disposition| disposition.lot.id == lot.id)
                        {
                            assessed = assessed.add(&per_share.multiply(
                                disposition.lot.qty,
                                personal_cash_dividend_rate(start, disposition.disposed_on)?,
                            )?)?;
                        }
                    }
                }
            }
        }
        Ok(assessed)
    }
    pub fn validate(&self) -> Result<(), DividendTaxError> {
        let mut operations = Vec::new();
        for day in &self.days {
            operations.push((day.operation_seq, day.day));
        }
        for dividend in &self.dividends {
            operations.push((dividend.operation_seq, dividend.registered_on));
            for payment in &dividend.payments {
                if payment.operation_seq <= dividend.operation_seq {
                    return Err(invalid("payment precedes dividend registration"));
                }
                operations.push((payment.operation_seq, payment.paid_on));
            }
        }
        for collection in &self.collections {
            operations.push((collection.operation_seq, collection.day));
        }
        operations.sort_unstable_by_key(|(sequence, _)| *sequence);
        let mut previous_fact_day = self.opened_on;
        for (index, (sequence, day)) in operations.iter().enumerate() {
            let expected = u64::try_from(index)
                .ok()
                .and_then(|index| index.checked_add(1))
                .ok_or_else(|| overflow("operation sequence"))?;
            if *sequence != expected || *day < previous_fact_day || *day > self.settled_on {
                return Err(invalid(
                    "operation facts must have consecutive unique sequence and nondecreasing dates",
                ));
            }
            previous_fact_day = *day;
        }
        if u64::try_from(operations.len()).map_err(|_| overflow("operation sequence"))?
            != self.operation_seq
        {
            return Err(invalid(
                "operation sequence does not match actual fact tail",
            ));
        }
        if self.profile != DividendTaxProfile::IndividualPublicMarket {
            return Err(DividendTaxError::UnsupportedProfile {
                profile: self.profile.clone(),
            });
        }
        if self.stock.0.trim().is_empty() || self.settled_on < self.opened_on {
            return Err(invalid("invalid tax book stock or opening/settled date"));
        }
        validate_lots(&self.lots, self.settled_on)?;
        validate_lots(&self.opening_lots, self.opened_on)?;
        let mut replayed = self.opening_lots.clone();
        let mut seen_lot_ids: BTreeSet<String> =
            replayed.iter().map(|lot| lot.id.clone()).collect();
        for (sequence, _) in &operations {
            if let Some(day) = self.days.iter().find(|day| day.operation_seq == *sequence) {
                if let Some(lot) = &day.acquisition {
                    if !seen_lot_ids.insert(lot.id.clone()) {
                        return Err(invalid("historical tax lot identity reused"));
                    }
                }
                let dispositions = apply_net_change(
                    &mut replayed,
                    day.day,
                    day.net_change,
                    day.acquisition.as_ref(),
                )?;
                if dispositions != day.dispositions {
                    return Err(invalid("tax disposal identities, metadata or FIFO quantities contradict held facts"));
                }
            }
            if let Some(dividend) = self
                .dividends
                .iter()
                .find(|dividend| dividend.operation_seq == *sequence)
            {
                if dividend.lots != replayed {
                    return Err(invalid("registered dividend tax lots contradict actual holding facts at registration"));
                }
            }
        }
        if replayed != self.lots {
            return Err(invalid("current tax lots contradict completed net changes"));
        }
        let mut day_ids = BTreeSet::new();
        let mut acquired_ids = BTreeSet::new();
        let mut previous_day = self.opened_on;
        for day in &self.days {
            if day.event_id.trim().is_empty()
                || !day_ids.insert(&day.event_id)
                || day.day.days_since(previous_day) != 1
                || day.day > self.settled_on
            {
                return Err(invalid(
                    "tax day receipts need unique identities and consecutive dates",
                ));
            }
            previous_day = day.day;
            if (day.net_change > 0) != day.acquisition.is_some() {
                return Err(invalid(
                    "tax day acquisition does not match positive net change",
                ));
            }
            if let Some(lot) = &day.acquisition {
                validate_lot(lot, day.day)?;
                if lot.acquired_on != day.day
                    || i128::from(lot.qty) != day.net_change
                    || !acquired_ids.insert(&lot.id)
                {
                    return Err(invalid("tax day acquisition facts are inconsistent"));
                }
            }
            let mut disposed_ids = BTreeSet::new();
            let mut total = 0i128;
            let mut last_acquired = None;
            for disposed in &day.dispositions {
                validate_lot(&disposed.lot, day.day)?;
                if disposed.disposed_on != day.day
                    || !disposed_ids.insert(&disposed.lot.id)
                    || last_acquired.is_some_and(|previous| previous > disposed.lot.acquired_on)
                    || matches!(&disposed.lot.class, TaxShareClass::StatutoryRestricted { release_on, .. } if day.day < *release_on)
                {
                    return Err(invalid("invalid tax FIFO disposal facts"));
                }
                last_acquired = Some(disposed.lot.acquired_on);
                total = total
                    .checked_add(i128::from(disposed.lot.qty))
                    .ok_or_else(|| overflow("tax disposition quantity"))?;
            }
            let expected = if day.net_change < 0 {
                day.net_change
                    .checked_neg()
                    .ok_or_else(|| overflow("negative net shares"))?
            } else {
                0
            };
            if total != expected {
                return Err(invalid("tax dispositions do not match day net reduction"));
            }
        }
        if previous_day != self.settled_on {
            return Err(invalid("tax settled day does not match receipt tail"));
        }
        let mut dividend_ids = BTreeSet::new();
        let mut payment_ids = BTreeSet::new();
        for dividend in &self.dividends {
            if dividend.event_id.trim().is_empty()
                || !dividend_ids.insert(&dividend.event_id)
                || dividend.registered_on < self.opened_on
                || dividend.registered_on > self.settled_on
                || dividend.per_share.numerator == 0
            {
                return Err(invalid("invalid immutable dividend registration"));
            }
            validate_lots(&dividend.lots, dividend.registered_on)?;
            let quantity = total_quantity(&dividend.lots)?;
            if quantity == 0 {
                return Err(invalid("registered dividend has no entitled shares"));
            }
            let mut received = 0i128;
            for payment in &dividend.payments {
                if payment.payment_id.trim().is_empty()
                    || !payment_ids.insert(&payment.payment_id)
                    || payment.evidence.trim().is_empty()
                    || payment.received_gross.cents() <= 0
                    || payment.paid_on < dividend.registered_on
                    || payment.paid_on > self.settled_on
                {
                    return Err(invalid("invalid actual dividend cash receipt"));
                }
                received = received
                    .checked_add(i128::from(payment.received_gross.cents()))
                    .ok_or_else(|| overflow("received dividends"))?;
            }
            let paid_scaled = received
                .checked_mul(i128::from(dividend.per_share.denominator))
                .ok_or_else(|| overflow("received dividend comparison"))?;
            let declared_scaled = dividend
                .per_share
                .numerator
                .checked_mul(i128::from(quantity))
                .ok_or_else(|| overflow("declared dividend comparison"))?;
            if paid_scaled > declared_scaled {
                return Err(invalid(
                    "received cash exceeds registered gross entitlement",
                ));
            }
            for lot in &dividend.lots {
                let disposed = self
                    .days
                    .iter()
                    .filter(|day| day.day > dividend.registered_on)
                    .flat_map(|day| &day.dispositions)
                    .filter(|disposed| disposed.lot.id == lot.id)
                    .try_fold(0u64, |total, disposed| {
                        total
                            .checked_add(disposed.lot.qty)
                            .ok_or_else(|| overflow("registered disposed quantity"))
                    })?;
                if disposed > lot.qty {
                    return Err(invalid("a dividend entitlement is disposed more than once"));
                }
            }
        }
        let mut collection_ids = BTreeSet::new();
        let mut collected = 0i128;
        let mut last_collection_day = self.opened_on;
        for collection in &self.collections {
            if collection.event_id.trim().is_empty()
                || !collection_ids.insert(&collection.event_id)
                || collection.day < last_collection_day
                || collection.day > self.settled_on
                || collection.available_cash.cents() < 0
                || collection.collected.cents() < 0
            {
                return Err(invalid("invalid tax collection identity, date or cash"));
            }
            last_collection_day = collection.day;
            let due = self
                .assessed_through(collection.day, collection.operation_seq)?
                .subtract_cents(collected)?;
            if due.denominator != 1 {
                return Err(DividendTaxError::NeedRoundingEvidence);
            }
            let expected = due
                .numerator
                .min(i128::from(collection.available_cash.cents()));
            if i128::from(collection.collected.cents()) != expected
                || collection.remaining_cash.cents()
                    != collection.available_cash.cents() - collection.collected.cents()
                || collection.outstanding != due.subtract_cents(expected)?
                || collection.needs_funds != (due.numerator > expected)
            {
                return Err(invalid(
                    "collection breaks actual cash or outstanding tax conservation",
                ));
            }
            collected = collected
                .checked_add(expected)
                .ok_or_else(|| overflow("collected tax total"))?;
        }
        Ok(())
    }

    fn advance_operation(&mut self) -> Result<u64, DividendTaxError> {
        self.operation_seq = self
            .operation_seq
            .checked_add(1)
            .ok_or_else(|| overflow("operation sequence"))?;
        Ok(self.operation_seq)
    }
}

pub fn personal_cash_dividend_rate(
    acquired_on: CivilDate,
    disposed_on: CivilDate,
) -> Result<u8, DividendTaxError> {
    if disposed_on < acquired_on {
        return Err(invalid("disposal must follow acquisition"));
    }
    let next_year = acquired_on.year() + i32::from(acquired_on.month() == 12);
    let next_month = if acquired_on.month() == 12 {
        1
    } else {
        acquired_on.month() + 1
    };
    let month_boundary = CivilDate::from_ymd(next_year, next_month, acquired_on.day())
        .map_err(|_| DividendTaxError::NeedHoldingPeriodBoundaryEvidence)?;
    if disposed_on <= month_boundary {
        return Ok(20);
    }
    let year_boundary = CivilDate::from_ymd(
        acquired_on.year() + 1,
        acquired_on.month(),
        acquired_on.day(),
    )
    .map_err(|_| DividendTaxError::NeedHoldingPeriodBoundaryEvidence)?;
    Ok(if disposed_on <= year_boundary { 10 } else { 0 })
}

fn total_quantity(lots: &[DividendTaxLot]) -> Result<u64, DividendTaxError> {
    lots.iter().try_fold(0u64, |total, lot| {
        total
            .checked_add(lot.qty)
            .ok_or_else(|| overflow("tax lot share sum"))
    })
}

fn apply_net_change(
    lots: &mut Vec<DividendTaxLot>,
    day: CivilDate,
    net_change: i128,
    acquisition: Option<&DividendTaxLot>,
) -> Result<Vec<TaxDisposition>, DividendTaxError> {
    if (net_change > 0) != acquisition.is_some() {
        return Err(invalid(
            "net acquisition facts do not match positive share change",
        ));
    }
    let mut dispositions = Vec::new();
    if let Some(lot) = acquisition {
        validate_lot(lot, day)?;
        if lot.acquired_on != day || i128::from(lot.qty) != net_change {
            return Err(invalid("net acquisition date or quantity mismatch"));
        }
        lots.push(lot.clone());
    } else if net_change < 0 {
        let public = lots
            .iter()
            .any(|lot| lot.class == TaxShareClass::PublicMarket);
        let restricted = lots
            .iter()
            .any(|lot| matches!(lot.class, TaxShareClass::StatutoryRestricted { .. }));
        if public && restricted {
            return Err(DividendTaxError::UnsupportedMixedRestrictedTaxLots);
        }
        let mut remaining = u64::try_from(
            net_change
                .checked_neg()
                .ok_or_else(|| overflow("negative net shares"))?,
        )
        .map_err(|_| invalid("daily net disposal exceeds u64"))?;
        for lot in lots.iter_mut() {
            if remaining == 0 {
                break;
            }
            if matches!(&lot.class, TaxShareClass::StatutoryRestricted { release_on, .. } if day < *release_on)
            {
                return Err(invalid(
                    "statutory restricted shares cannot be publicly disposed before release",
                ));
            }
            let consumed = remaining.min(lot.qty);
            let mut disposed_lot = lot.clone();
            disposed_lot.qty = consumed;
            dispositions.push(TaxDisposition {
                lot: disposed_lot,
                disposed_on: day,
            });
            lot.qty -= consumed;
            remaining -= consumed;
        }
        if remaining != 0 {
            return Err(invalid("insufficient shares for daily tax disposal"));
        }
        lots.retain(|lot| lot.qty != 0);
    }
    validate_lots(lots, day)?;
    Ok(dispositions)
}
fn validate_lots(lots: &[DividendTaxLot], day: CivilDate) -> Result<(), DividendTaxError> {
    let mut ids = BTreeSet::new();
    let mut previous_date = None;
    for lot in lots {
        validate_lot(lot, day)?;
        if !ids.insert(&lot.id) || previous_date.is_some_and(|previous| previous > lot.acquired_on)
        {
            return Err(invalid(
                "tax lots have duplicate identities or unordered acquisition dates",
            ));
        }
        previous_date = Some(lot.acquired_on);
    }
    total_quantity(lots)?;
    Ok(())
}
fn validate_lot(lot: &DividendTaxLot, day: CivilDate) -> Result<(), DividendTaxError> {
    if lot.id.trim().is_empty() || lot.qty == 0 || lot.acquired_on > day {
        return Err(invalid(
            "tax lot requires identity, positive shares and valid explicit acquisition date",
        ));
    }
    let evidence = match &lot.source {
        TaxAcquisitionSource::InitialAllocation { evidence } => evidence,
        TaxAcquisitionSource::SecondaryMarket { settlement } => settlement,
        TaxAcquisitionSource::CorporateAction { event } => event,
    };
    if evidence.trim().is_empty() {
        return Err(invalid("tax lot acquisition source evidence is required"));
    }
    if let TaxShareClass::StatutoryRestricted {
        release_on,
        qualification_evidence,
        ..
    } = &lot.class
    {
        if *release_on < lot.acquired_on || qualification_evidence.trim().is_empty() {
            return Err(invalid(
                "statutory restricted tax lot requires qualifying evidence and release date",
            ));
        }
    }
    Ok(())
}
fn required_lot<'de, Decoder: serde::Deserializer<'de>>(
    decoder: Decoder,
) -> Result<Option<DividendTaxLot>, Decoder::Error> {
    Option::<DividendTaxLot>::deserialize(decoder)
}
fn invalid(detail: &str) -> DividendTaxError {
    DividendTaxError::InvalidFact(detail.into())
}
fn overflow(operation: &'static str) -> DividendTaxError {
    DividendTaxError::Overflow { operation }
}
fn gcd(mut left: u128, mut right: u128) -> u128 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left
}

mod nonnegative_decimal {
    pub fn serialize<Serializer: serde::Serializer>(
        number: &i128,
        serializer: Serializer,
    ) -> Result<Serializer::Ok, Serializer::Error> {
        serializer.serialize_str(&number.to_string())
    }
    pub fn deserialize<'de, Decoder: serde::Deserializer<'de>>(
        decoder: Decoder,
    ) -> Result<i128, Decoder::Error> {
        let text = <String as serde::Deserialize>::deserialize(decoder)?;
        if text.is_empty()
            || !text.bytes().all(|byte| byte.is_ascii_digit())
            || (text.len() > 1 && text.starts_with('0'))
        {
            return Err(serde::de::Error::custom(
                "exact amount numerator must be a canonical nonnegative decimal string",
            ));
        }
        text.parse::<i128>().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests;
