use super::super::StepFatal;
use crate::{AccountId, OrderId, Side, StockCode};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum JournalRank {
    PreSeal,
    SealedBatch,
}
impl JournalRank {
    pub const fn rank(self) -> u8 {
        match self {
            Self::PreSeal => 0,
            Self::SealedBatch => 1,
        }
    }
}
impl Ord for JournalRank {
    fn cmp(&self, other: &Self) -> Ordering {
        self.rank().cmp(&other.rank())
    }
}
impl PartialOrd for JournalRank {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Payload numbering is source-local: expiry by (stock, order), sealed by sealed index,
/// auction/day-end by deterministic envelope order for pre-existing orders.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum ReceiptSource {
    SealedIntent(u64),
    P0Expiry(u32),
    Auction(u32),
    DayEnd(u32),
}
impl ReceiptSource {
    pub const fn journal(self) -> JournalRank {
        match self {
            Self::P0Expiry(_) => JournalRank::PreSeal,
            Self::SealedIntent(_) | Self::Auction(_) | Self::DayEnd(_) => JournalRank::SealedBatch,
        }
    }
    pub const fn rank(self) -> u8 {
        match self {
            Self::P0Expiry(_) | Self::SealedIntent(_) => 0,
            Self::Auction(_) => 1,
            Self::DayEnd(_) => 2,
        }
    }
    pub fn payload(self) -> u64 {
        match self {
            Self::SealedIntent(index) => index,
            Self::P0Expiry(index) | Self::Auction(index) | Self::DayEnd(index) => u64::from(index),
        }
    }
}
impl Ord for ReceiptSource {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.journal().rank(), self.rank(), self.payload()).cmp(&(
            other.journal().rank(),
            other.rank(),
            other.payload(),
        ))
    }
}
impl PartialOrd for ReceiptSource {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Identity contract only; no escrow resources or accounting are represented.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct EnvelopeKey {
    pub account: AccountId,
    pub stock: StockCode,
    pub order: OrderId,
    pub side: Side,
}
impl EnvelopeKey {
    const fn side_rank(&self) -> u8 {
        match self.side {
            Side::Buy => 0,
            Side::Sell => 1,
        }
    }
}
impl Ord for EnvelopeKey {
    fn cmp(&self, other: &Self) -> Ordering {
        (&self.account, &self.stock, &self.order, self.side_rank()).cmp(&(
            &other.account,
            &other.stock,
            &other.order,
            other.side_rank(),
        ))
    }
}
impl PartialOrd for EnvelopeKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Ordinal domain is (journal, source instance, envelope). A new source restarts at zero.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ReceiptTransition {
    pub envelope: EnvelopeKey,
    pub ordinal: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ReceiptLocalKey {
    journal: JournalRank,
    source: ReceiptSource,
    transition: ReceiptTransition,
}
impl ReceiptLocalKey {
    pub fn new(
        journal: JournalRank,
        source: ReceiptSource,
        transition: ReceiptTransition,
    ) -> Result<Self, StepFatal> {
        if journal != source.journal() {
            return Err(StepFatal::InvariantViolation {
                description: format!("invalid journal/source pairing: {journal:?}/{source:?}"),
                location: "ReceiptLocalKey::new".to_owned(),
            });
        }
        Ok(Self {
            journal,
            source,
            transition,
        })
    }

    pub const fn source(&self) -> ReceiptSource {
        self.source
    }

    pub const fn journal(&self) -> JournalRank {
        self.journal
    }
}
impl Ord for ReceiptLocalKey {
    fn cmp(&self, other: &Self) -> Ordering {
        (
            self.journal.rank(),
            self.source.rank(),
            self.source.payload(),
            &self.transition.envelope,
            self.transition.ordinal,
        )
            .cmp(&(
                other.journal.rank(),
                other.source.rank(),
                other.source.payload(),
                &other.transition.envelope,
                other.transition.ordinal,
            ))
    }
}
impl PartialOrd for ReceiptLocalKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub struct ReceiptIndex(pub u64);

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct IndexedReceiptKey {
    pub index: ReceiptIndex,
    pub envelope: EnvelopeKey,
    pub local_key: ReceiptLocalKey,
}

pub fn validate_receipt_keys(keys: &[ReceiptLocalKey]) -> Result<(), StepFatal> {
    let mut seen = BTreeSet::new();
    let mut next_ordinal = BTreeMap::new();
    let mut previous = None;
    for key in keys {
        if key.journal != key.source.journal() {
            return Err(invariant("invalid receipt journal/source pairing"));
        }
        if let Some(previous) = previous {
            if previous >= key {
                return Err(invariant("receipt local keys are not in canonical order"));
            }
        }
        if !seen.insert(key) {
            return Err(invariant(&format!("duplicate receipt identity: {key:?}")));
        }
        let domain = (key.source, key.transition.envelope.clone());
        let ordinal = next_ordinal.entry(domain).or_insert(0);
        if key.transition.ordinal != *ordinal {
            return Err(invariant("receipt source ordinal is not contiguous"));
        }
        *ordinal = ordinal
            .checked_add(1)
            .ok_or_else(|| invariant("receipt source ordinal overflow"))?;
        previous = Some(key);
    }
    Ok(())
}

pub fn validate_indexed_receipt_keys(keys: &[IndexedReceiptKey]) -> Result<(), StepFatal> {
    let local_keys: Vec<_> = keys.iter().map(|key| key.local_key.clone()).collect();
    validate_receipt_keys(&local_keys)?;

    let mut pairs = BTreeSet::new();
    let mut expected_index = keys.first().map(|key| key.index);
    for key in keys {
        if key.envelope != key.local_key.transition.envelope {
            return Err(invariant("receipt envelope disagrees with local key"));
        }
        if !pairs.insert((key.envelope.clone(), key.index)) {
            return Err(invariant("duplicate envelope receipt index"));
        }
        if Some(key.index) != expected_index {
            return Err(invariant("receipt indices are not contiguous"));
        }
        expected_index = Some(ReceiptIndex(
            key.index
                .0
                .checked_add(1)
                .ok_or_else(|| invariant("receipt index overflow"))?,
        ));
    }
    Ok(())
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::receipt_key".to_owned(),
    }
}
