use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::CompanyId;
use crate::{account::StockCode, calendar::CivilDate, orderbook::AccountId};

pub(crate) mod canonical_i128_decimal {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &i128, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<i128, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let bytes = value.as_bytes();
        let digits = if bytes.first() == Some(&b'-') {
            &bytes[1..]
        } else {
            bytes
        };
        let canonical = digits == b"0"
            || (digits
                .first()
                .is_some_and(|byte| (b'1'..=b'9').contains(byte))
                && digits.iter().all(u8::is_ascii_digit));
        if !canonical || (bytes.first() == Some(&b'-') && digits == b"0") {
            return Err(serde::de::Error::custom(
                "股数变动必须使用规范有符号十进制字符串",
            ));
        }
        value.parse::<i128>().map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum HolderId {
    Account(AccountId),
    External(String),
    IssuerTreasury,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum AcquisitionSource {
    InitialAllocation { evidence: String },
    SecondaryMarket { settlement: String },
    CorporateAction { event: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum ShareRestriction {
    Unrestricted,
    Restricted {
        reason: String,
        release_on: CivilDate,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 当前登记事实，不替代真实的回购成交或过户流水证明。
pub struct IssuerRepurchaseAccountFacts {
    pub account_reference: String,
    pub source_evidence: String,
    pub established_on: CivilDate,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShareLot {
    pub id: String,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub qty: u64,
    pub acquired_on: CivilDate,
    pub source: AcquisitionSource,
    pub restriction: ShareRestriction,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShareHolding {
    pub holder: HolderId,
    pub lots: Vec<ShareLot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetAcquisition {
    pub lot_id: String,
    pub source: AcquisitionSource,
    pub restriction: ShareRestriction,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DayNetChange {
    pub holder: HolderId,
    #[serde(with = "canonical_i128_decimal")]
    pub change: i128,
    #[serde(deserialize_with = "required_acquisition")]
    pub acquisition: Option<NetAcquisition>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShareDayRequest {
    pub event_id: String,
    pub day: CivilDate,
    pub scope: MovementScope,
    pub changes: Vec<DayNetChange>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum MovementScope {
    PublicMarket,
    NonTradingTransfer { basis: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisposedLot {
    pub holder: HolderId,
    pub lot: ShareLot,
    pub disposed_on: CivilDate,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShareDayReceipt {
    pub request: ShareDayRequest,
    pub disposals: Vec<DisposedLot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "SnapshotState")]
pub struct RegistrationSnapshot {
    event_id: String,
    stock: StockCode,
    issuer: CompanyId,
    registered_on: CivilDate,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    issued_shares: u64,
    #[serde(deserialize_with = "required_issuer_repurchase_account")]
    issuer_repurchase_account: Option<IssuerRepurchaseAccountFacts>,
    holdings: Vec<ShareHolding>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotState {
    event_id: String,
    stock: StockCode,
    issuer: CompanyId,
    registered_on: CivilDate,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    issued_shares: u64,
    #[serde(deserialize_with = "required_issuer_repurchase_account")]
    issuer_repurchase_account: Option<IssuerRepurchaseAccountFacts>,
    holdings: Vec<ShareHolding>,
}

impl TryFrom<SnapshotState> for RegistrationSnapshot {
    type Error = ShareRegistryError;

    fn try_from(state: SnapshotState) -> Result<Self, Self::Error> {
        let snapshot = Self {
            event_id: state.event_id,
            stock: state.stock,
            issuer: state.issuer,
            registered_on: state.registered_on,
            issued_shares: state.issued_shares,
            issuer_repurchase_account: state.issuer_repurchase_account,
            holdings: state.holdings,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RegistryState")]
pub struct ShareRegistry {
    stock: StockCode,
    issuer: CompanyId,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    issued_shares: u64,
    #[serde(deserialize_with = "required_issuer_repurchase_account")]
    issuer_repurchase_account: Option<IssuerRepurchaseAccountFacts>,
    settled_on: CivilDate,
    holdings: Vec<ShareHolding>,
    receipts: Vec<ShareDayReceipt>,
    registrations: Vec<RegistrationSnapshot>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryState {
    stock: StockCode,
    issuer: CompanyId,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    issued_shares: u64,
    #[serde(deserialize_with = "required_issuer_repurchase_account")]
    issuer_repurchase_account: Option<IssuerRepurchaseAccountFacts>,
    settled_on: CivilDate,
    holdings: Vec<ShareHolding>,
    receipts: Vec<ShareDayReceipt>,
    registrations: Vec<RegistrationSnapshot>,
}

impl TryFrom<RegistryState> for ShareRegistry {
    type Error = ShareRegistryError;

    fn try_from(state: RegistryState) -> Result<Self, Self::Error> {
        let registry = Self {
            stock: state.stock,
            issuer: state.issuer,
            issued_shares: state.issued_shares,
            issuer_repurchase_account: state.issuer_repurchase_account,
            settled_on: state.settled_on,
            holdings: state.holdings,
            receipts: state.receipts,
            registrations: state.registrations,
        };
        registry.validate()?;
        Ok(registry)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum ShareRegistryError {
    #[error("share registry: {detail}")]
    InvalidFact { detail: String },
    #[error("share registry: unsupported movement scope {scope:?}")]
    UnsupportedMovementScope { scope: MovementScope },
    #[error(
        "share registry: holder {holder:?} needs {requested} transferable shares, only {available} available"
    )]
    InsufficientTransferableShares {
        holder: HolderId,
        requested: u64,
        available: u64,
    },
}

impl ShareRegistry {
    pub fn new(
        stock: StockCode,
        issuer: CompanyId,
        issued_shares: u64,
        settled_on: CivilDate,
        holdings: Vec<ShareHolding>,
    ) -> Result<Self, ShareRegistryError> {
        let registry = Self {
            stock,
            issuer,
            issued_shares,
            issuer_repurchase_account: None,
            settled_on,
            holdings,
            receipts: vec![],
            registrations: vec![],
        };
        registry.validate()?;
        Ok(registry)
    }

    pub fn holdings(&self) -> &[ShareHolding] {
        &self.holdings
    }

    pub fn receipts(&self) -> &[ShareDayReceipt] {
        &self.receipts
    }

    pub fn issuer_repurchase_account(&self) -> Option<&IssuerRepurchaseAccountFacts> {
        self.issuer_repurchase_account.as_ref()
    }

    /// 设置回购专户事实；已有登记快照后不得回溯改变当时的资格分类。
    pub fn set_issuer_repurchase_account(
        &mut self,
        facts: IssuerRepurchaseAccountFacts,
    ) -> Result<(), ShareRegistryError> {
        if let Some(existing) = &self.issuer_repurchase_account {
            return if existing == &facts {
                Ok(())
            } else {
                Err(error(
                    "issuer repurchase account facts are immutable once set",
                ))
            };
        }
        validate_issuer_repurchase_account(Some(&facts), self.settled_on)?;
        if self
            .registrations
            .iter()
            .any(|snapshot| snapshot.registered_on >= facts.established_on)
        {
            return Err(error(
                "issuer repurchase account facts cannot be established retroactively across a registration",
            ));
        }
        self.issuer_repurchase_account = Some(facts);
        Ok(())
    }

    pub fn close_day(
        &mut self,
        request: ShareDayRequest,
    ) -> Result<ShareDayReceipt, ShareRegistryError> {
        if let Some(receipt) = self
            .receipts
            .iter()
            .find(|receipt| receipt.request.event_id == request.event_id)
        {
            if receipt.request == request {
                return Ok(receipt.clone());
            }
            return Err(error("day event identity reused with different input"));
        }
        validate_request(&request)?;
        for acquisition in request
            .changes
            .iter()
            .filter_map(|change| change.acquisition.as_ref())
        {
            if self.has_lot_id(&acquisition.lot_id) {
                return Err(error("share lot identity reused"));
            }
        }
        if request.day.days_since(self.settled_on) != 1 {
            return Err(error("day settlement must follow the previous natural day"));
        }
        let mut candidate = self.clone();
        let mut disposals = Vec::new();
        for change in &request.changes {
            if change.change < 0 {
                let quantity = u64::try_from(
                    change
                        .change
                        .checked_neg()
                        .ok_or_else(|| error("negative share change overflow"))?,
                )
                .map_err(|_| error("share change exceeds u64"))?;
                let holding = candidate
                    .holdings
                    .iter_mut()
                    .find(|holding| holding.holder == change.holder)
                    .ok_or_else(|| error("disposing holder is not registered"))?;
                let mut remaining = quantity;
                for lot in &mut holding.lots {
                    if remaining == 0 {
                        break;
                    }
                    if let ShareRestriction::Restricted { release_on, .. } = &lot.restriction {
                        if request.day < *release_on {
                            continue;
                        }
                    }
                    let consumed = remaining.min(lot.qty);
                    let mut disposed = lot.clone();
                    disposed.qty = consumed;
                    disposals.push(DisposedLot {
                        holder: change.holder.clone(),
                        lot: disposed,
                        disposed_on: request.day,
                    });
                    lot.qty -= consumed;
                    remaining -= consumed;
                }
                if remaining != 0 {
                    return Err(ShareRegistryError::InsufficientTransferableShares {
                        holder: change.holder.clone(),
                        requested: quantity,
                        available: quantity - remaining,
                    });
                }
                holding.lots.retain(|lot| lot.qty != 0);
            } else if change.change > 0 {
                let quantity =
                    u64::try_from(change.change).map_err(|_| error("share change exceeds u64"))?;
                let acquisition = change
                    .acquisition
                    .as_ref()
                    .ok_or_else(|| error("net acquisition facts are required"))?;
                if candidate.has_lot_id(&acquisition.lot_id) {
                    return Err(error("share lot identity reused"));
                }
                let lot = ShareLot {
                    id: acquisition.lot_id.clone(),
                    qty: quantity,
                    acquired_on: request.day,
                    source: acquisition.source.clone(),
                    restriction: acquisition.restriction.clone(),
                };
                if let Some(holding) = candidate
                    .holdings
                    .iter_mut()
                    .find(|holding| holding.holder == change.holder)
                {
                    holding.lots.push(lot);
                } else {
                    candidate.holdings.push(ShareHolding {
                        holder: change.holder.clone(),
                        lots: vec![lot],
                    });
                }
            } else if !candidate
                .holdings
                .iter()
                .any(|holding| holding.holder == change.holder)
            {
                return Err(error("zero-net holder is not registered"));
            }
        }
        let receipt = ShareDayReceipt { request, disposals };
        candidate.settled_on = receipt.request.day;
        candidate.receipts.push(receipt.clone());
        candidate.validate()?;
        *self = candidate;
        Ok(receipt)
    }

    pub fn register(
        &mut self,
        event_id: String,
        registered_on: CivilDate,
    ) -> Result<&RegistrationSnapshot, ShareRegistryError> {
        if let Some(index) = self
            .registrations
            .iter()
            .position(|snapshot| snapshot.event_id == event_id)
        {
            if self.registrations[index].registered_on != registered_on {
                return Err(error("registration identity reused with different date"));
            }
            return Ok(&self.registrations[index]);
        }
        if event_id.trim().is_empty() {
            return Err(error("empty registration event identity"));
        }
        if registered_on != self.settled_on {
            return Err(error(
                "registration must capture the current completed day, not a historical or future balance",
            ));
        }
        self.validate()?;
        self.registrations.push(RegistrationSnapshot {
            event_id,
            stock: self.stock.clone(),
            issuer: self.issuer.clone(),
            registered_on,
            issued_shares: self.issued_shares,
            issuer_repurchase_account: self.issuer_repurchase_account.clone(),
            holdings: self.holdings.clone(),
        });
        self.registrations
            .last()
            .ok_or_else(|| error("registration insertion failed"))
    }

    pub fn registration(&self, event_id: &str) -> Option<&RegistrationSnapshot> {
        self.registrations
            .iter()
            .find(|snapshot| snapshot.event_id == event_id)
    }

    pub fn validate(&self) -> Result<(), ShareRegistryError> {
        if self.stock.0.trim().is_empty() || self.issuer.0.trim().is_empty() {
            return Err(error("stock and issuer identities are required"));
        }
        validate_holdings(&self.holdings, self.issued_shares, self.settled_on)?;
        validate_issuer_repurchase_account(
            self.issuer_repurchase_account.as_ref(),
            self.settled_on,
        )?;
        let mut identities = BTreeSet::new();
        let mut previous_day = None;
        let mut acquired_ids = BTreeSet::new();
        for receipt in &self.receipts {
            validate_request(&receipt.request)?;
            if !identities.insert(&receipt.request.event_id)
                || receipt.request.day > self.settled_on
            {
                return Err(error("invalid settlement receipt identity or date"));
            }
            if let Some(previous) = previous_day {
                if receipt.request.day.days_since(previous) != 1 {
                    return Err(error("settlement receipt dates are not consecutive"));
                }
            }
            previous_day = Some(receipt.request.day);
            for change in &receipt.request.changes {
                if let Some(acquisition) = &change.acquisition {
                    if !acquired_ids.insert(&acquisition.lot_id) {
                        return Err(error("historical acquisition lot identity reused"));
                    }
                }
                let consumed = receipt
                    .disposals
                    .iter()
                    .filter(|disposed| disposed.holder == change.holder)
                    .try_fold(0i128, |total, disposed| {
                        total
                            .checked_add(i128::from(disposed.lot.qty))
                            .ok_or_else(|| error("disposal sum overflow"))
                    })?;
                let expected = if change.change < 0 {
                    change
                        .change
                        .checked_neg()
                        .ok_or_else(|| error("negative share change overflow"))?
                } else {
                    0
                };
                if consumed != expected {
                    return Err(error("disposal receipt does not match daily net reduction"));
                }
            }
            let mut disposal_ids = BTreeSet::new();
            for disposed in &receipt.disposals {
                validate_holder(&disposed.holder)?;
                validate_lot(&disposed.lot, disposed.disposed_on)?;
                if !disposal_ids.insert((&disposed.holder, &disposed.lot.id)) {
                    return Err(error(
                        "a physical lot is disposed more than once in one day",
                    ));
                }
                if matches!(&disposed.lot.restriction, ShareRestriction::Restricted { release_on, .. } if *release_on > disposed.disposed_on)
                {
                    return Err(error(
                        "public transfer receipt contains an unreleased physical lot",
                    ));
                }
                if disposed.disposed_on != receipt.request.day
                    || !receipt
                        .request
                        .changes
                        .iter()
                        .any(|change| change.holder == disposed.holder && change.change < 0)
                {
                    return Err(error("orphan disposal receipt"));
                }
            }
        }
        if previous_day.is_some_and(|day| day != self.settled_on) {
            return Err(error("latest receipt does not match settled date"));
        }
        let mut registration_ids = BTreeSet::new();
        for snapshot in &self.registrations {
            snapshot.validate()?;
            if snapshot.event_id.trim().is_empty()
                || !registration_ids.insert(&snapshot.event_id)
                || snapshot.stock != self.stock
                || snapshot.issuer != self.issuer
                || snapshot.issued_shares != self.issued_shares
                || snapshot.registered_on > self.settled_on
            {
                return Err(error(
                    "invalid immutable registration identity, issuer, capital or date",
                ));
            }
            validate_holdings(
                &snapshot.holdings,
                snapshot.issued_shares,
                snapshot.registered_on,
            )?;
            let expected_facts = self
                .issuer_repurchase_account
                .as_ref()
                .filter(|facts| facts.established_on <= snapshot.registered_on);
            if snapshot.issuer_repurchase_account.as_ref() != expected_facts {
                return Err(error(
                    "registration issuer repurchase account facts do not match registry history",
                ));
            }
        }
        Ok(())
    }

    pub fn stock(&self) -> &StockCode {
        &self.stock
    }
    pub fn issuer(&self) -> &CompanyId {
        &self.issuer
    }
    pub fn issued_shares(&self) -> u64 {
        self.issued_shares
    }
    pub fn settled_on(&self) -> CivilDate {
        self.settled_on
    }

    pub fn receipt_by_event(&self, event_id: &str) -> Option<&ShareDayReceipt> {
        self.receipts
            .iter()
            .find(|receipt| receipt.request.event_id == event_id)
    }

    fn has_lot_id(&self, id: &str) -> bool {
        self.holdings
            .iter()
            .flat_map(|holding| &holding.lots)
            .any(|lot| lot.id == id)
            || self.receipts.iter().any(|receipt| {
                receipt
                    .disposals
                    .iter()
                    .any(|disposed| disposed.lot.id == id)
                    || receipt.request.changes.iter().any(|change| {
                        change
                            .acquisition
                            .as_ref()
                            .is_some_and(|acquisition| acquisition.lot_id == id)
                    })
            })
            || self
                .registrations
                .iter()
                .flat_map(|snapshot| &snapshot.holdings)
                .flat_map(|holding| &holding.lots)
                .any(|lot| lot.id == id)
    }
}

fn validate_holder(holder: &HolderId) -> Result<(), ShareRegistryError> {
    if matches!(holder, HolderId::External(id) if id.trim().is_empty()) {
        return Err(error("external holder identity is required"));
    }
    Ok(())
}

fn validate_source(source: &AcquisitionSource) -> Result<(), ShareRegistryError> {
    let identity = match source {
        AcquisitionSource::InitialAllocation { evidence } => evidence,
        AcquisitionSource::SecondaryMarket { settlement } => settlement,
        AcquisitionSource::CorporateAction { event } => event,
    };
    if identity.trim().is_empty() {
        return Err(error("acquisition source evidence is required"));
    }
    Ok(())
}

fn validate_lot(lot: &ShareLot, settled_on: CivilDate) -> Result<(), ShareRegistryError> {
    if lot.id.trim().is_empty() || lot.qty == 0 || lot.acquired_on > settled_on {
        return Err(error(
            "invalid share lot identity, quantity or acquisition date",
        ));
    }
    validate_source(&lot.source)?;
    if let ShareRestriction::Restricted { reason, release_on } = &lot.restriction {
        if reason.trim().is_empty() || *release_on < lot.acquired_on {
            return Err(error("invalid share restriction facts"));
        }
    }
    Ok(())
}

fn validate_holdings(
    holdings: &[ShareHolding],
    issued_shares: u64,
    settled_on: CivilDate,
) -> Result<(), ShareRegistryError> {
    let mut holders = BTreeSet::new();
    let mut lots = BTreeSet::new();
    let mut total = 0u64;
    for holding in holdings {
        validate_holder(&holding.holder)?;
        if !holders.insert(&holding.holder) {
            return Err(error("duplicate holder"));
        }
        let mut previous_date = None;
        for lot in &holding.lots {
            validate_lot(lot, settled_on)?;
            if !lots.insert(&lot.id)
                || previous_date.is_some_and(|previous| previous > lot.acquired_on)
            {
                return Err(error(
                    "duplicate lot identity or non-FIFO acquisition dates",
                ));
            }
            previous_date = Some(lot.acquired_on);
            total = total
                .checked_add(lot.qty)
                .ok_or_else(|| error("share sum overflow"))?;
        }
    }
    if issued_shares == 0 || total != issued_shares {
        return Err(error("issued share conservation mismatch"));
    }
    Ok(())
}

fn validate_request(request: &ShareDayRequest) -> Result<(), ShareRegistryError> {
    if request.scope != MovementScope::PublicMarket {
        return Err(ShareRegistryError::UnsupportedMovementScope {
            scope: request.scope.clone(),
        });
    }
    if request.event_id.trim().is_empty() {
        return Err(error("empty daily settlement identity"));
    }
    let mut holders = BTreeSet::new();
    let mut total = 0i128;
    for change in &request.changes {
        validate_holder(&change.holder)?;
        if !holders.insert(&change.holder) {
            return Err(error(
                "daily request contains duplicate holder; supply one net change",
            ));
        }
        total = total
            .checked_add(change.change)
            .ok_or_else(|| error("daily net change sum overflow"))?;
        if change.change > 0 {
            let acquisition = change
                .acquisition
                .as_ref()
                .ok_or_else(|| error("net acquisition facts are required"))?;
            if !matches!(
                acquisition.source,
                AcquisitionSource::SecondaryMarket { .. }
            ) || acquisition.restriction != ShareRestriction::Unrestricted
            {
                return Err(error(
                    "public market acquisition requires secondary-market source and unrestricted shares",
                ));
            }
            let lot = ShareLot {
                id: acquisition.lot_id.clone(),
                qty: u64::try_from(change.change)
                    .map_err(|_| error("net acquisition exceeds u64"))?,
                acquired_on: request.day,
                source: acquisition.source.clone(),
                restriction: acquisition.restriction.clone(),
            };
            validate_lot(&lot, request.day)?;
        } else if change.acquisition.is_some() {
            return Err(error(
                "non-positive net change cannot create an acquisition lot",
            ));
        }
    }
    if total != 0 {
        return Err(error("secondary transfer must conserve issued shares"));
    }
    Ok(())
}

impl RegistrationSnapshot {
    pub fn validate(&self) -> Result<(), ShareRegistryError> {
        if self.event_id.trim().is_empty()
            || self.stock.0.trim().is_empty()
            || self.issuer.0.trim().is_empty()
        {
            return Err(error(
                "registration event, stock and issuer identities are required",
            ));
        }
        validate_holdings(&self.holdings, self.issued_shares, self.registered_on).and_then(|()| {
            validate_issuer_repurchase_account(
                self.issuer_repurchase_account.as_ref(),
                self.registered_on,
            )
        })
    }
    pub fn holdings(&self) -> &[ShareHolding] {
        &self.holdings
    }
    pub fn entitled_holdings(&self) -> impl Iterator<Item = &ShareHolding> {
        self.holdings.iter().filter(|holding| {
            holding.holder != HolderId::IssuerTreasury && !holding.lots.is_empty()
        })
    }
    pub fn event_id(&self) -> &str {
        &self.event_id
    }
    pub fn stock(&self) -> &StockCode {
        &self.stock
    }
    pub fn issuer(&self) -> &CompanyId {
        &self.issuer
    }
    pub fn registered_on(&self) -> CivilDate {
        self.registered_on
    }
    pub fn issued_shares(&self) -> u64 {
        self.issued_shares
    }
    pub fn issuer_repurchase_account(&self) -> Option<&IssuerRepurchaseAccountFacts> {
        self.issuer_repurchase_account.as_ref()
    }
}

fn validate_issuer_repurchase_account(
    facts: Option<&IssuerRepurchaseAccountFacts>,
    as_of: CivilDate,
) -> Result<(), ShareRegistryError> {
    if let Some(facts) = facts {
        if facts.account_reference.trim().is_empty()
            || facts.source_evidence.trim().is_empty()
            || facts.established_on > as_of
        {
            return Err(error("invalid issuer repurchase account facts"));
        }
    }
    Ok(())
}

fn error(detail: &str) -> ShareRegistryError {
    ShareRegistryError::InvalidFact {
        detail: detail.into(),
    }
}

fn required_acquisition<'de, Decoder: serde::Deserializer<'de>>(
    decoder: Decoder,
) -> Result<Option<NetAcquisition>, Decoder::Error> {
    Option::<NetAcquisition>::deserialize(decoder)
}

fn required_issuer_repurchase_account<'de, Decoder: serde::Deserializer<'de>>(
    decoder: Decoder,
) -> Result<Option<IssuerRepurchaseAccountFacts>, Decoder::Error> {
    Option::<IssuerRepurchaseAccountFacts>::deserialize(decoder)
}

#[cfg(test)]
mod tests;
