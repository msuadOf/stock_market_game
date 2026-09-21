//! Purpose-built Task 9 evidence projections for the escrow verifier.
//!
//! This module deliberately projects the runtime contracts field by field.  It
//! never exposes `Envelope`, `EnvelopeReceipt`, protocol frames, or account
//! snapshots through their ordinary serde representations.

use crate::{
    session::{
        pipeline::{
            EntityTag, Envelope, EnvelopeOrigin, EnvelopeReceipt, EventSourceIndex, JournalRank,
            ReceiptKind, ReceiptLocalKey, ReceiptSource, ResVec,
        },
        protocol::{CivilUpdate, EventFact, TickFrame},
        Event,
    },
    AccountId, AccountSnap, Money, OrderId, PositionSnap, Side, StockCode,
};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Display,
};

pub const OBSERVATION_SCHEMA: &str = "escrow-determinism-observation-v1";
pub const CONSERVATION_SCHEMA: &str = "escrow-conservation-snapshot-v1";
pub const CORPUS_SCHEMA: &str = "escrow-corpus-projection-v1";

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EvidenceError {
    #[error("Task 9 evidence field {field} must not be empty")]
    EmptyField { field: &'static str },
    #[error("Task 9 evidence contains an invalid six-digit stock code: {code}")]
    InvalidStockCode { code: String },
    #[error("Task 9 evidence cannot project negative money at {field}: {cents} cents")]
    NegativeMoney { field: &'static str, cents: i64 },
    #[error("receipt {receipt_index} does not belong to its projected envelope")]
    ReceiptEnvelopeMismatch { receipt_index: u64 },
    #[error("receipt {receipt_index} breaks the envelope live-resource chain")]
    ReceiptChainMismatch { receipt_index: u64 },
    #[error("receipt indices are duplicated or non-contiguous")]
    ReceiptIndexSequence,
    #[error("receipt {receipt_index} has invalid local identity: {detail}")]
    InvalidReceiptIdentity {
        receipt_index: u64,
        detail: &'static str,
    },
    #[error("projected envelope final state disagrees with its receipt chain")]
    EnvelopeFinalState,
    #[error("conservation evidence violates the escrow contract: {detail}")]
    InvalidConservation { detail: &'static str },
    #[error("account {account_id} is missing from the conservation input")]
    MissingAccount { account_id: String },
    #[error(
        "account {account_id} has t1_locked {locked} greater than quantity {qty} for {stock_code}"
    )]
    T1Overflow {
        account_id: String,
        stock_code: String,
        qty: u32,
        locked: u32,
    },
    #[error("runtime update is invalid: {detail}")]
    InvalidUpdate { detail: String },
    #[error("event fact identity is invalid: {detail}")]
    InvalidEventIdentity { detail: &'static str },
    #[error("runtime update contains no event facts")]
    EmptyUpdate,
    #[error("Task 9 evidence integer arithmetic overflow at {field}: {value}")]
    IntegerOverflow { field: &'static str, value: u64 },
    #[error("SHA-256 provider returned an invalid lowercase digest for {artifact}")]
    InvalidDigest { artifact: &'static str },
    #[error("save-slot bytes are required before determinism evidence can be produced")]
    MissingSaveBytes,
    #[error("exactly two explicit restore-slot evidence records are required")]
    MissingRestoreSlots,
    #[error("exactly two restore-slot evidence records are required, got {actual}")]
    RestoreSlotCount { actual: usize },
    #[error("restore slot {slot} has no bytes for {field}")]
    EmptyRestoreBytes { slot: String, field: &'static str },
    #[error("restore slot {slot} saved/restored bytes differ")]
    RestoreBytesMismatch { slot: String },
    #[error("restore slot {slot} continuation bytes differ")]
    RestoreContinuationMismatch { slot: String },
    #[error("pre-canonical {dimension} must contain at least two distinct real identities")]
    InvalidPrecanonicalOrder { dimension: &'static str },
    #[error("execution coverage is incomplete: {detail}")]
    IncompleteExecutionCoverage { detail: &'static str },
    #[error("corpus surface {surface} is not supported by the supplied B1/B2 evidence: {detail}")]
    CorpusEvidenceMismatch {
        surface: &'static str,
        detail: &'static str,
    },
    #[error("save v2 evidence failed during {stage}: {detail}")]
    InvalidSaveV2 { stage: &'static str, detail: String },
}

#[derive(Clone, Debug)]
pub struct EnvelopeChainInput<'a> {
    pub envelope: &'a Envelope,
    pub receipts: &'a [EnvelopeReceipt],
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ResourceProjection {
    pub cash_cents: String,
    pub shares: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct EnvelopeKeyProjection {
    pub account_id: String,
    pub stock_code: String,
    pub order_id: String,
    pub side: &'static str,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum ConservationBasisProjection {
    Existing {
        tick_start_live: ResourceProjection,
        p1_live: ResourceProjection,
    },
    Created {
        created: ResourceProjection,
    },
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ReceiptProjection {
    pub receipt_index: String,
    pub journal: &'static str,
    pub source: ReceiptSourceProjection,
    pub transition_ordinal_within_source: String,
    pub kind: &'static str,
    pub live_before: ResourceProjection,
    pub spent: ResourceProjection,
    pub released: ResourceProjection,
    pub live_after: ResourceProjection,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ReceiptSourceProjection {
    pub kind: &'static str,
    pub index: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct EnvelopeConservationProjection {
    pub key: EnvelopeKeyProjection,
    pub origin: &'static str,
    pub basis: ConservationBasisProjection,
    pub receipts: Vec<ReceiptProjection>,
    pub commit_live: ResourceProjection,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct AccountAggregateProjection {
    pub left: ResourceProjection,
    pub right: ResourceProjection,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct PositionProjection {
    pub stock_code: String,
    pub qty: String,
    pub t1_locked: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ConservationAccountProjection {
    pub account_id: String,
    pub cash_cents: String,
    pub positions: Vec<PositionProjection>,
    pub aggregate: AccountAggregateProjection,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ConservationSnapshot {
    pub schema: &'static str,
    pub scenario: String,
    pub seed: String,
    #[serde(serialize_with = "serialize_decimal")]
    pub tick: u64,
    pub envelopes: Vec<EnvelopeConservationProjection>,
    pub accounts: Vec<ConservationAccountProjection>,
}

pub fn project_conservation_snapshot(
    scenario: &str,
    seed: u64,
    tick: u64,
    chains: &[EnvelopeChainInput<'_>],
    accounts: &BTreeMap<AccountId, AccountSnap>,
) -> Result<ConservationSnapshot, EvidenceError> {
    require_text(scenario, "scenario")?;
    if chains.is_empty() {
        return Err(EvidenceError::IncompleteExecutionCoverage {
            detail: "conservation input has no envelope chains",
        });
    }
    let mut projected = Vec::with_capacity(chains.len());
    let mut global_identities = Vec::new();
    let mut aggregates = BTreeMap::<AccountId, (ResourceTotal, ResourceTotal)>::new();
    let mut seen_keys = BTreeSet::new();

    for chain in chains {
        let envelope = chain.envelope;
        validate_stock(&envelope.key().stock)?;
        validate_envelope_axis(envelope)?;
        if !seen_keys.insert(envelope.key().clone()) {
            return Err(EvidenceError::EnvelopeFinalState);
        }
        let basis = envelope.basis();
        let mut live = basis;
        let mut p1_live = basis;
        let mut reached_sealed = false;
        let mut spent_total = ResVec::ZERO;
        let mut released_total = ResVec::ZERO;
        let mut receipts = Vec::with_capacity(chain.receipts.len());
        for receipt in chain.receipts {
            if &receipt.envelope != envelope.key() {
                return Err(EvidenceError::ReceiptEnvelopeMismatch {
                    receipt_index: receipt.index,
                });
            }
            if receipt.local_key.transition_envelope() != &receipt.envelope {
                return Err(EvidenceError::InvalidReceiptIdentity {
                    receipt_index: receipt.index,
                    detail: "local transition envelope disagrees with receipt envelope",
                });
            }
            let journal = match receipt.local_key.journal() {
                JournalRank::PreSeal => {
                    if envelope.origin() != EnvelopeOrigin::TickStart
                        || receipt.kind != ReceiptKind::Release
                    {
                        return Err(EvidenceError::InvalidConservation {
                            detail: "PreSeal must be a Release on an existing envelope",
                        });
                    }
                    "PreSeal"
                }
                JournalRank::SealedBatch => {
                    if !reached_sealed {
                        p1_live = live;
                        reached_sealed = true;
                    }
                    "SealedBatch"
                }
            };
            validate_receipt_axis(envelope.key().side, receipt)?;
            if reached_sealed && receipt.local_key.journal() == JournalRank::PreSeal {
                return Err(EvidenceError::ReceiptChainMismatch {
                    receipt_index: receipt.index,
                });
            }
            let consumed = receipt
                .delta
                .spent
                .checked_add(receipt.delta.released)
                .map_err(|_| EvidenceError::ReceiptChainMismatch {
                    receipt_index: receipt.index,
                })?;
            let expected_after =
                live.checked_sub(consumed)
                    .map_err(|_| EvidenceError::ReceiptChainMismatch {
                        receipt_index: receipt.index,
                    })?;
            if expected_after != receipt.delta.live_after {
                return Err(EvidenceError::ReceiptChainMismatch {
                    receipt_index: receipt.index,
                });
            }
            spent_total = spent_total
                .checked_add(receipt.delta.spent)
                .map_err(|_| EvidenceError::EnvelopeFinalState)?;
            released_total = released_total
                .checked_add(receipt.delta.released)
                .map_err(|_| EvidenceError::EnvelopeFinalState)?;
            global_identities.push((receipt.index, receipt.local_key.clone()));
            receipts.push(ReceiptProjection {
                receipt_index: receipt.index.to_string(),
                journal,
                source: project_receipt_source(receipt.local_key.source()),
                transition_ordinal_within_source: receipt
                    .local_key
                    .transition_ordinal()
                    .to_string(),
                kind: receipt_kind(receipt.kind),
                live_before: project_resource(live, "receipt.live_before")?,
                spent: project_resource(receipt.delta.spent, "receipt.spent")?,
                released: project_resource(receipt.delta.released, "receipt.released")?,
                live_after: project_resource(receipt.delta.live_after, "receipt.live_after")?,
            });
            live = receipt.delta.live_after;
        }
        if !reached_sealed {
            p1_live = live;
        }
        if live != envelope.live()
            || spent_total != envelope.spent()
            || released_total != envelope.released()
        {
            return Err(EvidenceError::EnvelopeFinalState);
        }
        let key = project_envelope_key(envelope.key());
        let basis_projection = match envelope.origin() {
            EnvelopeOrigin::TickStart => ConservationBasisProjection::Existing {
                tick_start_live: project_resource(basis, "envelope.tick_start_live")?,
                p1_live: project_resource(p1_live, "envelope.p1_live")?,
            },
            EnvelopeOrigin::P3Created => ConservationBasisProjection::Created {
                created: project_resource(basis, "envelope.created")?,
            },
        };
        let row_right = resource_total(spent_total, "envelope.spent")?
            .checked_add(resource_total(released_total, "envelope.released")?)?
            .checked_add(resource_total(envelope.live(), "envelope.commit_live")?)?;
        let entry = aggregates.entry(envelope.key().account).or_default();
        entry.0 = entry
            .0
            .checked_add(resource_total(basis, "envelope.basis")?)?;
        entry.1 = entry.1.checked_add(row_right)?;
        projected.push(EnvelopeConservationProjection {
            key,
            origin: match envelope.origin() {
                EnvelopeOrigin::TickStart => "existing",
                EnvelopeOrigin::P3Created => "created",
            },
            basis: basis_projection,
            receipts,
            commit_live: project_resource(envelope.live(), "envelope.commit_live")?,
        });
    }

    validate_global_receipt_identities(&mut global_identities)?;
    let mut projected_accounts = Vec::with_capacity(accounts.len());
    for account_id in aggregates.keys() {
        if !accounts.contains_key(account_id) {
            return Err(EvidenceError::MissingAccount {
                account_id: account_id.0.to_string(),
            });
        }
    }
    for (account_id, account) in accounts {
        let cash = nonnegative_money(account.cash, "account.cash")?;
        let mut positions = Vec::with_capacity(account.positions.len());
        for (stock, position) in &account.positions {
            validate_stock(stock)?;
            if position.t1_locked > position.qty {
                return Err(EvidenceError::T1Overflow {
                    account_id: account_id.0.to_string(),
                    stock_code: stock.0.clone(),
                    qty: position.qty,
                    locked: position.t1_locked,
                });
            }
            positions.push(PositionProjection {
                stock_code: stock.0.clone(),
                qty: position.qty.to_string(),
                t1_locked: position.t1_locked.to_string(),
            });
        }
        let (left, right) = aggregates.get(account_id).copied().unwrap_or_default();
        projected_accounts.push(ConservationAccountProjection {
            account_id: account_id.0.to_string(),
            cash_cents: cash.to_string(),
            positions,
            aggregate: AccountAggregateProjection {
                left: left.project(),
                right: right.project(),
            },
        });
    }
    Ok(ConservationSnapshot {
        schema: CONSERVATION_SCHEMA,
        scenario: scenario.to_owned(),
        seed: seed.to_string(),
        tick,
        envelopes: projected,
        accounts: projected_accounts,
    })
}

fn validate_global_receipt_identities(
    identities: &mut [(u64, ReceiptLocalKey)],
) -> Result<(), EvidenceError> {
    identities.sort_by_key(|(index, _)| *index);
    let mut next_ordinals = BTreeMap::new();
    for (position, (index, local_key)) in identities.iter().enumerate() {
        if position > 0 {
            let (previous_index, previous_key) = &identities[position - 1];
            if previous_index.checked_add(1) != Some(*index) {
                return Err(EvidenceError::ReceiptIndexSequence);
            }
            if previous_key >= local_key {
                return Err(EvidenceError::InvalidReceiptIdentity {
                    receipt_index: *index,
                    detail: "global receipt-index order disagrees with canonical local-key order",
                });
            }
        }
        let domain = (
            local_key.journal(),
            local_key.source(),
            local_key.transition_envelope().clone(),
        );
        let expected = next_ordinals.entry(domain).or_insert(0u64);
        if local_key.transition_ordinal() != *expected {
            return Err(EvidenceError::InvalidReceiptIdentity {
                receipt_index: *index,
                detail: "source-local transition ordinal is not zero-based and contiguous",
            });
        }
        *expected = expected
            .checked_add(1)
            .ok_or(EvidenceError::InvalidReceiptIdentity {
                receipt_index: *index,
                detail: "source-local transition ordinal overflow",
            })?;
    }
    Ok(())
}

fn validate_envelope_axis(envelope: &Envelope) -> Result<(), EvidenceError> {
    let values = [
        envelope.basis(),
        envelope.spent(),
        envelope.released(),
        envelope.live(),
    ];
    let valid = match envelope.key().side {
        Side::Buy => values.iter().all(|value| value.shares == 0),
        Side::Sell => values.iter().all(|value| value.cash == Money::ZERO),
    };
    if valid {
        Ok(())
    } else {
        Err(EvidenceError::InvalidConservation {
            detail: "envelope resource axis disagrees with its side",
        })
    }
}

fn validate_receipt_axis(side: Side, receipt: &EnvelopeReceipt) -> Result<(), EvidenceError> {
    if receipt.kind != ReceiptKind::Fill && receipt.delta.spent != ResVec::ZERO {
        return Err(EvidenceError::InvalidConservation {
            detail: "non-Fill receipt spends escrow resources",
        });
    }
    let values = [
        receipt.delta.spent,
        receipt.delta.released,
        receipt.delta.live_after,
    ];
    let valid_axis = match side {
        Side::Buy => values.iter().all(|value| value.shares == 0),
        Side::Sell => values.iter().all(|value| value.cash == Money::ZERO),
    };
    let valid_fill = match (side, receipt.kind) {
        (Side::Buy, ReceiptKind::Fill) => receipt.delta.spent.cash.cents() > 0,
        (Side::Sell, ReceiptKind::Fill) => receipt.delta.spent.shares > 0,
        (_, _) => true,
    };
    if valid_axis && valid_fill {
        Ok(())
    } else {
        Err(EvidenceError::InvalidConservation {
            detail: "receipt resource axis or Fill spend disagrees with its side",
        })
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct ResourceTotal {
    cash: u64,
    shares: u64,
}

impl ResourceTotal {
    fn checked_add(self, other: Self) -> Result<Self, EvidenceError> {
        Ok(Self {
            cash: self.cash.checked_add(other.cash).ok_or(
                EvidenceError::IncompleteExecutionCoverage {
                    detail: "cash aggregate overflow",
                },
            )?,
            shares: self.shares.checked_add(other.shares).ok_or(
                EvidenceError::IncompleteExecutionCoverage {
                    detail: "share aggregate overflow",
                },
            )?,
        })
    }

    fn project(self) -> ResourceProjection {
        ResourceProjection {
            cash_cents: self.cash.to_string(),
            shares: self.shares.to_string(),
        }
    }
}

fn resource_total(value: ResVec, field: &'static str) -> Result<ResourceTotal, EvidenceError> {
    Ok(ResourceTotal {
        cash: nonnegative_money(value.cash, field)?,
        shares: u64::from(value.shares),
    })
}

fn project_resource(
    value: ResVec,
    field: &'static str,
) -> Result<ResourceProjection, EvidenceError> {
    let cash = nonnegative_money(value.cash, field)?;
    Ok(ResourceProjection {
        cash_cents: cash.to_string(),
        shares: value.shares.to_string(),
    })
}

fn nonnegative_money(value: Money, field: &'static str) -> Result<u64, EvidenceError> {
    u64::try_from(value.cents()).map_err(|_| EvidenceError::NegativeMoney {
        field,
        cents: value.cents(),
    })
}

fn project_envelope_key(key: &crate::session::pipeline::EnvelopeKey) -> EnvelopeKeyProjection {
    EnvelopeKeyProjection {
        account_id: key.account.0.to_string(),
        stock_code: key.stock.0.clone(),
        order_id: key.order.0.to_string(),
        side: side_name(key.side),
    }
}

fn side_name(side: Side) -> &'static str {
    match side {
        Side::Buy => "Buy",
        Side::Sell => "Sell",
    }
}

fn receipt_kind(kind: ReceiptKind) -> &'static str {
    match kind {
        ReceiptKind::Fill => "Fill",
        ReceiptKind::Release => "Release",
        ReceiptKind::Reject => "Reject",
        ReceiptKind::Rollover => "Rollover",
    }
}

fn project_receipt_source(source: ReceiptSource) -> ReceiptSourceProjection {
    let kind = match source {
        ReceiptSource::P0Expiry(_) => "P0Expiry",
        ReceiptSource::SealedIntent(_) => "SealedIntent",
        ReceiptSource::Auction(_) => "Auction",
        ReceiptSource::DayEnd(_) => "DayEnd",
    };
    ReceiptSourceProjection {
        kind,
        index: source.payload().to_string(),
    }
}

fn require_text(value: &str, field: &'static str) -> Result<(), EvidenceError> {
    if value.is_empty() {
        Err(EvidenceError::EmptyField { field })
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub enum RuntimeUpdateRef<'a> {
    Tick(&'a TickFrame),
    Civil(&'a CivilUpdate),
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ComparisonEventFact {
    pub comparison_event_key: (String, String, String, String),
    pub event: Value,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct UpdateProjection {
    pub kind: &'static str,
    #[serde(serialize_with = "serialize_decimal")]
    pub tick: u64,
    #[serde(serialize_with = "serialize_decimal")]
    pub seq_from: u64,
    #[serde(serialize_with = "serialize_decimal")]
    pub seq_to: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeseries_payload: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub civil_payload: Option<Value>,
    pub events: Vec<ComparisonEventFact>,
}

pub fn project_update(update: RuntimeUpdateRef<'_>) -> Result<UpdateProjection, EvidenceError> {
    let mut next_ordinals = BTreeMap::new();
    let mut comparison_keys = BTreeSet::new();
    project_update_in_scope(
        update,
        &mut next_ordinals,
        &mut comparison_keys,
        "local_event_index is not zero-based and contiguous in its ADR domain",
        "comparison_event_key is duplicated within one runtime update",
    )
}

type EventOrdinalDomain = (u64, u8, EntityTag, EventSourceIndex);
type ComparisonEventKey = (String, String, String, String);

fn project_update_in_scope(
    update: RuntimeUpdateRef<'_>,
    next_ordinals: &mut BTreeMap<EventOrdinalDomain, u64>,
    comparison_keys: &mut BTreeSet<ComparisonEventKey>,
    ordinal_error: &'static str,
    duplicate_error: &'static str,
) -> Result<UpdateProjection, EvidenceError> {
    let (kind, tick, seq_cursor, seq_to, facts, tick_payload, civil_payload) = match update {
        RuntimeUpdateRef::Tick(frame) => (
            "TickFrame",
            frame.tick,
            frame.seq_from,
            frame.seq_to,
            frame.facts.as_slice(),
            Some(project_timeseries_payload(&frame.timeseries_payload)?),
            None,
        ),
        RuntimeUpdateRef::Civil(update) => (
            "CivilUpdate",
            update.tick,
            update.seq_from,
            update.seq_to,
            update.facts.as_slice(),
            None,
            Some(project_civil_payload(update)?),
        ),
    };
    validate_event_fact_identities(
        tick,
        facts,
        next_ordinals,
        comparison_keys,
        ordinal_error,
        duplicate_error,
    )?;
    match update {
        RuntimeUpdateRef::Tick(frame) => frame.validate(),
        RuntimeUpdateRef::Civil(update) => update.validate(),
    }
    .map_err(|error| EvidenceError::InvalidUpdate {
        detail: error.to_string(),
    })?;
    if facts.is_empty() {
        return Err(EvidenceError::EmptyUpdate);
    }
    let seq_from = seq_cursor
        .checked_add(1)
        .ok_or(EvidenceError::IntegerOverflow {
            field: "update.seq_from",
            value: seq_cursor,
        })?;

    let events = facts
        .iter()
        .map(|fact| {
            let (variant, event) = project_event(&fact.event)?;
            Ok(ComparisonEventFact {
                comparison_event_key: comparison_event_key(tick, fact, variant)?,
                event,
            })
        })
        .collect::<Result<Vec<_>, EvidenceError>>()?;
    Ok(UpdateProjection {
        kind,
        tick,
        seq_from,
        seq_to,
        timeseries_payload: tick_payload,
        civil_payload,
        events,
    })
}

fn validate_event_fact_identities(
    tick: u64,
    facts: &[EventFact],
    next_ordinals: &mut BTreeMap<EventOrdinalDomain, u64>,
    comparison_keys: &mut BTreeSet<ComparisonEventKey>,
    ordinal_error: &'static str,
    duplicate_error: &'static str,
) -> Result<(), EvidenceError> {
    for fact in facts {
        let (phase_rank, entity, source) = expected_event_identity(&fact.event);
        if fact.key.phase_rank() != phase_rank {
            return Err(EvidenceError::InvalidEventIdentity {
                detail: "phase_rank disagrees with the ADR event mapping",
            });
        }
        if fact.key.entity() != &entity {
            return Err(EvidenceError::InvalidEventIdentity {
                detail: "entity disagrees with the ADR event mapping",
            });
        }
        if fact.key.source() != source {
            return Err(EvidenceError::InvalidEventIdentity {
                detail: "source disagrees with the ADR event mapping",
            });
        }
        let key = comparison_event_key(tick, fact, event_variant(&fact.event))?;
        if !comparison_keys.insert(key) {
            return Err(EvidenceError::InvalidEventIdentity {
                detail: duplicate_error,
            });
        }
    }
    let mut sorted: Vec<_> = facts.iter().collect();
    sorted.sort_by(|left, right| left.key.cmp(&right.key));
    for fact in sorted {
        let (phase_rank, entity, source) = expected_event_identity(&fact.event);
        let domain = (tick, phase_rank, entity, source);
        let expected = next_ordinals.entry(domain).or_insert(0u64);
        if fact.key.local_event_index() != *expected {
            return Err(EvidenceError::InvalidEventIdentity {
                detail: ordinal_error,
            });
        }
        *expected = expected
            .checked_add(1)
            .ok_or(EvidenceError::InvalidEventIdentity {
                detail: "local_event_index overflow",
            })?;
    }
    Ok(())
}

fn comparison_event_key(
    tick: u64,
    fact: &EventFact,
    variant: &str,
) -> Result<ComparisonEventKey, EvidenceError> {
    Ok((
        tick.to_string(),
        format!("{}:{variant}", fact.key.phase_rank()),
        event_key_entity(fact.key.entity())?,
        fact.key.local_event_index().to_string(),
    ))
}

fn expected_event_identity(event: &Event) -> (u8, EntityTag, EventSourceIndex) {
    match event {
        Event::Trade { code, .. } => (4, EntityTag::Stock(code.clone()), EventSourceIndex::Sealed),
        Event::AuctionTick { code, .. }
        | Event::AuctionCompleted { code, .. }
        | Event::PriceTick { code, .. } => (
            4,
            EntityTag::Stock(code.clone()),
            EventSourceIndex::PriceTick,
        ),
        Event::IntentRejected { account, .. }
        | Event::SettlementError { account, .. }
        | Event::OrderCanceled { account, .. }
        | Event::OrderAccepted { account, .. } => {
            (4, EntityTag::Account(*account), EventSourceIndex::Sealed)
        }
        Event::DayBoundary { .. } => (5, EntityTag::Session, EventSourceIndex::DayEnd),
        Event::CivilDateAdvanced { .. }
        | Event::CompanyDisclosurePublished { .. }
        | Event::ResourceLimit { .. } => (6, EntityTag::Session, EventSourceIndex::Session),
    }
}

fn event_key_entity(entity: &EntityTag) -> Result<String, EvidenceError> {
    match entity {
        EntityTag::Session => Ok("Session".to_owned()),
        EntityTag::Stock(code) => {
            validate_stock(code)?;
            Ok(format!("Stock:{}", code.0))
        }
        EntityTag::Account(account) => Ok(format!("Account:{}", account.0)),
    }
}

fn serialize_decimal<T, S>(value: &T, serializer: S) -> Result<S::Ok, S::Error>
where
    T: Display,
    S: Serializer,
{
    serializer.serialize_str(&value.to_string())
}

fn event_variant(event: &Event) -> &'static str {
    match event {
        Event::Trade { .. } => "Trade",
        Event::AuctionTick { .. } => "AuctionTick",
        Event::AuctionCompleted { .. } => "AuctionCompleted",
        Event::PriceTick { .. } => "PriceTick",
        Event::DayBoundary { .. } => "DayBoundary",
        Event::CivilDateAdvanced { .. } => "CivilDateAdvanced",
        Event::CompanyDisclosurePublished { .. } => "CompanyDisclosurePublished",
        Event::IntentRejected { .. } => "IntentRejected",
        Event::SettlementError { .. } => "SettlementError",
        Event::ResourceLimit { .. } => "ResourceLimit",
        Event::OrderCanceled { .. } => "OrderCanceled",
        Event::OrderAccepted { .. } => "OrderAccepted",
    }
}

fn event_entity(event: &Event) -> Result<String, EvidenceError> {
    match event {
        Event::Trade { code, .. }
        | Event::AuctionTick { code, .. }
        | Event::AuctionCompleted { code, .. }
        | Event::PriceTick { code, .. } => {
            validate_stock(code)?;
            Ok(format!("Stock:{}", code.0))
        }
        Event::IntentRejected { account, .. }
        | Event::SettlementError { account, .. }
        | Event::OrderCanceled { account, .. }
        | Event::OrderAccepted { account, .. } => Ok(format!("Account:{}", account.0)),
        Event::DayBoundary { .. }
        | Event::CivilDateAdvanced { .. }
        | Event::CompanyDisclosurePublished { .. }
        | Event::ResourceLimit { .. } => Ok("Session".to_owned()),
    }
}

fn project_event(event: &Event) -> Result<(&'static str, Value), EvidenceError> {
    let variant = event_variant(event);
    event_entity(event)?;
    let payload = match event {
        Event::Trade {
            seq,
            code,
            price,
            qty,
            maker,
            taker,
        } => serde_json::json!({
            "seq": seq.to_string(),
            "code": code.0,
            "price": price.cents().to_string(),
            "qty": qty.to_string(),
            "maker": maker.0.to_string(),
            "taker": taker.0.to_string(),
        }),
        Event::AuctionTick {
            seq,
            tick,
            phase,
            code,
            indicative_price,
            matched_volume,
            imbalance,
        } => serde_json::json!({
            "seq": seq.to_string(),
            "tick": tick.to_string(),
            "phase": trading_phase(*phase),
            "code": code.0,
            "indicative_price": indicative_price.map(|price| price.cents().to_string()),
            "matched_volume": matched_volume.to_string(),
            "imbalance": imbalance.to_string(),
        }),
        Event::AuctionCompleted {
            seq,
            tick,
            phase,
            code,
            clearing_price,
            matched_volume,
        } => serde_json::json!({
            "seq": seq.to_string(),
            "tick": tick.to_string(),
            "phase": trading_phase(*phase),
            "code": code.0,
            "clearing_price": clearing_price.map(|price| price.cents().to_string()),
            "matched_volume": matched_volume.to_string(),
        }),
        Event::PriceTick {
            seq,
            tick,
            code,
            last_price,
            daily_candle,
            bids,
            asks,
        } => serde_json::json!({
            "seq": seq.to_string(),
            "tick": tick.to_string(),
            "code": code.0,
            "last_price": last_price.cents().to_string(),
            "daily_candle": project_candle(daily_candle)?,
            "bids": project_depth(bids, "PriceTick.bids")?,
            "asks": project_depth(asks, "PriceTick.asks")?,
        }),
        Event::DayBoundary {
            seq,
            day,
            closed_daily_candles,
        } => serde_json::json!({
            "seq": seq.to_string(),
            "day": day.to_string(),
            "closed_daily_candles": project_candle_map(closed_daily_candles)?,
        }),
        Event::CivilDateAdvanced {
            seq,
            settled_date,
            next_date,
            next_status,
        } => serde_json::json!({
            "seq": seq.to_string(),
            "settled_date": project_date(*settled_date),
            "next_date": project_date(*next_date),
            "next_status": project_day_status(next_status),
        }),
        Event::CompanyDisclosurePublished {
            seq,
            publication_id,
            company,
            published_at,
            kind,
        } => serde_json::json!({
            "seq": seq.to_string(),
            "publication_id": publication_id.value().to_string(),
            "company": company.0,
            "published_at": {
                "date": project_date(published_at.date()),
                "second_of_day": published_at.second_of_day().to_string(),
            },
            "kind": project_disclosure_kind(kind),
        }),
        Event::IntentRejected {
            seq,
            account,
            code,
            reason,
        } => serde_json::json!({
            "seq": seq.to_string(),
            "account": account.0.to_string(),
            "code": code.0,
            "reason": rejection_reason(reason),
        }),
        Event::SettlementError {
            seq,
            account,
            code,
            reason,
        } => serde_json::json!({
            "seq": seq.to_string(),
            "account": account.0.to_string(),
            "code": code.0,
            "reason": reason,
        }),
        Event::ResourceLimit {
            seq,
            resource,
            limit,
        } => serde_json::json!({
            "seq": seq.to_string(),
            "resource": match resource {
                crate::session::RuntimeResource::PendingPlanEvents => "PendingPlanEvents",
            },
            "limit": limit.to_string(),
        }),
        Event::OrderCanceled {
            seq,
            account,
            code,
            id,
            remaining_qty,
        } => serde_json::json!({
            "seq": seq.to_string(),
            "account": account.0.to_string(),
            "code": code.0,
            "id": id.0.to_string(),
            "remaining_qty": remaining_qty.to_string(),
        }),
        Event::OrderAccepted {
            seq,
            account,
            code,
            id,
            side,
            price,
            remaining_qty,
        } => serde_json::json!({
            "seq": seq.to_string(),
            "account": account.0.to_string(),
            "code": code.0,
            "id": id.0.to_string(),
            "side": side_name(*side),
            "price": price.cents().to_string(),
            "remaining_qty": remaining_qty.to_string(),
        }),
    };
    Ok((
        variant,
        Value::Object(serde_json::Map::from_iter([(variant.to_owned(), payload)])),
    ))
}

fn project_timeseries_payload(
    payload: &crate::session::protocol::TickTimeseriesPayload,
) -> Result<Value, EvidenceError> {
    let markets = payload
        .markets
        .iter()
        .map(|(code, market)| {
            validate_stock(code)?;
            Ok((
                code.0.clone(),
                serde_json::json!({
                    "last_price": market.last_price.cents().to_string(),
                    "last_close": market.last_close.cents().to_string(),
                    "best_bid": market.best_bid.map(|price| price.cents().to_string()),
                    "best_ask": market.best_ask.map(|price| price.cents().to_string()),
                    "bids": project_depth(&market.bids, "timeseries.markets.bids")?,
                    "asks": project_depth(&market.asks, "timeseries.markets.asks")?,
                }),
            ))
        })
        .collect::<Result<serde_json::Map<_, _>, EvidenceError>>()?;
    let auction_points = payload
        .auction_points
        .iter()
        .map(|(code, points)| {
            validate_stock(code)?;
            let projected = points
                .iter()
                .map(|point| {
                    Ok(serde_json::json!({
                        "key": project_stable_key(&point.key)?,
                        "tick": point.tick.to_string(),
                        "kind": match point.kind {
                            crate::session::protocol::AuctionPointKind::Indication => "Indication",
                            crate::session::protocol::AuctionPointKind::Completion => "Completion",
                        },
                        "phase": trading_phase(point.phase),
                        "indicative_price": point.indicative_price.map(|price| price.cents().to_string()),
                        "matched_volume": point.matched_volume.to_string(),
                        "imbalance": point.imbalance.map(|value| value.to_string()),
                    }))
                })
                .collect::<Result<Vec<_>, EvidenceError>>()?;
            Ok((code.0.clone(), Value::Array(projected)))
        })
        .collect::<Result<serde_json::Map<_, _>, EvidenceError>>()?;
    let continuous_points = payload
        .continuous_points
        .iter()
        .map(|(code, point)| {
            validate_stock(code)?;
            Ok((
                code.0.clone(),
                serde_json::json!({
                    "tick": point.tick.to_string(),
                    "phase": trading_phase(point.phase),
                    "last_price": point.last_price.cents().to_string(),
                    "cumulative_volume": point.cumulative_volume.to_string(),
                    "bids": project_depth(&point.bids, "timeseries.continuous.bids")?,
                    "asks": project_depth(&point.asks, "timeseries.continuous.asks")?,
                }),
            ))
        })
        .collect::<Result<serde_json::Map<_, _>, EvidenceError>>()?;
    Ok(serde_json::json!({
        "markets": markets,
        "active_daily_candles": project_candle_map(&payload.active_daily_candles)?,
        "closed_daily_candles": project_candle_map(&payload.closed_daily_candles)?,
        "auction_points": auction_points,
        "continuous_points": continuous_points,
    }))
}

fn project_civil_payload(update: &CivilUpdate) -> Result<Value, EvidenceError> {
    let security_codes = update
        .refresh
        .securities
        .iter()
        .map(|security| {
            validate_stock(&security.code)?;
            Ok(security.code.0.clone())
        })
        .collect::<Result<Vec<_>, EvidenceError>>()?;
    Ok(serde_json::json!({
        "boundary": {
            "settled_date": project_date(update.boundary.settled_date),
            "settled_phase": match update.boundary.settled_phase {
                crate::session::CivilPhase::IntradayTrading => "IntradayTrading",
                crate::session::CivilPhase::ClosedDay => "ClosedDay",
            },
            "next_date": project_date(update.boundary.next_date),
            "next_status": project_day_status(&update.boundary.next_status),
        },
        "kinds": update.kinds.iter().map(|kind| match kind {
            crate::session::protocol::CivilUpdateKind::AfterClose => "AfterClose",
            crate::session::protocol::CivilUpdateKind::BeforeOpen => "BeforeOpen",
            crate::session::protocol::CivilUpdateKind::CivilAdvance => "CivilAdvance",
        }).collect::<Vec<_>>(),
        "civil_date": update.civil_date,
        "refresh": {
            "ticks_per_day": update.refresh.ticks_per_day.to_string(),
            "snapshot_tick": update.refresh.snapshot.tick.to_string(),
            "snapshot_seq": update.refresh.snapshot.seq.to_string(),
            "security_codes": security_codes,
            "intraday_ticks": update.refresh.intraday.iter().map(|frame| {
                frame.tick.to_string()
            }).collect::<Vec<_>>(),
            "public_publication_ids": update.refresh.public_publication_ids,
        },
    }))
}

fn project_stable_key(
    key: &crate::session::pipeline::EventStableKey,
) -> Result<Value, EvidenceError> {
    let entity = match key.entity() {
        crate::session::pipeline::EntityTag::Session => serde_json::json!("Session"),
        crate::session::pipeline::EntityTag::Stock(code) => {
            validate_stock(code)?;
            serde_json::json!({ "Stock": code.0 })
        }
        crate::session::pipeline::EntityTag::Account(account) => {
            serde_json::json!({ "Account": account.0.to_string() })
        }
    };
    Ok(serde_json::json!({
        "phase_rank": key.phase_rank().to_string(),
        "entity": entity,
        "source": match key.source() {
            crate::session::pipeline::EventSourceIndex::Sealed => "Sealed",
            crate::session::pipeline::EventSourceIndex::P0 => "P0",
            crate::session::pipeline::EventSourceIndex::PriceTick => "PriceTick",
            crate::session::pipeline::EventSourceIndex::DayEnd => "DayEnd",
            crate::session::pipeline::EventSourceIndex::Session => "Session",
        },
        "local_event_index": key.local_event_index().to_string(),
    }))
}

fn project_depth(depth: &[(Money, u64)], field: &'static str) -> Result<Vec<Value>, EvidenceError> {
    depth
        .iter()
        .map(|(price, qty)| {
            let _ = field;
            Ok(serde_json::json!([
                price.cents().to_string(),
                qty.to_string()
            ]))
        })
        .collect()
}

fn project_candle(candle: &crate::DailyCandle) -> Result<Value, EvidenceError> {
    let mut value = serde_json::json!({
        "time": candle.time.to_string(),
        "open": candle.open.cents().to_string(),
        "high": candle.high.cents().to_string(),
        "low": candle.low.cents().to_string(),
        "close": candle.close.cents().to_string(),
        "volume": candle.volume.to_string(),
    });
    if let Some(stats) = &candle.trade_stats {
        value
            .as_object_mut()
            .expect("JSON object constructed above")
            .insert(
                "trade_stats".to_owned(),
                serde_json::json!({
                    "turnover_cents": stats.turnover_cents.to_string(),
                    "trade_count": stats.trade_count.to_string(),
                }),
            );
    }
    Ok(value)
}

fn project_candle_map(
    candles: &BTreeMap<StockCode, crate::DailyCandle>,
) -> Result<Value, EvidenceError> {
    candles
        .iter()
        .map(|(code, candle)| {
            validate_stock(code)?;
            Ok((code.0.clone(), project_candle(candle)?))
        })
        .collect::<Result<serde_json::Map<_, _>, EvidenceError>>()
        .map(Value::Object)
}

fn project_date(date: crate::CivilDate) -> Value {
    serde_json::json!({
        "year": date.year().to_string(),
        "month": date.month().to_string(),
        "day": date.day().to_string(),
    })
}

fn project_day_status(status: &crate::DayStatus) -> Value {
    use crate::calendar::{ClosedReason, HolidayKind};
    match status {
        crate::DayStatus::Trading => serde_json::json!("Trading"),
        crate::DayStatus::Closed(reason) => {
            let reason = match reason {
                ClosedReason::Weekend => serde_json::json!("Weekend"),
                ClosedReason::OfficialHoliday { citation_id } => {
                    serde_json::json!({ "OfficialHoliday": { "citation_id": citation_id } })
                }
                ClosedReason::SimulatedHoliday(kind) => serde_json::json!({
                    "SimulatedHoliday": match kind {
                        HolidayKind::NewYearDay => "NewYearDay",
                        HolidayKind::LabourDay => "LabourDay",
                        HolidayKind::NationalDay => "NationalDay",
                        HolidayKind::SpringFestival => "SpringFestival",
                        HolidayKind::Qingming => "Qingming",
                        HolidayKind::DragonBoat => "DragonBoat",
                        HolidayKind::MidAutumn => "MidAutumn",
                    }
                }),
            };
            serde_json::json!({ "Closed": reason })
        }
    }
}

fn project_disclosure_kind(kind: &crate::session::CompanyDisclosureKind) -> Value {
    match kind {
        crate::session::CompanyDisclosureKind::Report { report_revision } => {
            serde_json::json!({ "Report": { "report_revision": report_revision.to_string() } })
        }
        crate::session::CompanyDisclosureKind::Announcement => serde_json::json!("Announcement"),
    }
}

fn rejection_reason(reason: &crate::RejectionReason) -> &'static str {
    match reason {
        crate::RejectionReason::InsufficientCash => "InsufficientCash",
        crate::RejectionReason::InsufficientShares => "InsufficientShares",
        crate::RejectionReason::LimitExceeded => "LimitExceeded",
        crate::RejectionReason::PriceCageExceeded => "PriceCageExceeded",
        crate::RejectionReason::UnknownStock => "UnknownStock",
        crate::RejectionReason::AuctionLimitOrderRequired => "AuctionLimitOrderRequired",
        crate::RejectionReason::AuctionOrderNotCancelable => "AuctionOrderNotCancelable",
        crate::RejectionReason::AuctionOrderEntryClosed => "AuctionOrderEntryClosed",
        crate::RejectionReason::InvalidQuantity => "InvalidQuantity",
        crate::RejectionReason::ResourceLimitExceeded => "ResourceLimitExceeded",
        crate::RejectionReason::OrderNotFound => "OrderNotFound",
        crate::RejectionReason::SameTickOrderNotCancelable => "SameTickOrderNotCancelable",
        crate::RejectionReason::NotOrderOwner => "NotOrderOwner",
    }
}

fn trading_phase(phase: crate::TradingPhase) -> &'static str {
    match phase {
        crate::TradingPhase::CallAuction => "CallAuction",
        crate::TradingPhase::PreOpen => "PreOpen",
        crate::TradingPhase::ClosingAuction => "ClosingAuction",
        crate::TradingPhase::Continuous => "Continuous",
    }
}

fn validate_stock(code: &StockCode) -> Result<(), EvidenceError> {
    if code.0.len() == 6 && code.0.bytes().all(|byte| byte.is_ascii_digit()) {
        Ok(())
    } else {
        Err(EvidenceError::InvalidStockCode {
            code: code.0.clone(),
        })
    }
}

pub trait Sha256Provider {
    fn digest_hex(&self, bytes: &[u8]) -> String;
}

#[derive(Clone, Copy, Debug)]
pub struct ObservationArtifactBytes<'a> {
    pub authoritative_state: &'a [u8],
    pub event_stream: &'a [u8],
    pub receipts: &'a [u8],
    pub save_slot: Option<&'a [u8]>,
}

#[derive(Clone, Copy, Debug)]
pub struct RestoreSlotBytes<'a> {
    pub slot: &'a str,
    pub saved: &'a [u8],
    pub restored: &'a [u8],
    pub uninterrupted_continuation: &'a [u8],
    pub restored_continuation: &'a [u8],
}

#[derive(Clone, Debug)]
pub struct PrecanonicalOrderInput<'a> {
    pub account_shards: &'a [String],
    pub stock_shards: &'a [String],
    pub completion_order: &'a [String],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FinalizerAuditInput {
    pub auction_finalizations: u64,
    pub day_end_finalizations: u64,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ArtifactReceipt {
    #[serde(serialize_with = "serialize_decimal")]
    pub byte_length: usize,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ObservationArtifacts {
    pub authoritative_state: ArtifactReceipt,
    pub event_stream: ArtifactReceipt,
    pub receipts: ArtifactReceipt,
    pub save_slot: ArtifactReceipt,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct PrecanonicalOrder {
    pub account_shards: Vec<String>,
    pub stock_shards: Vec<String>,
    pub completion_order: Vec<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct RestoreSlotProjection {
    pub slot: String,
    pub saved: ArtifactReceipt,
    pub restored: ArtifactReceipt,
    pub uninterrupted_continuation: ArtifactReceipt,
    pub restored_continuation: ArtifactReceipt,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ExecutionCoverage {
    #[serde(serialize_with = "serialize_decimal")]
    pub tick_from: u64,
    #[serde(serialize_with = "serialize_decimal")]
    pub tick_to: u64,
    #[serde(serialize_with = "serialize_decimal")]
    pub auction_finalizations: u64,
    #[serde(serialize_with = "serialize_decimal")]
    pub day_end_finalizations: u64,
    pub stock_codes: Vec<String>,
    pub multi_leg_order_ids: Vec<String>,
    pub restore_slots: Vec<RestoreSlotProjection>,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub enum ObservationMode {
    #[serde(rename = "canonical")]
    Canonical,
    #[serde(rename = "perturbed")]
    Perturbed,
    #[serde(rename = "negative-control")]
    NegativeControl,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct DeterminismObservation {
    pub schema: &'static str,
    pub scenario: String,
    pub seed: String,
    pub budget: String,
    #[serde(serialize_with = "serialize_decimal")]
    pub repeat: u32,
    pub mode: ObservationMode,
    pub canonical_merge_disabled: Option<String>,
    pub artifacts: ObservationArtifacts,
    pub precanonical_order: PrecanonicalOrder,
    pub execution_coverage: ExecutionCoverage,
}

pub struct ObservationInput<'a> {
    pub scenario: &'a str,
    pub seed: u64,
    pub budget: &'a str,
    pub repeat: u32,
    pub mode: ObservationMode,
    pub canonical_merge_disabled: Option<&'a str>,
    pub artifacts: ObservationArtifactBytes<'a>,
    pub precanonical_order: PrecanonicalOrderInput<'a>,
    pub finalizers: &'a [FinalizerAuditInput],
    pub conservation: &'a [ConservationSnapshot],
    pub restore_slots: Option<&'a [RestoreSlotBytes<'a>]>,
}

pub fn project_observation(
    input: ObservationInput<'_>,
    sha256: &impl Sha256Provider,
) -> Result<DeterminismObservation, EvidenceError> {
    let save_slot = input
        .artifacts
        .save_slot
        .filter(|bytes| !bytes.is_empty())
        .ok_or(EvidenceError::MissingSaveBytes)?;
    let restore_inputs = input
        .restore_slots
        .ok_or(EvidenceError::MissingRestoreSlots)?;
    if restore_inputs.len() != 2 {
        return Err(EvidenceError::RestoreSlotCount {
            actual: restore_inputs.len(),
        });
    }
    require_text(input.scenario, "scenario")?;
    if !matches!(input.budget, "1" | "2" | "4" | "auto") {
        return Err(EvidenceError::IncompleteExecutionCoverage {
            detail: "budget is not 1, 2, 4, or auto",
        });
    }
    match (input.mode, input.canonical_merge_disabled) {
        (ObservationMode::NegativeControl, Some("account" | "stock" | "completion")) => {}
        (ObservationMode::NegativeControl, _) => {
            return Err(EvidenceError::IncompleteExecutionCoverage {
                detail: "negative control lacks one real disabled merge dimension",
            });
        }
        (_, None) => {}
        (_, Some(_)) => {
            return Err(EvidenceError::IncompleteExecutionCoverage {
                detail: "canonical merge is disabled outside a negative control",
            });
        }
    }
    let precanonical_order = PrecanonicalOrder {
        account_shards: validate_real_identities(
            input.precanonical_order.account_shards,
            "account_shards",
        )?,
        stock_shards: validate_real_identities(
            input.precanonical_order.stock_shards,
            "stock_shards",
        )?,
        completion_order: validate_real_identities(
            input.precanonical_order.completion_order,
            "completion_order",
        )?,
    };
    let execution_coverage = project_execution_coverage(
        input.scenario,
        input.seed,
        input.conservation,
        input.finalizers,
        restore_inputs,
        sha256,
    )?;
    let artifacts = ObservationArtifacts {
        authoritative_state: artifact_receipt(
            input.artifacts.authoritative_state,
            "authoritative_state",
            sha256,
        )?,
        event_stream: artifact_receipt(input.artifacts.event_stream, "event_stream", sha256)?,
        receipts: artifact_receipt(input.artifacts.receipts, "receipts", sha256)?,
        save_slot: artifact_receipt(save_slot, "save_slot", sha256)?,
    };
    Ok(DeterminismObservation {
        schema: OBSERVATION_SCHEMA,
        scenario: input.scenario.to_owned(),
        seed: input.seed.to_string(),
        budget: input.budget.to_owned(),
        repeat: input.repeat,
        mode: input.mode,
        canonical_merge_disabled: input.canonical_merge_disabled.map(str::to_owned),
        artifacts,
        precanonical_order,
        execution_coverage,
    })
}

fn validate_real_identities(
    values: &[String],
    dimension: &'static str,
) -> Result<Vec<String>, EvidenceError> {
    let unique: BTreeSet<_> = values.iter().collect();
    if values.len() < 2
        || unique.len() != values.len()
        || values.iter().any(|value| value.is_empty())
    {
        return Err(EvidenceError::InvalidPrecanonicalOrder { dimension });
    }
    Ok(values.to_vec())
}

fn artifact_receipt(
    bytes: &[u8],
    artifact: &'static str,
    sha256: &(impl Sha256Provider + ?Sized),
) -> Result<ArtifactReceipt, EvidenceError> {
    if bytes.is_empty() {
        return Err(EvidenceError::EmptyField { field: artifact });
    }
    let digest = sha256.digest_hex(bytes);
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(EvidenceError::InvalidDigest { artifact });
    }
    Ok(ArtifactReceipt {
        byte_length: bytes.len(),
        sha256: digest,
    })
}

fn project_execution_coverage(
    scenario: &str,
    seed: u64,
    snapshots: &[ConservationSnapshot],
    finalizers: &[FinalizerAuditInput],
    restore_inputs: &[RestoreSlotBytes<'_>],
    sha256: &impl Sha256Provider,
) -> Result<ExecutionCoverage, EvidenceError> {
    if snapshots.is_empty() {
        return Err(EvidenceError::IncompleteExecutionCoverage {
            detail: "no per-tick conservation snapshots",
        });
    }
    let mut ticks = BTreeSet::new();
    let mut stock_codes = BTreeSet::new();
    let mut multi_leg_order_ids = BTreeSet::new();
    for snapshot in snapshots {
        if snapshot.scenario != scenario || snapshot.seed != seed.to_string() {
            return Err(EvidenceError::IncompleteExecutionCoverage {
                detail: "conservation identity differs from observation identity",
            });
        }
        if !ticks.insert(snapshot.tick) {
            return Err(EvidenceError::IncompleteExecutionCoverage {
                detail: "duplicate conservation tick",
            });
        }
        for envelope in &snapshot.envelopes {
            validate_stock(&StockCode(envelope.key.stock_code.clone()))?;
            stock_codes.insert(envelope.key.stock_code.clone());
            if envelope
                .receipts
                .iter()
                .filter(|receipt| receipt.journal == "SealedBatch" && receipt.kind == "Fill")
                .count()
                >= 2
            {
                multi_leg_order_ids.insert(envelope.key.order_id.clone());
            }
        }
    }
    let tick_from = *ticks
        .first()
        .ok_or(EvidenceError::IncompleteExecutionCoverage {
            detail: "no conservation tick",
        })?;
    let tick_to = *ticks
        .last()
        .ok_or(EvidenceError::IncompleteExecutionCoverage {
            detail: "no conservation tick",
        })?;
    let expected_len = tick_to
        .checked_sub(tick_from)
        .and_then(|width| width.checked_add(1))
        .and_then(|width| usize::try_from(width).ok())
        .ok_or(EvidenceError::IncompleteExecutionCoverage {
            detail: "tick coverage overflow",
        })?;
    if ticks.len() != expected_len {
        return Err(EvidenceError::IncompleteExecutionCoverage {
            detail: "conservation snapshots do not cover every tick",
        });
    }
    if stock_codes.len() < 2 {
        return Err(EvidenceError::IncompleteExecutionCoverage {
            detail: "fewer than two stocks were observed",
        });
    }
    if multi_leg_order_ids.is_empty() {
        return Err(EvidenceError::IncompleteExecutionCoverage {
            detail: "no actual multi-leg Fill envelope was observed",
        });
    }
    let auction_finalizations = finalizers.iter().try_fold(0u64, |total, audit| {
        total.checked_add(audit.auction_finalizations).ok_or(
            EvidenceError::IncompleteExecutionCoverage {
                detail: "auction finalizer count overflow",
            },
        )
    })?;
    let day_end_finalizations = finalizers.iter().try_fold(0u64, |total, audit| {
        total.checked_add(audit.day_end_finalizations).ok_or(
            EvidenceError::IncompleteExecutionCoverage {
                detail: "day-end finalizer count overflow",
            },
        )
    })?;
    if auction_finalizations == 0 || day_end_finalizations == 0 {
        return Err(EvidenceError::IncompleteExecutionCoverage {
            detail: "auction/day-end finalizer audit has no real completion",
        });
    }
    let mut slots = BTreeSet::new();
    let restore_slots = restore_inputs
        .iter()
        .map(|input| {
            if input.slot.is_empty() || !slots.insert(input.slot) {
                return Err(EvidenceError::EmptyRestoreBytes {
                    slot: input.slot.to_owned(),
                    field: "slot",
                });
            }
            for (field, bytes) in [
                ("saved", input.saved),
                ("restored", input.restored),
                (
                    "uninterrupted_continuation",
                    input.uninterrupted_continuation,
                ),
                ("restored_continuation", input.restored_continuation),
            ] {
                if bytes.is_empty() {
                    return Err(EvidenceError::EmptyRestoreBytes {
                        slot: input.slot.to_owned(),
                        field,
                    });
                }
            }
            if input.saved != input.restored {
                return Err(EvidenceError::RestoreBytesMismatch {
                    slot: input.slot.to_owned(),
                });
            }
            if input.uninterrupted_continuation != input.restored_continuation {
                return Err(EvidenceError::RestoreContinuationMismatch {
                    slot: input.slot.to_owned(),
                });
            }
            Ok(RestoreSlotProjection {
                slot: input.slot.to_owned(),
                saved: artifact_receipt(input.saved, "restore.saved", sha256)?,
                restored: artifact_receipt(input.restored, "restore.restored", sha256)?,
                uninterrupted_continuation: artifact_receipt(
                    input.uninterrupted_continuation,
                    "restore.uninterrupted_continuation",
                    sha256,
                )?,
                restored_continuation: artifact_receipt(
                    input.restored_continuation,
                    "restore.restored_continuation",
                    sha256,
                )?,
            })
        })
        .collect::<Result<Vec<_>, EvidenceError>>()?;
    Ok(ExecutionCoverage {
        tick_from,
        tick_to,
        auction_finalizations,
        day_end_finalizations,
        stock_codes: stock_codes.into_iter().collect(),
        multi_leg_order_ids: multi_leg_order_ids.into_iter().collect(),
        restore_slots,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TradeRole {
    MakerBuy,
    TakerBuy,
    MakerSell,
    TakerSell,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FeedbackAuditInput {
    pub strategy_generated_intents: u64,
    pub plan_generated_intents: u64,
    pub state_dependent_intents: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SellerCostBasisInput {
    pub invested_cents: i64,
    pub recovered_cents: i64,
}

#[derive(Clone, Copy, Debug)]
pub struct ControlledContinuationBytes<'a> {
    pub sealed_exogenous_script: &'a [u8],
    pub strategy_state: &'a [u8],
    pub plan_state: &'a [u8],
    pub pending_intents: &'a [u8],
    pub restore_order: &'a [u8],
    pub rng_cursor: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlledSellSurface {
    AuctionRollover,
    CrossTickPartialFill,
}

#[derive(Clone, Copy, Debug)]
pub struct SaveLiveOrderIdentity<'a> {
    pub account: AccountId,
    pub stock: &'a StockCode,
    pub order: OrderId,
    pub side: Side,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveRestoreContinuationCommand {
    pub account: AccountId,
    pub intent: crate::Intent,
}

#[derive(Clone, Copy)]
pub struct SaveRestoreLiveOrderInput<'a> {
    pub saved_bytes: &'a [u8],
    pub restored_resave_bytes: &'a [u8],
    pub uninterrupted_authority: &'a [u8],
    pub restored_authority: &'a [u8],
    pub continuation_input: &'a [u8],
    pub uninterrupted_continuation: &'a [u8],
    pub restored_continuation: &'a [u8],
    pub expected: SaveLiveOrderIdentity<'a>,
    pub legacy_sell_reservation: Money,
    pub sha256: &'a dyn Sha256Provider,
}

impl std::fmt::Debug for SaveRestoreLiveOrderInput<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SaveRestoreLiveOrderInput")
            .field("saved_bytes_len", &self.saved_bytes.len())
            .field(
                "restored_resave_bytes_len",
                &self.restored_resave_bytes.len(),
            )
            .field(
                "uninterrupted_authority_len",
                &self.uninterrupted_authority.len(),
            )
            .field("restored_authority_len", &self.restored_authority.len())
            .field("continuation_input_len", &self.continuation_input.len())
            .field(
                "uninterrupted_continuation_len",
                &self.uninterrupted_continuation.len(),
            )
            .field(
                "restored_continuation_len",
                &self.restored_continuation.len(),
            )
            .field("expected", &self.expected)
            .field("legacy_sell_reservation", &self.legacy_sell_reservation)
            .field("sha256", &"<dyn Sha256Provider>")
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct ControlledSellInput<'a> {
    pub envelope: &'a Envelope,
    pub receipts: &'a [EnvelopeReceipt],
    pub trade_role: TradeRole,
    pub submission_cash: Money,
    pub account_after: &'a AccountSnap,
    pub legacy_sell_reservation: Money,
    pub cost_basis: SellerCostBasisInput,
    pub feedback: FeedbackAuditInput,
    pub continuation: ControlledContinuationBytes<'a>,
}

#[derive(Clone, Copy, Debug)]
pub enum CorpusSurfaceInput<'a> {
    NormalMultiLegTerminal {
        envelope: &'a Envelope,
        receipts: &'a [EnvelopeReceipt],
        trade_role: TradeRole,
        submission_cash: Money,
        account_after: &'a AccountSnap,
        legacy_sell_reservation: Money,
        feedback: FeedbackAuditInput,
    },
    AcceptanceFlip {
        account: AccountId,
        stock: &'a StockCode,
        trade_role: TradeRole,
        submission_cash: Money,
        account_after: &'a AccountSnap,
        legacy_sell_reservation: Money,
        feedback: FeedbackAuditInput,
    },
    ThreeLegFeeCatchup {
        envelope: &'a Envelope,
        receipts: &'a [EnvelopeReceipt],
        trade_role: TradeRole,
        account_after: &'a AccountSnap,
        legacy_sell_reservation: Money,
        feedback: FeedbackAuditInput,
    },
    BuyerFees {
        envelope: &'a Envelope,
        receipts: &'a [EnvelopeReceipt],
        trade_role: TradeRole,
    },
    T1 {
        envelope: &'a Envelope,
        receipts: &'a [EnvelopeReceipt],
        trade_role: TradeRole,
        before: &'a PositionSnap,
        after: &'a PositionSnap,
    },
    PriceCage {
        account: AccountId,
        stock: &'a StockCode,
        inside_order: OrderId,
    },
    ContinuousBuyLeg {
        envelope: &'a Envelope,
        receipts: &'a [EnvelopeReceipt],
        trade_role: TradeRole,
    },
    SaveRestoreLiveOrder(SaveRestoreLiveOrderInput<'a>),
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct CorpusProjection {
    pub schema: &'static str,
    pub case_id: String,
    pub scenario: String,
    pub seed: String,
    pub class: &'static str,
    pub updates: Vec<UpdateProjection>,
    pub state: Value,
    pub seller_fee_control: Option<Value>,
    pub corpus_control: Value,
}

pub fn project_corpus_surface(
    case_id: &str,
    scenario: &str,
    seed: u64,
    updates: &[RuntimeUpdateRef<'_>],
    surface: CorpusSurfaceInput<'_>,
) -> Result<CorpusProjection, EvidenceError> {
    require_text(case_id, "case_id")?;
    require_text(scenario, "scenario")?;
    if updates.is_empty() {
        return Err(EvidenceError::CorpusEvidenceMismatch {
            surface: if matches!(surface, CorpusSurfaceInput::SaveRestoreLiveOrder(_)) {
                "save-restore-live-order"
            } else {
                "unknown"
            },
            detail: "no real runtime updates were supplied",
        });
    }
    let projected_updates = project_corpus_updates(updates)?;
    if let CorpusSurfaceInput::SaveRestoreLiveOrder(input) = surface {
        return project_save_restore_live_order(
            case_id,
            scenario,
            seed,
            updates,
            projected_updates,
            input,
        );
    }
    let events = update_events(updates);
    let seller_projection = match surface {
        CorpusSurfaceInput::NormalMultiLegTerminal {
            envelope,
            receipts,
            trade_role,
            submission_cash,
            account_after,
            legacy_sell_reservation,
            feedback,
        } => Some(project_normal_multi_leg_terminal(
            envelope,
            receipts,
            trade_role,
            submission_cash,
            account_after,
            legacy_sell_reservation,
            feedback,
            &events,
        )?),
        CorpusSurfaceInput::AcceptanceFlip {
            account,
            stock,
            trade_role,
            submission_cash,
            account_after,
            legacy_sell_reservation,
            feedback,
        } => Some(project_acceptance_flip(
            account,
            stock,
            trade_role,
            submission_cash,
            account_after,
            legacy_sell_reservation,
            feedback,
            &events,
        )?),
        CorpusSurfaceInput::ThreeLegFeeCatchup {
            envelope,
            receipts,
            trade_role,
            account_after,
            legacy_sell_reservation,
            feedback,
        } => Some(project_three_leg_fee_catchup(
            envelope,
            receipts,
            trade_role,
            account_after,
            legacy_sell_reservation,
            feedback,
            &events,
        )?),
        _ => None,
    };
    if let Some(projection) = seller_projection {
        return Ok(CorpusProjection {
            schema: CORPUS_SCHEMA,
            case_id: case_id.to_owned(),
            scenario: scenario.to_owned(),
            seed: seed.to_string(),
            class: projection.class,
            updates: projected_updates,
            state: projection.state,
            seller_fee_control: None,
            corpus_control: projection.control,
        });
    }
    let (surface_name, evidence) = match surface {
        CorpusSurfaceInput::BuyerFees {
            envelope,
            receipts,
            trade_role,
        } => project_buyer_fee_surface(envelope, receipts, trade_role, &events)?,
        CorpusSurfaceInput::T1 {
            envelope,
            receipts,
            trade_role,
            before,
            after,
        } => project_t1_surface(envelope, receipts, trade_role, before, after, &events)?,
        CorpusSurfaceInput::PriceCage {
            account,
            stock,
            inside_order,
        } => project_price_cage_surface(account, stock, inside_order, &events)?,
        CorpusSurfaceInput::ContinuousBuyLeg {
            envelope,
            receipts,
            trade_role,
        } => project_continuous_buy_surface(envelope, receipts, trade_role, &events)?,
        CorpusSurfaceInput::NormalMultiLegTerminal { .. }
        | CorpusSurfaceInput::AcceptanceFlip { .. }
        | CorpusSurfaceInput::ThreeLegFeeCatchup { .. } => {
            unreachable!("seller surfaces returned above")
        }
        CorpusSurfaceInput::SaveRestoreLiveOrder(_) => unreachable!("handled above"),
    };
    let state_field = match surface_name {
        "buyer-fees" => "buyer_fee_control",
        "t1" => "t1_control",
        "price-cage" => "price_cage_control",
        "continuous-buy-leg" => "continuous_buy_leg_control",
        _ => {
            return Err(EvidenceError::CorpusEvidenceMismatch {
                surface: surface_name,
                detail: "surface has no purpose-built state field",
            });
        }
    };
    let state = Value::Object(serde_json::Map::from_iter([(
        state_field.to_owned(),
        evidence.clone(),
    )]));
    Ok(CorpusProjection {
        schema: CORPUS_SCHEMA,
        case_id: case_id.to_owned(),
        scenario: scenario.to_owned(),
        seed: seed.to_string(),
        class: "equivalence",
        updates: projected_updates,
        state,
        seller_fee_control: None,
        corpus_control: equivalence_control(surface_name, evidence),
    })
}

#[allow(clippy::too_many_arguments)]
fn project_save_restore_live_order(
    case_id: &str,
    scenario: &str,
    seed: u64,
    updates: &[RuntimeUpdateRef<'_>],
    projected_updates: Vec<UpdateProjection>,
    input: SaveRestoreLiveOrderInput<'_>,
) -> Result<CorpusProjection, EvidenceError> {
    const SURFACE: &str = "save-restore-live-order";

    let saved = artifact_receipt(input.saved_bytes, "saved_bytes", input.sha256)?;
    let restored_resave = artifact_receipt(
        input.restored_resave_bytes,
        "restored_resave_bytes",
        input.sha256,
    )?;
    let uninterrupted_authority = artifact_receipt(
        input.uninterrupted_authority,
        "uninterrupted_authority",
        input.sha256,
    )?;
    let restored_authority =
        artifact_receipt(input.restored_authority, "restored_authority", input.sha256)?;
    let continuation_input =
        artifact_receipt(input.continuation_input, "continuation_input", input.sha256)?;
    let uninterrupted_continuation = artifact_receipt(
        input.uninterrupted_continuation,
        "uninterrupted_continuation",
        input.sha256,
    )?;
    let restored_continuation = artifact_receipt(
        input.restored_continuation,
        "restored_continuation",
        input.sha256,
    )?;

    validate_stock(input.expected.stock)?;
    if input.expected.side != Side::Sell {
        return Err(corpus_mismatch(
            SURFACE,
            "expected live order identity must be a Sell order",
        ));
    }
    let slot = crate::decode_save_slot(input.saved_bytes, &crate::SaveDecodeLimits::default())
        .map_err(|error| EvidenceError::InvalidSaveV2 {
            stage: "decode",
            detail: error.to_string(),
        })?;
    if slot.schema_version != crate::SAVE_SCHEMA_VERSION_V2 {
        return Err(EvidenceError::InvalidSaveV2 {
            stage: "schema",
            detail: format!(
                "expected schema {}, got {}",
                crate::SAVE_SCHEMA_VERSION_V2,
                slot.schema_version
            ),
        });
    }
    let continuation_command: SaveRestoreContinuationCommand =
        serde_json::from_slice(input.continuation_input).map_err(|error| {
            EvidenceError::InvalidSaveV2 {
                stage: "continuation-decode",
                detail: error.to_string(),
            }
        })?;
    let restored =
        crate::GameSession::restore(&slot).map_err(|error| EvidenceError::InvalidSaveV2 {
            stage: "restore",
            detail: error.to_string(),
        })?;
    let public_resave = restored
        .save()
        .map_err(|error| EvidenceError::InvalidSaveV2 {
            stage: "resave",
            detail: error.to_string(),
        })?;
    let public_resave_bytes =
        serde_json::to_vec(&public_resave).map_err(|error| EvidenceError::InvalidSaveV2 {
            stage: "resave-serialization",
            detail: error.to_string(),
        })?;
    // This seam proves byte continuity, not merely semantic JSON equivalence. SaveSlot's struct
    // field order and BTreeMap key order make serde_json::to_vec deterministic, so reordered or
    // whitespace-modified input is intentionally rejected here.
    if public_resave_bytes != input.restored_resave_bytes
        || public_resave_bytes != input.saved_bytes
    {
        return Err(corpus_mismatch(
            SURFACE,
            "restored re-save bytes differ from the canonical save",
        ));
    }

    let public_authority =
        restored
            .business_state_hash()
            .map_err(|error| EvidenceError::InvalidSaveV2 {
                stage: "authority",
                detail: error.to_string(),
            })?;
    let public_authority_bytes =
        serde_json::to_vec(&public_authority).map_err(|error| EvidenceError::InvalidSaveV2 {
            stage: "authority-serialization",
            detail: error.to_string(),
        })?;
    if public_authority_bytes != input.restored_authority {
        return Err(corpus_mismatch(
            SURFACE,
            "restored authority bytes differ from the public restored session",
        ));
    }
    if input.uninterrupted_authority != input.restored_authority {
        return Err(corpus_mismatch(
            SURFACE,
            "restored authority bytes differ from uninterrupted authority bytes",
        ));
    }

    let live_order_count = slot.resting_orders.values().map(Vec::len).sum::<usize>()
        + slot.auction_orders.values().map(Vec::len).sum::<usize>();
    if live_order_count == 0 {
        return Err(corpus_mismatch(SURFACE, "save v2 contains no live order"));
    }
    let seller_order_count = slot
        .resting_orders
        .values()
        .flatten()
        .filter(|order| order.side == Side::Sell)
        .count()
        + slot
            .auction_orders
            .values()
            .flatten()
            .filter(|order| order.side == Side::Sell)
            .count();
    let seller_envelope_count = slot
        .runtime_v2
        .live_envelopes
        .iter()
        .filter(|envelope| envelope.key.side == Side::Sell)
        .count();
    if seller_order_count != 1 || seller_envelope_count != 1 {
        return Err(corpus_mismatch(
            SURFACE,
            "controlled save surface requires exactly one live Sell order and envelope",
        ));
    }
    let order = slot
        .resting_orders
        .get(input.expected.stock)
        .and_then(|orders| {
            orders.iter().find(|order| {
                order.owner == input.expected.account
                    && order.id == input.expected.order
                    && order.side == input.expected.side
            })
        })
        .ok_or_else(|| {
            corpus_mismatch(
                SURFACE,
                "expected live order identity is absent from the saved order book",
            )
        })?;
    let envelope = slot
        .runtime_v2
        .live_envelopes
        .iter()
        .find(|envelope| {
            envelope.key.account == input.expected.account
                && envelope.key.stock == *input.expected.stock
                && envelope.key.order == input.expected.order
                && envelope.key.side == input.expected.side
        })
        .ok_or_else(|| {
            corpus_mismatch(
                SURFACE,
                "expected live order identity is absent from the saved envelope ledger",
            )
        })?;
    if order.qty != envelope.audit.remaining_qty
        || envelope.live.shares != order.qty
        || envelope.live.cash != Money::ZERO
    {
        return Err(corpus_mismatch(
            SURFACE,
            "saved order and live envelope resources disagree",
        ));
    }
    if continuation_command.account != input.expected.account {
        return Err(corpus_mismatch(
            SURFACE,
            "continuation account differs from the expected live order account",
        ));
    }
    match &continuation_command.intent {
        crate::Intent::Cancel { code, id }
            if code == input.expected.stock && *id == input.expected.order => {}
        _ => {
            return Err(corpus_mismatch(
                SURFACE,
                "continuation must cancel the expected live order",
            ));
        }
    }

    let public_uninterrupted_continuation =
        replay_save_v2_continuation(&slot, &continuation_command)?;
    let public_restored_continuation = replay_save_v2_continuation(&slot, &continuation_command)?;
    if public_uninterrupted_continuation != public_restored_continuation {
        return Err(EvidenceError::InvalidSaveV2 {
            stage: "continuation-replay",
            detail: "two public restores produced different continuation bytes".to_owned(),
        });
    }
    if public_uninterrupted_continuation != input.uninterrupted_continuation {
        return Err(corpus_mismatch(
            SURFACE,
            "uninterrupted continuation bytes differ from the public replay",
        ));
    }
    if public_restored_continuation != input.restored_continuation {
        return Err(corpus_mismatch(
            SURFACE,
            "restored continuation bytes differ from the public replay",
        ));
    }
    if input.uninterrupted_continuation != input.restored_continuation {
        return Err(corpus_mismatch(
            SURFACE,
            "restored continuation bytes differ from uninterrupted continuation bytes",
        ));
    }

    let account = slot
        .snapshot
        .accounts
        .get(&input.expected.account)
        .ok_or_else(|| corpus_mismatch(SURFACE, "live order account is absent from the save"))?;
    let position = account
        .positions
        .get(input.expected.stock)
        .ok_or_else(|| corpus_mismatch(SURFACE, "live Sell position is absent from the save"))?;
    let events = update_events(updates);
    let accepted = events.iter().any(|event| {
        matches!(
            event,
            Event::OrderAccepted {
                account,
                code,
                id,
                side: Side::Sell,
                ..
            } if *account == input.expected.account
                && code == input.expected.stock
                && *id == input.expected.order
        )
    });
    let canceled = events.iter().any(|event| {
        matches!(
            event,
            Event::OrderCanceled {
                account,
                code,
                id,
                ..
            } if *account == input.expected.account
                && code == input.expected.stock
                && *id == input.expected.order
        )
    });
    if !accepted || !canceled {
        return Err(corpus_mismatch(
            SURFACE,
            "runtime updates do not prove the live Sell acceptance and deterministic continuation",
        ));
    }

    let strategy_state = serde_json::to_vec(&slot.runtime_v2.strategy_states).map_err(|error| {
        EvidenceError::InvalidSaveV2 {
            stage: "strategy-state-serialization",
            detail: error.to_string(),
        }
    })?;
    let plan_state =
        serde_json::to_vec(&slot.plans).map_err(|error| EvidenceError::InvalidSaveV2 {
            stage: "plan-state-serialization",
            detail: error.to_string(),
        })?;
    let pending_intents =
        serde_json::to_vec(&slot.pending_player).map_err(|error| EvidenceError::InvalidSaveV2 {
            stage: "pending-intents-serialization",
            detail: error.to_string(),
        })?;
    let restore_order =
        serde_json::to_vec(order).map_err(|error| EvidenceError::InvalidSaveV2 {
            stage: "restore-order-serialization",
            detail: error.to_string(),
        })?;
    let continuation = project_continuation(
        ControlledContinuationBytes {
            sealed_exogenous_script: input.continuation_input,
            strategy_state: &strategy_state,
            plan_state: &plan_state,
            pending_intents: &pending_intents,
            restore_order: &restore_order,
            rng_cursor: slot.rng_state,
        },
        input.sha256,
    )?;

    let nominal = fee_components_v2_total(envelope.audit.nominal, SURFACE)?;
    let charged = fee_components_v2_total(envelope.audit.charged, SURFACE)?;
    let prefixes = vec![FeePrefixProjection {
        nominal_cents: decimal_money(nominal, SURFACE)?,
        charged_cents: decimal_money(charged, SURFACE)?,
    }];
    let terminal_cash = nonnegative_money(account.cash, "save_restore.terminal_cash")?;
    let invested =
        u64::try_from(position.invested_cents).map_err(|_| EvidenceError::NegativeMoney {
            field: "save_restore.invested_cents",
            cents: position.invested_cents,
        })?;
    let recovered =
        u64::try_from(position.recovered_cents).map_err(|_| EvidenceError::NegativeMoney {
            field: "save_restore.recovered_cents",
            cents: position.recovered_cents,
        })?;
    let state = serde_json::json!({
        "reserved_cash_cents": decimal_money(account.reserved_cash, SURFACE)?,
        "live_shares": envelope.live.shares.to_string(),
        "order_id": envelope.key.order.0.to_string(),
        "save_representation": {
            "schema": "v2",
        },
        "save_restore_live_order_control": {
            "schema_version": slot.schema_version.to_string(),
            "live_order": {
                "account_id": envelope.key.account.0.to_string(),
                "stock_code": envelope.key.stock.0,
                "order_id": envelope.key.order.0.to_string(),
                "side": "Sell",
                "remaining_qty": envelope.audit.remaining_qty.to_string(),
                "live_cash_cents": decimal_money(envelope.live.cash, SURFACE)?,
                "live_shares": envelope.live.shares.to_string(),
            },
            "saved": saved,
            "restored_resave": restored_resave,
            "uninterrupted_authority": uninterrupted_authority,
            "restored_authority": restored_authority,
            "continuation_input": continuation_input,
            "uninterrupted_continuation": uninterrupted_continuation,
            "restored_continuation": restored_continuation,
        },
    });

    Ok(CorpusProjection {
        schema: CORPUS_SCHEMA,
        case_id: case_id.to_owned(),
        scenario: scenario.to_owned(),
        seed: seed.to_string(),
        class: "controlled-live-sell",
        updates: projected_updates,
        state,
        seller_fee_control: Some(serde_json::json!({
            "gross_cents": decimal_money(envelope.audit.filled_value, SURFACE)?,
            "nominal_final_cents": decimal_money(nominal, SURFACE)?,
            "charged_final_cents": decimal_money(charged, SURFACE)?,
            "terminal_cash_cents": terminal_cash.to_string(),
            "invested_cents": invested.to_string(),
            "recovered_cents": recovered.to_string(),
            "fills": [],
        })),
        corpus_control: seller_control(
            SURFACE,
            input.legacy_sell_reservation,
            prefixes,
            FeedbackAuditInput::default(),
            "accepted",
            true,
            None,
            &["pre-save", "post-restore", "post-continuation-tick"],
            Some(continuation),
        )?,
    })
}

fn replay_save_v2_continuation(
    slot: &crate::SaveSlot,
    command: &SaveRestoreContinuationCommand,
) -> Result<Vec<u8>, EvidenceError> {
    let mut session =
        crate::GameSession::restore(slot).map_err(|error| EvidenceError::InvalidSaveV2 {
            stage: "continuation-restore",
            detail: error.to_string(),
        })?;
    session
        .enqueue_player_intent(command.account, command.intent.clone())
        .map_err(|error| EvidenceError::InvalidSaveV2 {
            stage: "continuation-enqueue",
            detail: error.to_string(),
        })?;
    let frame = session
        .step_frame()
        .map_err(|error| EvidenceError::InvalidSaveV2 {
            stage: "continuation-step",
            detail: error.to_string(),
        })?;
    let final_save = session
        .save()
        .map_err(|error| EvidenceError::InvalidSaveV2 {
            stage: "continuation-save",
            detail: error.to_string(),
        })?;
    let final_save_bytes =
        serde_json::to_vec(&final_save).map_err(|error| EvidenceError::InvalidSaveV2 {
            stage: "continuation-save-serialization",
            detail: error.to_string(),
        })?;
    serde_json::to_vec(&(&frame, &final_save_bytes)).map_err(|error| EvidenceError::InvalidSaveV2 {
        stage: "continuation-result-serialization",
        detail: error.to_string(),
    })
}

fn fee_components_v2_total(
    fees: crate::FeeComponentsV2,
    surface: &'static str,
) -> Result<Money, EvidenceError> {
    fees.commission
        .add(fees.stamp_tax)
        .and_then(|subtotal| subtotal.add(fees.transfer_fee))
        .map_err(|_| corpus_mismatch(surface, "saved cumulative fee total overflow"))
}

struct SellerCorpusFields {
    class: &'static str,
    state: Value,
    control: Value,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
struct FeePrefixProjection {
    nominal_cents: String,
    charged_cents: String,
}

fn project_normal_multi_leg_terminal(
    envelope: &Envelope,
    receipts: &[EnvelopeReceipt],
    role: TradeRole,
    submission_cash: Money,
    account_after: &AccountSnap,
    legacy_sell_reservation: Money,
    feedback: FeedbackAuditInput,
    events: &[&Event],
) -> Result<SellerCorpusFields, EvidenceError> {
    require_sell_surface(
        envelope,
        receipts,
        role,
        account_after,
        "normal-multi-leg-terminal",
    )?;
    require_feedback_disabled(feedback, "normal-multi-leg-terminal")?;
    if submission_cash != Money::ZERO {
        return Err(corpus_mismatch(
            "normal-multi-leg-terminal",
            "zero-cash acceptance was not observed at submission",
        ));
    }
    if legacy_sell_reservation != Money::ZERO {
        return Err(corpus_mismatch(
            "normal-multi-leg-terminal",
            "frozen legacy sell reservation is not zero",
        ));
    }
    let fills = matching_fills(envelope, receipts, "normal-multi-leg-terminal")?;
    let prefixes = project_fee_prefixes(envelope, &fills, "normal-multi-leg-terminal")?;
    if fills.len() < 2
        || envelope.live() != ResVec::ZERO
        || prefixes
            .iter()
            .any(|prefix| prefix.nominal_cents != prefix.charged_cents)
    {
        return Err(corpus_mismatch(
            "normal-multi-leg-terminal",
            "surface needs a terminal multi-leg Sell with no fee shortfall",
        ));
    }
    require_related_sell_execution(envelope, role, &fills, events, "normal-multi-leg-terminal")?;
    let charged =
        envelope.audit().charged.total().map_err(|_| {
            corpus_mismatch("normal-multi-leg-terminal", "final charged fee overflow")
        })?;
    let delivered = sum_delivery(&fills, "normal-multi-leg-terminal")?;
    Ok(SellerCorpusFields {
        class: "equivalence",
        state: serde_json::json!({
            "reserved_cash_cents": decimal_money(account_after.reserved_cash, "normal-multi-leg-terminal")?,
            "charged_total_cents": decimal_money(charged, "normal-multi-leg-terminal")?,
            "net_delivery_cents": decimal_money(delivered, "normal-multi-leg-terminal")?,
            "terminal": true,
        }),
        control: seller_control(
            "normal-multi-leg-terminal",
            legacy_sell_reservation,
            prefixes,
            feedback,
            "accepted",
            true,
            None,
            &["post-commit"],
            None,
        )?,
    })
}

#[allow(clippy::too_many_arguments)]
fn project_acceptance_flip(
    account: AccountId,
    stock: &StockCode,
    role: TradeRole,
    submission_cash: Money,
    account_after: &AccountSnap,
    legacy_sell_reservation: Money,
    feedback: FeedbackAuditInput,
    events: &[&Event],
) -> Result<SellerCorpusFields, EvidenceError> {
    const SURFACE: &str = "acceptance-flip";
    validate_stock(stock)?;
    require_feedback_disabled(feedback, SURFACE)?;
    if !matches!(role, TradeRole::MakerSell | TradeRole::TakerSell) {
        return Err(corpus_mismatch(
            SURFACE,
            "surface role does not identify the Sell leg",
        ));
    }
    if legacy_sell_reservation.cents() <= 0 {
        return Err(corpus_mismatch(
            SURFACE,
            "frozen legacy sell reservation must be positive",
        ));
    }
    let reserved = decimal_money(account_after.reserved_cash, SURFACE)?;
    if reserved != "0" {
        return Err(corpus_mismatch(
            SURFACE,
            "current Sell path retained a cash reservation",
        ));
    }
    let accepted = events.iter().any(|event| match event {
        Event::OrderAccepted {
            account: event_account,
            code,
            side: Side::Sell,
            ..
        } => *event_account == account && code == stock,
        Event::Trade {
            code, maker, taker, ..
        } if code == stock => match role {
            TradeRole::MakerSell => *maker == account,
            TradeRole::TakerSell => *taker == account,
            TradeRole::MakerBuy | TradeRole::TakerBuy => false,
        },
        _ => false,
    });
    let rejected = events.iter().any(|event| {
        matches!(
            event,
            Event::IntentRejected {
                account: event_account,
                code,
                reason: crate::RejectionReason::InsufficientCash,
                ..
            } if *event_account == account && code == stock
        )
    });
    if !accepted || rejected {
        return Err(corpus_mismatch(
            SURFACE,
            "current runtime facts do not prove the Sell acceptance flip",
        ));
    }
    let zero_cash = submission_cash == Money::ZERO;
    Ok(SellerCorpusFields {
        class: "divergence-9",
        state: serde_json::json!({
            "reserved_cash": reserved,
            "acceptance": "accepted",
        }),
        control: seller_control(
            SURFACE,
            legacy_sell_reservation,
            vec![],
            feedback,
            "accepted",
            zero_cash,
            Some(serde_json::json!({
                "account_id": account.0.to_string(),
                "stock_code": stock.0,
                "side": "Sell",
                "trade_role": role_name(role),
            })),
            &["post-commit"],
            None,
        )?,
    })
}

fn project_three_leg_fee_catchup(
    envelope: &Envelope,
    receipts: &[EnvelopeReceipt],
    role: TradeRole,
    account_after: &AccountSnap,
    legacy_sell_reservation: Money,
    feedback: FeedbackAuditInput,
    events: &[&Event],
) -> Result<SellerCorpusFields, EvidenceError> {
    const SURFACE: &str = "three-leg-fee-catchup";
    require_sell_surface(envelope, receipts, role, account_after, SURFACE)?;
    require_feedback_disabled(feedback, SURFACE)?;
    let fills = matching_fills(envelope, receipts, SURFACE)?;
    let prefixes = project_fee_prefixes(envelope, &fills, SURFACE)?;
    if fills.len() < 3
        || !prefixes
            .iter()
            .any(|prefix| prefix.nominal_cents != prefix.charged_cents)
    {
        return Err(corpus_mismatch(
            SURFACE,
            "surface needs three Fill legs and an actual charged-fee shortfall",
        ));
    }
    require_related_sell_execution(envelope, role, &fills, events, SURFACE)?;
    let charged = envelope
        .audit()
        .charged
        .total()
        .map_err(|_| corpus_mismatch(SURFACE, "final charged fee overflow"))?;
    let delivered = sum_delivery(&fills, SURFACE)?;
    Ok(SellerCorpusFields {
        class: "divergence-9",
        state: serde_json::json!({
            "reserved_cash_cents": decimal_money(account_after.reserved_cash, SURFACE)?,
            "charged_total_cents": decimal_money(charged, SURFACE)?,
            "net_delivery_cents": decimal_money(delivered, SURFACE)?,
        }),
        control: seller_control(
            SURFACE,
            legacy_sell_reservation,
            prefixes,
            feedback,
            "accepted",
            false,
            None,
            &["post-commit"],
            None,
        )?,
    })
}

fn require_sell_surface(
    envelope: &Envelope,
    receipts: &[EnvelopeReceipt],
    role: TradeRole,
    account_after: &AccountSnap,
    surface: &'static str,
) -> Result<(), EvidenceError> {
    validate_stock(&envelope.key().stock)?;
    if envelope.key().side != Side::Sell
        || !matches!(role, TradeRole::MakerSell | TradeRole::TakerSell)
    {
        return Err(corpus_mismatch(
            surface,
            "surface must be bound to an actual Sell envelope and Sell trade role",
        ));
    }
    if receipts.is_empty() {
        return Err(corpus_mismatch(surface, "surface has no envelope receipts"));
    }
    for value in [
        envelope.basis().cash,
        envelope.spent().cash,
        envelope.released().cash,
        envelope.live().cash,
    ] {
        if value != Money::ZERO {
            return Err(corpus_mismatch(
                surface,
                "Sell envelope contains a forbidden cash escrow component",
            ));
        }
    }
    if account_after.reserved_cash != Money::ZERO {
        return Err(corpus_mismatch(
            surface,
            "current Sell path retained reserved cash",
        ));
    }
    nonnegative_money(account_after.cash, "seller.account_after.cash")?;
    Ok(())
}

fn require_feedback_disabled(
    feedback: FeedbackAuditInput,
    surface: &'static str,
) -> Result<(), EvidenceError> {
    if feedback != FeedbackAuditInput::default() {
        return Err(corpus_mismatch(
            surface,
            "feedback audit is non-zero for a controlled corpus surface",
        ));
    }
    Ok(())
}

fn project_fee_prefixes(
    envelope: &Envelope,
    fills: &[&EnvelopeReceipt],
    surface: &'static str,
) -> Result<Vec<FeePrefixProjection>, EvidenceError> {
    let nominal_deltas = fills.iter().try_fold(
        crate::session::pipeline::FeeComponents::ZERO,
        |total, receipt| fee_components_add(total, receipt.nominal, surface),
    )?;
    let mut nominal = fee_components_sub(envelope.audit().nominal, nominal_deltas, surface)?;
    let mut charged = fills
        .first()
        .ok_or_else(|| corpus_mismatch(surface, "no Fill receipt was supplied"))?
        .charged_before;
    let mut prefixes = Vec::with_capacity(fills.len());
    for receipt in fills {
        if receipt.charged_before != charged {
            return Err(corpus_mismatch(
                surface,
                "Fill charged-before chain is discontinuous",
            ));
        }
        nominal = fee_components_add(nominal, receipt.nominal, surface)?;
        charged = fee_components_add(charged, receipt.charged, surface)?;
        if receipt.charged_after != charged {
            return Err(corpus_mismatch(
                surface,
                "Fill charged-after chain disagrees with charged delta",
            ));
        }
        let nominal_total = nominal
            .total()
            .map_err(|_| corpus_mismatch(surface, "nominal fee prefix overflow"))?;
        let charged_total = charged
            .total()
            .map_err(|_| corpus_mismatch(surface, "charged fee prefix overflow"))?;
        if charged_total > nominal_total {
            return Err(corpus_mismatch(
                surface,
                "charged fee prefix exceeds nominal fee prefix",
            ));
        }
        prefixes.push(FeePrefixProjection {
            nominal_cents: decimal_money(nominal_total, surface)?,
            charged_cents: decimal_money(charged_total, surface)?,
        });
    }
    if nominal != envelope.audit().nominal || charged != envelope.audit().charged {
        return Err(corpus_mismatch(
            surface,
            "Fill fee prefixes do not reach the envelope final audit",
        ));
    }
    Ok(prefixes)
}

fn fee_components_add(
    left: crate::session::pipeline::FeeComponents,
    right: crate::session::pipeline::FeeComponents,
    surface: &'static str,
) -> Result<crate::session::pipeline::FeeComponents, EvidenceError> {
    Ok(crate::session::pipeline::FeeComponents {
        commission: left
            .commission
            .add(right.commission)
            .map_err(|_| corpus_mismatch(surface, "commission fee overflow"))?,
        stamp_tax: left
            .stamp_tax
            .add(right.stamp_tax)
            .map_err(|_| corpus_mismatch(surface, "stamp-tax fee overflow"))?,
        transfer_fee: left
            .transfer_fee
            .add(right.transfer_fee)
            .map_err(|_| corpus_mismatch(surface, "transfer-fee overflow"))?,
    })
}

fn fee_components_sub(
    left: crate::session::pipeline::FeeComponents,
    right: crate::session::pipeline::FeeComponents,
    surface: &'static str,
) -> Result<crate::session::pipeline::FeeComponents, EvidenceError> {
    let component = |left: Money, right: Money| {
        left.sub(right)
            .map_err(|_| corpus_mismatch(surface, "fee prefix subtraction overflow"))
            .and_then(|value| {
                if value.cents() < 0 {
                    Err(corpus_mismatch(
                        surface,
                        "receipt nominal deltas exceed the envelope final audit",
                    ))
                } else {
                    Ok(value)
                }
            })
    };
    Ok(crate::session::pipeline::FeeComponents {
        commission: component(left.commission, right.commission)?,
        stamp_tax: component(left.stamp_tax, right.stamp_tax)?,
        transfer_fee: component(left.transfer_fee, right.transfer_fee)?,
    })
}

fn require_related_sell_execution(
    envelope: &Envelope,
    role: TradeRole,
    fills: &[&EnvelopeReceipt],
    events: &[&Event],
    surface: &'static str,
) -> Result<(), EvidenceError> {
    let trades = related_trades(envelope, role, events);
    if trades.len() != fills.len() {
        return Err(corpus_mismatch(
            surface,
            "related Trade leg count disagrees with Sell Fill receipts",
        ));
    }
    require_related_trade_gross(envelope, role, events, fill_gross(fills, surface)?, surface)
}

fn sum_delivery(fills: &[&EnvelopeReceipt], surface: &'static str) -> Result<Money, EvidenceError> {
    fills.iter().try_fold(Money::ZERO, |total, receipt| {
        total
            .add(receipt.deliver_cash)
            .map_err(|_| corpus_mismatch(surface, "net delivery overflow"))
    })
}

#[allow(clippy::too_many_arguments)]
fn seller_control(
    surface: &'static str,
    legacy_sell_reservation: Money,
    prefixes: Vec<FeePrefixProjection>,
    feedback: FeedbackAuditInput,
    acceptance: &'static str,
    zero_cash_acceptance: bool,
    surface_evidence: Option<Value>,
    comparison_points: &[&str],
    continuation: Option<Value>,
) -> Result<Value, EvidenceError> {
    let continuation = continuation.unwrap_or_else(|| {
        serde_json::json!({
            "sealed_exogenous_script_sha256": null,
            "rng_cursor": null,
            "strategy_state_sha256": null,
            "plan_state_sha256": null,
            "pending_intents_sha256": null,
            "restore_order_sha256": null,
        })
    });
    Ok(serde_json::json!({
        "surface": surface,
        "seller_order_count": "1",
        "old_sell_reservation_cents": decimal_money(legacy_sell_reservation, surface)?,
        "fee_prefixes": prefixes,
        "feedback": {
            "strategy_generated_intents": feedback.strategy_generated_intents.to_string(),
            "plan_generated_intents": feedback.plan_generated_intents.to_string(),
            "state_dependent_intents": feedback.state_dependent_intents.to_string(),
        },
        "acceptance": acceptance,
        "zero_cash_acceptance": zero_cash_acceptance,
        "sealed_exogenous_script_sha256": continuation["sealed_exogenous_script_sha256"].clone(),
        "rng_cursor": continuation["rng_cursor"].clone(),
        "strategy_state_sha256": continuation["strategy_state_sha256"].clone(),
        "plan_state_sha256": continuation["plan_state_sha256"].clone(),
        "pending_intents_sha256": continuation["pending_intents_sha256"].clone(),
        "restore_order_sha256": continuation["restore_order_sha256"].clone(),
        "comparison_points": comparison_points,
        "surface_evidence": surface_evidence,
    }))
}

fn corpus_mismatch(surface: &'static str, detail: &'static str) -> EvidenceError {
    EvidenceError::CorpusEvidenceMismatch { surface, detail }
}

pub fn project_controlled_sell_corpus(
    case_id: &str,
    scenario: &str,
    seed: u64,
    updates: &[RuntimeUpdateRef<'_>],
    surface: ControlledSellSurface,
    input: ControlledSellInput<'_>,
    sha256: &impl Sha256Provider,
) -> Result<CorpusProjection, EvidenceError> {
    let surface_name = match surface {
        ControlledSellSurface::AuctionRollover => "auction-rollover",
        ControlledSellSurface::CrossTickPartialFill => "cross-tick-partial-fill",
    };
    require_text(case_id, "case_id")?;
    require_text(scenario, "scenario")?;
    if updates.is_empty() {
        return Err(corpus_mismatch(
            surface_name,
            "no real runtime updates were supplied",
        ));
    }
    let projected_updates = project_corpus_updates(updates)?;
    require_sell_surface(
        input.envelope,
        input.receipts,
        input.trade_role,
        input.account_after,
        surface_name,
    )?;
    require_feedback_disabled(input.feedback, surface_name)?;
    let events = update_events(updates);
    let fills = matching_fills(input.envelope, input.receipts, surface_name)?;
    let prefixes = project_fee_prefixes(input.envelope, &fills, surface_name)?;
    require_related_sell_execution(
        input.envelope,
        input.trade_role,
        &fills,
        &events,
        surface_name,
    )?;
    match surface {
        ControlledSellSurface::AuctionRollover => {
            if !input
                .receipts
                .iter()
                .any(|receipt| receipt.kind == ReceiptKind::Rollover)
                || input.envelope.live() == ResVec::ZERO
                || projected_updates.len() < 2
            {
                return Err(corpus_mismatch(
                    surface_name,
                    "surface lacks a live auction Rollover or continuation update",
                ));
            }
        }
        ControlledSellSurface::CrossTickPartialFill => {
            let trade_ticks = updates
                .iter()
                .filter(|update| {
                    let events = match update {
                        RuntimeUpdateRef::Tick(frame) => frame.events.as_slice(),
                        RuntimeUpdateRef::Civil(update) => update.events.as_slice(),
                    };
                    !related_trades(
                        input.envelope,
                        input.trade_role,
                        &events.iter().collect::<Vec<_>>(),
                    )
                    .is_empty()
                })
                .map(|update| match update {
                    RuntimeUpdateRef::Tick(frame) => frame.tick,
                    RuntimeUpdateRef::Civil(update) => update.tick,
                })
                .collect::<BTreeSet<_>>();
            if fills.len() < 2 || trade_ticks.len() < 2 {
                return Err(corpus_mismatch(
                    surface_name,
                    "surface lacks related partial Fill facts in two distinct ticks",
                ));
            }
        }
    }
    let continuation = project_continuation(input.continuation, sha256)?;
    let fill_rows = project_seller_fills(input.envelope, &fills, surface_name)?;
    let gross = fill_gross(&fills, surface_name)?;
    let nominal = input
        .envelope
        .audit()
        .nominal
        .total()
        .map_err(|_| corpus_mismatch(surface_name, "final nominal fee overflow"))?;
    let charged = input
        .envelope
        .audit()
        .charged
        .total()
        .map_err(|_| corpus_mismatch(surface_name, "final charged fee overflow"))?;
    let terminal_cash = nonnegative_money(input.account_after.cash, "seller.terminal_cash")?;
    let invested = u64::try_from(input.cost_basis.invested_cents).map_err(|_| {
        EvidenceError::NegativeMoney {
            field: "seller.invested_cents",
            cents: input.cost_basis.invested_cents,
        }
    })?;
    let recovered = u64::try_from(input.cost_basis.recovered_cents).map_err(|_| {
        EvidenceError::NegativeMoney {
            field: "seller.recovered_cents",
            cents: input.cost_basis.recovered_cents,
        }
    })?;
    let comparison_points: &[&str] = match surface {
        ControlledSellSurface::AuctionRollover => {
            &["post-auction-rollover", "post-continuation-tick"]
        }
        ControlledSellSurface::CrossTickPartialFill => {
            &["post-partial-fill", "post-continuation-tick"]
        }
    };
    Ok(CorpusProjection {
        schema: CORPUS_SCHEMA,
        case_id: case_id.to_owned(),
        scenario: scenario.to_owned(),
        seed: seed.to_string(),
        class: "controlled-live-sell",
        updates: projected_updates,
        state: serde_json::json!({
            "reserved_cash_cents": decimal_money(input.account_after.reserved_cash, surface_name)?,
            "live_shares": input.envelope.live().shares.to_string(),
            "order_id": input.envelope.key().order.0.to_string(),
        }),
        seller_fee_control: Some(serde_json::json!({
            "gross_cents": decimal_money(gross, surface_name)?,
            "nominal_final_cents": decimal_money(nominal, surface_name)?,
            "charged_final_cents": decimal_money(charged, surface_name)?,
            "terminal_cash_cents": terminal_cash.to_string(),
            "invested_cents": invested.to_string(),
            "recovered_cents": recovered.to_string(),
            "fills": fill_rows,
        })),
        corpus_control: seller_control(
            surface_name,
            input.legacy_sell_reservation,
            prefixes,
            input.feedback,
            "accepted",
            input.submission_cash == Money::ZERO,
            None,
            comparison_points,
            Some(continuation),
        )?,
    })
}

fn project_continuation(
    input: ControlledContinuationBytes<'_>,
    sha256: &(impl Sha256Provider + ?Sized),
) -> Result<Value, EvidenceError> {
    Ok(serde_json::json!({
        "sealed_exogenous_script_sha256": artifact_receipt(
            input.sealed_exogenous_script,
            "sealed_exogenous_script",
            sha256,
        )?.sha256,
        "rng_cursor": input.rng_cursor.to_string(),
        "strategy_state_sha256": artifact_receipt(
            input.strategy_state,
            "strategy_state",
            sha256,
        )?.sha256,
        "plan_state_sha256": artifact_receipt(
            input.plan_state,
            "plan_state",
            sha256,
        )?.sha256,
        "pending_intents_sha256": artifact_receipt(
            input.pending_intents,
            "pending_intents",
            sha256,
        )?.sha256,
        "restore_order_sha256": artifact_receipt(
            input.restore_order,
            "restore_order",
            sha256,
        )?.sha256,
    }))
}

fn project_seller_fills(
    envelope: &Envelope,
    fills: &[&EnvelopeReceipt],
    surface: &'static str,
) -> Result<Vec<Value>, EvidenceError> {
    fills
        .iter()
        .map(|receipt| {
            let qty = receipt
                .qty_before
                .checked_sub(receipt.qty_after)
                .ok_or_else(|| corpus_mismatch(surface, "Fill quantity chain runs backwards"))?;
            if qty == 0 {
                return Err(corpus_mismatch(surface, "Fill quantity is zero"));
            }
            let gross = receipt
                .value_after
                .sub(receipt.value_before)
                .map_err(|_| corpus_mismatch(surface, "Fill gross overflow"))?;
            let cents = nonnegative_money(gross, "seller.fill.gross")?;
            let price = cents
                .checked_div(u64::from(qty))
                .ok_or_else(|| corpus_mismatch(surface, "Fill quantity is zero"))?;
            if price.checked_mul(u64::from(qty)) != Some(cents) {
                return Err(corpus_mismatch(
                    surface,
                    "Fill gross is not an integral price-times-quantity value",
                ));
            }
            Ok(serde_json::json!({
                "order_id": envelope.key().order.0.to_string(),
                "qty": qty.to_string(),
                "price_cents": price.to_string(),
            }))
        })
        .collect()
}

fn update_events<'a>(updates: &'a [RuntimeUpdateRef<'a>]) -> Vec<&'a Event> {
    updates
        .iter()
        .flat_map(|update| match update {
            RuntimeUpdateRef::Tick(frame) => frame.events.iter(),
            RuntimeUpdateRef::Civil(update) => update.events.iter(),
        })
        .collect()
}

fn project_corpus_updates(
    updates: &[RuntimeUpdateRef<'_>],
) -> Result<Vec<UpdateProjection>, EvidenceError> {
    let mut next_ordinals = BTreeMap::new();
    let mut comparison_keys = BTreeSet::new();
    let projected = updates
        .iter()
        .copied()
        .map(|update| {
            project_update_in_scope(
                update,
                &mut next_ordinals,
                &mut comparison_keys,
                "local_event_index is not zero-based and contiguous in its corpus ADR domain",
                "comparison_event_key is duplicated across corpus updates",
            )
        })
        .collect::<Result<Vec<_>, EvidenceError>>()?;
    validate_update_stream(&projected)?;
    Ok(projected)
}

fn validate_update_stream(updates: &[UpdateProjection]) -> Result<(), EvidenceError> {
    for pair in updates.windows(2) {
        let previous = &pair[0];
        let current = &pair[1];
        let expected_seq =
            previous
                .seq_to
                .checked_add(1)
                .ok_or(EvidenceError::IntegerOverflow {
                    field: "corpus.update.seq_from",
                    value: previous.seq_to,
                })?;
        let expected_tick = if current.kind == "CivilUpdate" {
            previous.tick
        } else {
            previous
                .tick
                .checked_add(1)
                .ok_or(EvidenceError::IntegerOverflow {
                    field: "corpus.update.tick",
                    value: previous.tick,
                })?
        };
        if current.seq_from != expected_seq || current.tick != expected_tick {
            return Err(EvidenceError::InvalidUpdate {
                detail: "corpus update stream has a sequence or tick gap".to_owned(),
            });
        }
    }
    Ok(())
}

fn project_buyer_fee_surface(
    envelope: &Envelope,
    receipts: &[EnvelopeReceipt],
    role: TradeRole,
    events: &[&Event],
) -> Result<(&'static str, Value), EvidenceError> {
    require_buy_surface(envelope, receipts, role, "buyer-fees")?;
    let fills = matching_fills(envelope, receipts, "buyer-fees")?;
    let gross = fill_gross(&fills, "buyer-fees")?;
    let commission = sum_fee(&fills, |receipt| receipt.charged.commission, "buyer-fees")?;
    let transfer = sum_fee(&fills, |receipt| receipt.charged.transfer_fee, "buyer-fees")?;
    let spent = fills.iter().try_fold(Money::ZERO, |total, receipt| {
        total
            .add(receipt.delta.spent.cash)
            .map_err(|_| EvidenceError::CorpusEvidenceMismatch {
                surface: "buyer-fees",
                detail: "spent cash overflow",
            })
    })?;
    let expected = gross
        .add(commission)
        .and_then(|value| value.add(transfer))
        .map_err(|_| EvidenceError::CorpusEvidenceMismatch {
            surface: "buyer-fees",
            detail: "buyer fee equation overflow",
        })?;
    if spent != expected {
        return Err(EvidenceError::CorpusEvidenceMismatch {
            surface: "buyer-fees",
            detail: "spent cash does not equal gross plus charged buyer fees",
        });
    }
    require_related_trade_gross(envelope, role, events, gross, "buyer-fees")?;
    Ok((
        "buyer-fees",
        serde_json::json!({
            "account_id": envelope.key().account.0.to_string(),
            "stock_code": envelope.key().stock.0,
            "side": "Buy",
            "trade_role": role_name(role),
            "gross_cents": decimal_money(gross, "buyer-fees")?,
            "commission_cents": decimal_money(commission, "buyer-fees")?,
            "transfer_fee_cents": decimal_money(transfer, "buyer-fees")?,
            "spent_cash_cents": decimal_money(spent, "buyer-fees")?,
        }),
    ))
}

fn project_t1_surface(
    envelope: &Envelope,
    receipts: &[EnvelopeReceipt],
    role: TradeRole,
    before: &PositionSnap,
    after: &PositionSnap,
    events: &[&Event],
) -> Result<(&'static str, Value), EvidenceError> {
    require_buy_surface(envelope, receipts, role, "t1")?;
    let fills = matching_fills(envelope, receipts, "t1")?;
    let bought = fills.iter().try_fold(0u32, |total, receipt| {
        total
            .checked_add(receipt.deliver_qty)
            .ok_or(EvidenceError::CorpusEvidenceMismatch {
                surface: "t1",
                detail: "bought quantity overflow",
            })
    })?;
    if bought == 0
        || before.qty.checked_add(bought) != Some(after.qty)
        || before.t1_locked.checked_add(bought) != Some(after.t1_locked)
        || after.t1_locked > after.qty
    {
        return Err(EvidenceError::CorpusEvidenceMismatch {
            surface: "t1",
            detail: "position snapshots do not prove bought shares became T+1 locked",
        });
    }
    let event_qty = related_trade_qty(envelope, role, events, "t1")?;
    if event_qty != bought {
        return Err(EvidenceError::CorpusEvidenceMismatch {
            surface: "t1",
            detail: "related Trade quantities disagree with buy receipts",
        });
    }
    Ok((
        "t1",
        serde_json::json!({
            "account_id": envelope.key().account.0.to_string(),
            "stock_code": envelope.key().stock.0,
            "side": "Buy",
            "trade_role": role_name(role),
            "qty_before": before.qty.to_string(),
            "bought_qty": bought.to_string(),
            "qty_after": after.qty.to_string(),
            "t1_locked_before": before.t1_locked.to_string(),
            "t1_locked_after": after.t1_locked.to_string(),
        }),
    ))
}

fn project_price_cage_surface(
    account: AccountId,
    stock: &StockCode,
    inside_order: OrderId,
    events: &[&Event],
) -> Result<(&'static str, Value), EvidenceError> {
    validate_stock(stock)?;
    let rejected = events
        .iter()
        .filter(|event| {
            matches!(
                event,
                Event::IntentRejected {
                    account: event_account,
                    code,
                    reason: crate::RejectionReason::PriceCageExceeded,
                    ..
                } if *event_account == account && code == stock
            )
        })
        .count();
    let accepted = events
        .iter()
        .filter(|event| {
            matches!(
                event,
                Event::OrderAccepted {
                    account: event_account,
                    code,
                    id,
                    side: Side::Buy,
                    ..
                } if *event_account == account && code == stock && *id == inside_order
            )
        })
        .count();
    if rejected != 1 || accepted != 1 {
        return Err(EvidenceError::CorpusEvidenceMismatch {
            surface: "price-cage",
            detail: "runtime facts do not contain exactly one outside rejection and inside Buy acceptance",
        });
    }
    Ok((
        "price-cage",
        serde_json::json!({
            "account_id": account.0.to_string(),
            "stock_code": stock.0,
            "side": "Buy",
            "outside_rejection": "PriceCageExceeded",
            "inside_acceptance": "accepted",
            "inside_order_id": inside_order.0.to_string(),
        }),
    ))
}

fn project_continuous_buy_surface(
    envelope: &Envelope,
    receipts: &[EnvelopeReceipt],
    role: TradeRole,
    events: &[&Event],
) -> Result<(&'static str, Value), EvidenceError> {
    require_buy_surface(envelope, receipts, role, "continuous-buy-leg")?;
    let fills = matching_fills(envelope, receipts, "continuous-buy-leg")?;
    let accepted = events
        .iter()
        .filter(|event| {
            matches!(
                event,
                Event::OrderAccepted {
                    account,
                    code,
                    id,
                    side: Side::Buy,
                    ..
                } if *account == envelope.key().account
                    && code == &envelope.key().stock
                    && *id == envelope.key().order
            )
        })
        .count();
    let trades = related_trades(envelope, role, events);
    if accepted == 0 || trades.len() != fills.len() {
        return Err(EvidenceError::CorpusEvidenceMismatch {
            surface: "continuous-buy-leg",
            detail: "OrderAccepted identity or related Trade leg count disagrees with receipts",
        });
    }
    Ok((
        "continuous-buy-leg",
        serde_json::json!({
            "account_id": envelope.key().account.0.to_string(),
            "stock_code": envelope.key().stock.0,
            "side": "Buy",
            "trade_role": role_name(role),
            "order_id": envelope.key().order.0.to_string(),
            "trade_leg_count": fills.len().to_string(),
            "stable_order_identity_count": "1",
        }),
    ))
}

fn require_buy_surface(
    envelope: &Envelope,
    receipts: &[EnvelopeReceipt],
    role: TradeRole,
    surface: &'static str,
) -> Result<(), EvidenceError> {
    validate_stock(&envelope.key().stock)?;
    if envelope.key().side != Side::Buy
        || !matches!(role, TradeRole::MakerBuy | TradeRole::TakerBuy)
    {
        return Err(EvidenceError::CorpusEvidenceMismatch {
            surface,
            detail: "surface must be bound to an actual Buy envelope and Buy trade role",
        });
    }
    if receipts.is_empty() {
        return Err(EvidenceError::CorpusEvidenceMismatch {
            surface,
            detail: "surface has no envelope receipts",
        });
    }
    Ok(())
}

fn matching_fills<'a>(
    envelope: &Envelope,
    receipts: &'a [EnvelopeReceipt],
    surface: &'static str,
) -> Result<Vec<&'a EnvelopeReceipt>, EvidenceError> {
    if receipts
        .iter()
        .any(|receipt| &receipt.envelope != envelope.key())
    {
        return Err(EvidenceError::CorpusEvidenceMismatch {
            surface,
            detail: "receipt belongs to another envelope",
        });
    }
    let fills: Vec<_> = receipts
        .iter()
        .filter(|receipt| receipt.kind == ReceiptKind::Fill)
        .collect();
    if fills.is_empty() {
        return Err(EvidenceError::CorpusEvidenceMismatch {
            surface,
            detail: "no real Fill receipt was supplied",
        });
    }
    Ok(fills)
}

fn fill_gross(fills: &[&EnvelopeReceipt], surface: &'static str) -> Result<Money, EvidenceError> {
    fills.iter().try_fold(Money::ZERO, |total, receipt| {
        let delta = receipt.value_after.sub(receipt.value_before).map_err(|_| {
            EvidenceError::CorpusEvidenceMismatch {
                surface,
                detail: "receipt value chain is negative or overflowed",
            }
        })?;
        if delta.cents() <= 0 {
            return Err(EvidenceError::CorpusEvidenceMismatch {
                surface,
                detail: "Fill receipt has no positive gross delta",
            });
        }
        total
            .add(delta)
            .map_err(|_| EvidenceError::CorpusEvidenceMismatch {
                surface,
                detail: "gross value overflow",
            })
    })
}

fn sum_fee(
    fills: &[&EnvelopeReceipt],
    select: impl Fn(&EnvelopeReceipt) -> Money,
    surface: &'static str,
) -> Result<Money, EvidenceError> {
    fills.iter().try_fold(Money::ZERO, |total, receipt| {
        total
            .add(select(receipt))
            .map_err(|_| EvidenceError::CorpusEvidenceMismatch {
                surface,
                detail: "fee value overflow",
            })
    })
}

fn related_trades<'a>(
    envelope: &Envelope,
    role: TradeRole,
    events: &'a [&Event],
) -> Vec<&'a Event> {
    events
        .iter()
        .copied()
        .filter(|event| match event {
            Event::Trade {
                code, maker, taker, ..
            } if code == &envelope.key().stock => match role {
                TradeRole::MakerBuy | TradeRole::MakerSell => *maker == envelope.key().account,
                TradeRole::TakerBuy | TradeRole::TakerSell => *taker == envelope.key().account,
            },
            _ => false,
        })
        .collect()
}

fn require_related_trade_gross(
    envelope: &Envelope,
    role: TradeRole,
    events: &[&Event],
    expected: Money,
    surface: &'static str,
) -> Result<(), EvidenceError> {
    let trades = related_trades(envelope, role, events);
    if trades.is_empty() {
        return Err(EvidenceError::CorpusEvidenceMismatch {
            surface,
            detail: "no related Trade fact was observed",
        });
    }
    let actual = trades.iter().try_fold(Money::ZERO, |total, event| {
        let Event::Trade { price, qty, .. } = event else {
            unreachable!("related_trades only returns Trade")
        };
        let gross = price
            .mul_shares(*qty)
            .map_err(|_| EvidenceError::CorpusEvidenceMismatch {
                surface,
                detail: "Trade gross overflow",
            })?;
        total
            .add(gross)
            .map_err(|_| EvidenceError::CorpusEvidenceMismatch {
                surface,
                detail: "Trade gross total overflow",
            })
    })?;
    if actual != expected {
        return Err(EvidenceError::CorpusEvidenceMismatch {
            surface,
            detail: "Trade gross disagrees with Fill receipts",
        });
    }
    Ok(())
}

fn related_trade_qty(
    envelope: &Envelope,
    role: TradeRole,
    events: &[&Event],
    surface: &'static str,
) -> Result<u32, EvidenceError> {
    let trades = related_trades(envelope, role, events);
    if trades.is_empty() {
        return Err(EvidenceError::CorpusEvidenceMismatch {
            surface,
            detail: "no related Trade fact was observed",
        });
    }
    trades.iter().try_fold(0u32, |total, event| {
        let Event::Trade { qty, .. } = event else {
            unreachable!("related_trades only returns Trade")
        };
        total
            .checked_add(*qty)
            .ok_or(EvidenceError::CorpusEvidenceMismatch {
                surface,
                detail: "Trade quantity overflow",
            })
    })
}

fn decimal_money(value: Money, surface: &'static str) -> Result<String, EvidenceError> {
    if value.cents() < 0 {
        Err(EvidenceError::CorpusEvidenceMismatch {
            surface,
            detail: "negative money cannot cross the evidence boundary",
        })
    } else {
        Ok(value.cents().to_string())
    }
}

fn role_name(role: TradeRole) -> &'static str {
    match role {
        TradeRole::MakerBuy => "maker-buy",
        TradeRole::TakerBuy => "taker-buy",
        TradeRole::MakerSell => "maker-sell",
        TradeRole::TakerSell => "taker-sell",
    }
}

fn equivalence_control(surface: &'static str, evidence: Value) -> Value {
    serde_json::json!({
        "surface": surface,
        "seller_order_count": "0",
        "old_sell_reservation_cents": "0",
        "fee_prefixes": [],
        "feedback": {
            "strategy_generated_intents": "0",
            "plan_generated_intents": "0",
            "state_dependent_intents": "0",
        },
        "acceptance": "accepted",
        "zero_cash_acceptance": false,
        "sealed_exogenous_script_sha256": null,
        "rng_cursor": null,
        "strategy_state_sha256": null,
        "plan_state_sha256": null,
        "pending_intents_sha256": null,
        "restore_order_sha256": null,
        "comparison_points": ["post-commit"],
        "surface_evidence": evidence,
    })
}

#[cfg(test)]
#[path = "verification_evidence/tests.rs"]
mod tests;
