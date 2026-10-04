//! Purpose-built Task 9 evidence projections for the escrow verifier.
//!
//! This module deliberately projects the runtime contracts field by field.  It
//! never exposes `Envelope`, `EnvelopeReceipt`, protocol frames, or account
//! snapshots through their ordinary serde representations.

#[cfg(test)]
use crate::OrderId;
#[cfg(test)]
use crate::PositionSnap;
use crate::{
    session::{
        pipeline::{
            EntityTag, Envelope, EnvelopeOrigin, EnvelopeReceipt, EventSourceIndex, JournalRank,
            ReceiptKind, ReceiptLocalKey, ReceiptSource, ResVec,
        },
        protocol::{CivilUpdate, EventFact, TickFrame},
        Event, SaveAccountSnap,
    },
    AccountId, Money, Side, StockCode,
};
use serde::{Serialize, Serializer};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Display,
};

#[path = "verification_evidence/phase_timing.rs"]
mod phase_timing;
pub(crate) use phase_timing::{
    begin_authoritative_tick, enter_phase, mark_committed, validate_precommit,
};
pub use phase_timing::{
    CommittedPhaseTiming, PhaseTimingCaptureError, PhaseTimingPhase, PhaseTimingRecord,
    RunnableThreadSample, TimedStep,
};

pub const OBSERVATION_SCHEMA: &str = "escrow-determinism-observation-v1";
pub const CONSERVATION_SCHEMA: &str = "escrow-conservation-snapshot-v1";

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
    #[error("pre-canonical {dimension} must contain at least two distinct real identities")]
    InvalidPrecanonicalOrder { dimension: &'static str },
    #[error("execution coverage is incomplete: {detail}")]
    IncompleteExecutionCoverage { detail: &'static str },
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
        #[serde(rename = "p1_live")]
        allocation_live: ResourceProjection,
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
    accounts: &BTreeMap<AccountId, SaveAccountSnap>,
) -> Result<ConservationSnapshot, EvidenceError> {
    require_text(scenario, "scenario")?;
    // Quiet ticks have an empty sum, not missing evidence. The run-level
    // coverage gate still requires real multi-stock and multi-leg execution.
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
        let mut allocation_live = basis;
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
                        allocation_live = live;
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
            allocation_live = live;
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
                allocation_live: project_resource(allocation_live, "envelope.p1_live")?,
            },
            EnvelopeOrigin::CreatedAtValidation => ConservationBasisProjection::Created {
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
                EnvelopeOrigin::CreatedAtValidation => "created",
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
    let mut seen_identities = BTreeSet::new();
    for (position, (index, local_key)) in identities.iter().enumerate() {
        if position > 0 {
            let (previous_index, _) = &identities[position - 1];
            if previous_index.checked_add(1) != Some(*index) {
                return Err(EvidenceError::ReceiptIndexSequence);
            }
        }
        if !seen_identities.insert(local_key) {
            return Err(EvidenceError::InvalidReceiptIdentity {
                receipt_index: *index,
                detail: "duplicate local receipt identity",
            });
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
        ReceiptKind::PriceResolved { .. } => "PriceResolved",
        ReceiptKind::Release => "Release",
        ReceiptKind::Reject => "Reject",
        ReceiptKind::Rollover => "Rollover",
    }
}

fn project_receipt_source(source: ReceiptSource) -> ReceiptSourceProjection {
    let kind = match source {
        ReceiptSource::QuoteExpiry(_) => "P0Expiry",
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct UpdateStreamCursor {
    tick: u64,
    seq_to: u64,
}

impl From<&UpdateProjection> for UpdateStreamCursor {
    fn from(update: &UpdateProjection) -> Self {
        Self {
            tick: update.tick,
            seq_to: update.seq_to,
        }
    }
}

/// Projects one complete, ordered runtime update stream while retaining the
/// ADR event-identity scope that spans update boundaries.
///
/// In particular, phase-6 `Session` facts in a `TickFrame` and a following
/// same-tick `CivilUpdate` share one ordinal domain. A failed projection is
/// transactional: callers may correct the rejected update and retry without
/// rebuilding the projector or corrupting the accepted prefix.
#[derive(Clone, Debug)]
pub struct UpdateStreamProjector {
    next_ordinals: BTreeMap<EventOrdinalDomain, u64>,
    comparison_keys: BTreeSet<ComparisonEventKey>,
    previous: Option<UpdateStreamCursor>,
    ordinal_error: &'static str,
    duplicate_error: &'static str,
    sequence_error: &'static str,
    sequence_field: &'static str,
    tick_field: &'static str,
}

impl Default for UpdateStreamProjector {
    fn default() -> Self {
        Self::new()
    }
}

impl UpdateStreamProjector {
    pub fn new() -> Self {
        Self::with_diagnostics(
            "local_event_index is not zero-based and contiguous in its full update-stream ADR domain",
            "comparison_event_key is duplicated across the full update stream",
            "full runtime update stream has a sequence or tick gap",
            "update_stream.seq_from",
            "update_stream.tick",
        )
    }

    fn with_diagnostics(
        ordinal_error: &'static str,
        duplicate_error: &'static str,
        sequence_error: &'static str,
        sequence_field: &'static str,
        tick_field: &'static str,
    ) -> Self {
        Self {
            next_ordinals: BTreeMap::new(),
            comparison_keys: BTreeSet::new(),
            previous: None,
            ordinal_error,
            duplicate_error,
            sequence_error,
            sequence_field,
            tick_field,
        }
    }

    pub fn project_update(
        &mut self,
        update: RuntimeUpdateRef<'_>,
    ) -> Result<UpdateProjection, EvidenceError> {
        let update_tick = match update {
            RuntimeUpdateRef::Tick(frame) => frame.tick,
            RuntimeUpdateRef::Civil(update) => update.tick,
        };
        let continues_same_tick = self
            .previous
            .as_ref()
            .is_some_and(|previous| previous.tick == update_tick);
        let mut next_ordinals = if continues_same_tick {
            self.next_ordinals.clone()
        } else {
            BTreeMap::new()
        };
        let mut comparison_keys = if continues_same_tick {
            self.comparison_keys.clone()
        } else {
            BTreeSet::new()
        };
        let projected = project_update_in_scope(
            update,
            &mut next_ordinals,
            &mut comparison_keys,
            self.ordinal_error,
            self.duplicate_error,
        )?;
        if let Some(previous) = self.previous.as_ref() {
            validate_update_transition(
                previous,
                &projected,
                self.sequence_error,
                self.sequence_field,
                self.tick_field,
            )?;
        }

        self.next_ordinals = next_ordinals;
        self.comparison_keys = comparison_keys;
        self.previous = Some(UpdateStreamCursor::from(&projected));
        Ok(projected)
    }
}

/// Projects a complete runtime update stream with the same stateful semantics
/// as [`UpdateStreamProjector`].
pub fn project_update_stream(
    updates: &[RuntimeUpdateRef<'_>],
) -> Result<Vec<UpdateProjection>, EvidenceError> {
    if updates.is_empty() {
        return Err(EvidenceError::InvalidUpdate {
            detail: "full runtime update stream contains no updates".to_owned(),
        });
    }
    let mut projector = UpdateStreamProjector::new();
    updates
        .iter()
        .copied()
        .map(|update| projector.project_update(update))
        .collect()
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
    if facts.is_empty() && kind != "TickFrame" {
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
        if fact.key.local_event_index() > crate::orderbook::js_safe_u64::MAX {
            return Err(EvidenceError::InvalidEventIdentity {
                detail: "local_event_index exceeds the JS-safe event identity domain",
            });
        }
        if matches!(entity, EntityTag::Account(_))
            && source == EventSourceIndex::Sealed
            && fact.key.local_event_index()
                >= crate::session::pipeline::QUOTE_EXPIRY_EVENT_INDEX_BASE
            && !matches!(fact.event, Event::OrderCanceled { .. })
        {
            return Err(EvidenceError::InvalidEventIdentity {
                detail: "the reserved P0 event identity index requires OrderCanceled",
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
        let sparse_account_domain =
            matches!(entity, EntityTag::Account(_)) && source == EventSourceIndex::Sealed;
        let domain = (tick, phase_rank, entity, source);
        let expected = next_ordinals.entry(domain).or_insert(0u64);
        if sparse_account_domain && fact.key.local_event_index() < *expected {
            return Err(EvidenceError::InvalidEventIdentity {
                detail: duplicate_error,
            });
        }
        if !sparse_account_domain && fact.key.local_event_index() != *expected {
            return Err(EvidenceError::InvalidEventIdentity {
                detail: ordinal_error,
            });
        }
        *expected = fact.key.local_event_index()
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
        Event::CivilDateAdvanced { .. } | Event::CompanyDisclosurePublished { .. } => {
            (6, EntityTag::Session, EventSourceIndex::Session)
        }
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
        | Event::CompanyDisclosurePublished { .. } => Ok("Session".to_owned()),
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
            crate::session::pipeline::EventSourceIndex::QuoteExpiry => "P0",
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
        crate::RejectionReason::OrderNotFound => "OrderNotFound",
        crate::RejectionReason::OrderAlreadyFilled => "OrderAlreadyFilled",
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
        (ObservationMode::NegativeControl, Some("completion")) => {}
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

fn validate_update_transition(
    previous: &UpdateStreamCursor,
    current: &UpdateProjection,
    sequence_error: &'static str,
    sequence_field: &'static str,
    tick_field: &'static str,
) -> Result<(), EvidenceError> {
    let expected_seq = previous
        .seq_to
        .checked_add(1)
        .ok_or(EvidenceError::IntegerOverflow {
            field: sequence_field,
            value: previous.seq_to,
        })?;
    let expected_tick = if current.kind == "CivilUpdate" {
        previous.tick
    } else {
        previous
            .tick
            .checked_add(1)
            .ok_or(EvidenceError::IntegerOverflow {
                field: tick_field,
                value: previous.tick,
            })?
    };
    if current.seq_from != expected_seq || current.tick != expected_tick {
        return Err(EvidenceError::InvalidUpdate {
            detail: sequence_error.to_owned(),
        });
    }
    Ok(())
}

#[cfg(test)]
#[path = "verification_evidence/tests.rs"]
mod tests;
