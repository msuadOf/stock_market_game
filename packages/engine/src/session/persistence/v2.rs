use super::super::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const SAVE_SCHEMA_VERSION_V2: u32 = 2;
pub const SIMULATION_POLICY_ID_V2: &str = "a-share-simulation-v2";

/// The parallel-tick state that schema v2 adds to the pre-existing session
/// payload. It is deliberately a quiet-point DTO instead of a serialized
/// `EnvelopeLedger`: tick-local terminals, conservation rows and local-key
/// dedupe evidence must not leak across a successful commit boundary.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveRuntimeV2 {
    /// A poisoned session can never truthfully produce a save. Keeping the
    /// marker explicit also makes tampering detectable at restore time.
    pub poisoned: bool,
    #[serde(with = "super::super::u64_decimal")]
    pub next_receipt_base: u64,
    pub live_envelopes: Vec<LiveEnvelopeV2>,
    pub retail_projection_seen: Vec<RetailReceiptIdentityV2>,
    /// Complete production strategy state is authoritative. A profile is
    /// derived from each enum value and is not persisted beside it.
    pub strategy_states: BTreeMap<AccountId, crate::strategy::StrategyState>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveEnvelopeV2 {
    pub key: EnvelopeKeyV2,
    pub live: ResourceV2,
    pub audit: EnvelopeAuditV2,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvelopeKeyV2 {
    pub account: AccountId,
    pub stock: StockCode,
    pub order: OrderId,
    pub side: Side,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceV2 {
    pub cash: Money,
    pub shares: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvelopeAuditV2 {
    pub limit: Money,
    pub remaining_qty: u32,
    pub filled_qty: u32,
    pub filled_value: Money,
    pub nominal: FeeComponentsV2,
    pub charged: FeeComponentsV2,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeeComponentsV2 {
    pub commission: Money,
    pub stamp_tax: Money,
    pub transfer_fee: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetailReceiptIdentityV2 {
    #[serde(with = "super::super::u64_decimal")]
    pub index: u64,
    pub local_key: ReceiptLocalKeyV2,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptLocalKeyV2 {
    pub journal: JournalRankV2,
    pub source: ReceiptSourceV2,
    pub transition: ReceiptTransitionV2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum JournalRankV2 {
    PreSeal,
    SealedBatch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReceiptSourceV2 {
    SealedIntent(#[serde(with = "super::super::u64_decimal")] u64),
    P0Expiry(u32),
    Auction(u32),
    DayEnd(u32),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptTransitionV2 {
    pub envelope: EnvelopeKeyV2,
    #[serde(with = "super::super::u64_decimal")]
    pub ordinal: u64,
}

#[derive(Deserialize)]
struct SchemaVersionHeader {
    schema_version: Option<u32>,
}

/// Rejects missing, legacy and future versions before deserializing the full
/// save. There is intentionally no legacy branch and no migration fallback.
pub fn validate_schema_version_header(json: &[u8]) -> Result<(), SessionError> {
    let header: SchemaVersionHeader = serde_json::from_slice(json).map_err(|error| {
        SessionError::InvalidSave(format!("save schema header is not decodable: {error}"))
    })?;
    let version = header.schema_version.ok_or_else(|| {
        SessionError::InvalidSave(
            "save schema_version is missing; legacy saves are not supported".to_owned(),
        )
    })?;
    validate_schema_version(version)
}

pub fn validate_schema_version(version: u32) -> Result<(), SessionError> {
    match version.cmp(&SAVE_SCHEMA_VERSION_V2) {
        std::cmp::Ordering::Equal => Ok(()),
        std::cmp::Ordering::Less => Err(SessionError::InvalidSave(format!(
            "save schema_version {version} is legacy; schema v2 requires an explicit new save"
        ))),
        std::cmp::Ordering::Greater => Err(SessionError::InvalidSave(format!(
            "save schema_version {version} is newer than supported schema v2"
        ))),
    }
}

/// Captures every v2-only authority from a healthy, committed quiet point.
pub fn capture_runtime_v2(session: &GameSession) -> Result<SaveRuntimeV2, StepFatal> {
    session.require_healthy()?;
    if session.setup.simulation_policy_id != SIMULATION_POLICY_ID_V2 {
        return Err(invariant(format!(
            "simulation policy {:?} cannot be written as schema v2",
            session.setup.simulation_policy_id
        )));
    }
    session.envelope_ledger.validate_complete_evidence()?;
    if session.envelope_ledger.terminal_count() != 0 {
        return Err(invariant(
            "quiet-point envelope ledger retains terminal tick evidence".to_owned(),
        ));
    }
    if session.envelope_ledger.next_receipt_index() != session.next_receipt_base {
        return Err(invariant(format!(
            "envelope ledger cursor {} disagrees with next_receipt_base {}",
            session.envelope_ledger.next_receipt_index(),
            session.next_receipt_base
        )));
    }

    let live_envelopes = session
        .envelope_ledger
        .iter()
        .map(|(_, envelope)| {
            if envelope.origin() != pipeline::EnvelopeOrigin::TickStart
                || envelope.basis() != envelope.live()
                || envelope.spent() != pipeline::ResVec::ZERO
                || envelope.released() != pipeline::ResVec::ZERO
            {
                return Err(invariant(format!(
                    "envelope {:?} is not rebased at the save quiet point",
                    envelope.key()
                )));
            }
            Ok(LiveEnvelopeV2::from_envelope(envelope))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut rebased_ledger = session.envelope_ledger.clone();
    rebased_ledger.rebase_live_for_next_tick()?;
    if rebased_ledger != session.envelope_ledger {
        return Err(invariant(
            "save requires a fully rebased envelope-ledger quiet point".to_owned(),
        ));
    }

    let retail_projection_seen = session
        .retail_projection_seen
        .authoritative_identities()
        .into_iter()
        .map(|(index, key)| {
            Ok(RetailReceiptIdentityV2 {
                index,
                local_key: ReceiptLocalKeyV2::from_runtime(&key)?,
            })
        })
        .collect::<Result<Vec<_>, StepFatal>>()?;

    let strategy_states = session
        .accounts
        .iter()
        .filter_map(|(account, value)| {
            value.strategy.as_ref().map(|strategy| {
                strategy
                    .production_state()
                    .map(|state| (*account, state))
                    .map_err(|error| invariant(error.to_string()))
            })
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;

    let state = SaveRuntimeV2 {
        poisoned: false,
        next_receipt_base: session.next_receipt_base,
        live_envelopes,
        retail_projection_seen,
        strategy_states,
    };
    validate_runtime_v2(session, &state).map_err(session_error_as_invariant)?;

    let reconstructed = state.build_ledger().map_err(session_error_as_invariant)?;
    let actual: Vec<_> = session
        .envelope_ledger
        .iter()
        .map(|(_, envelope)| envelope.clone())
        .collect();
    let projected: Vec<_> = reconstructed
        .iter()
        .map(|(_, envelope)| envelope.clone())
        .collect();
    if actual != projected {
        return Err(invariant("v2 live-envelope DTO is not lossless".to_owned()));
    }
    Ok(state)
}

/// Cross-validates the DTO against already-restored orders and deterministic
/// account identities. It performs no mutation and never clears damaged data.
pub fn validate_runtime_v2(
    session: &GameSession,
    state: &SaveRuntimeV2,
) -> Result<(), SessionError> {
    if state.poisoned {
        return Err(SessionError::InvalidSave(
            "schema v2 explicitly rejects poisoned session state".to_owned(),
        ));
    }
    if session.setup.simulation_policy_id != SIMULATION_POLICY_ID_V2 {
        return Err(SessionError::InvalidSave(format!(
            "schema v2 requires simulation policy {SIMULATION_POLICY_ID_V2:?}, got {:?}",
            session.setup.simulation_policy_id
        )));
    }

    let ledger = state.build_ledger()?;
    validate_live_envelopes_against_orders(session, state, &ledger)?;

    let identities = state
        .retail_projection_seen
        .iter()
        .map(|identity| {
            validate_receipt_identity_domain(session, identity)?;
            identity
                .local_key
                .to_runtime()
                .map(|key| (identity.index, key))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let seen = pipeline::RetailProjectionSeen::from_authoritative_identities(
        identities,
        state.next_receipt_base,
    )
    .map_err(|error| invalid_save("saved retail projection receipt prefix", error))?;
    let canonical = seen
        .authoritative_identities()
        .into_iter()
        .map(|(index, local_key)| {
            ReceiptLocalKeyV2::from_runtime(&local_key)
                .map(|local_key| RetailReceiptIdentityV2 { index, local_key })
                .map_err(|error| invalid_save("saved retail projection canonical identity", error))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if canonical.as_slice() != state.retail_projection_seen.as_slice() {
        return Err(SessionError::InvalidSave(
            "saved retail projection identities are not canonical".to_owned(),
        ));
    }

    let expected_strategy_accounts: BTreeSet<_> = session
        .accounts
        .iter()
        .filter_map(|(account, value)| value.strategy.as_ref().map(|_| *account))
        .collect();
    let saved_strategy_accounts: BTreeSet<_> = state.strategy_states.keys().copied().collect();
    if saved_strategy_accounts != expected_strategy_accounts {
        return Err(SessionError::InvalidSave(
            "saved StrategyState account set does not exactly match NPC accounts".to_owned(),
        ));
    }
    for (account, saved_state) in &state.strategy_states {
        let expected_profile = session
            .accounts
            .get(account)
            .ok_or_else(|| {
                SessionError::InvalidSave(format!(
                    "saved StrategyState refers to missing account {}",
                    account.0
                ))
            })?
            .strategy
            .as_ref()
            .ok_or_else(|| {
                SessionError::InvalidSave(format!(
                    "saved StrategyState refers to strategy-free account {}",
                    account.0
                ))
            })?
            .profile();
        if saved_state.profile() != expected_profile {
            return Err(SessionError::InvalidSave(format!(
                "saved StrategyState for account {} changes its deterministic strategy identity",
                account.0
            )));
        }
        let attention_probability = session
            .npc_attention
            .get(account)
            .ok_or_else(|| {
                SessionError::InvalidSave(format!(
                    "saved StrategyState for account {} has no NPC attention state",
                    account.0
                ))
            })?
            .base_probability;
        if saved_state.base_observation_probability().to_bits() != attention_probability.to_bits() {
            return Err(SessionError::InvalidSave(format!(
                "saved StrategyState for account {} base observation probability {} disagrees with NPC attention probability {}",
                account.0,
                saved_state.base_observation_probability(),
                attention_probability
            )));
        }
        saved_state.clone().into_strategy().map_err(|error| {
            SessionError::InvalidSave(format!(
                "saved StrategyState for account {} is invalid: {error}",
                account.0
            ))
        })?;
    }
    Ok(())
}

fn validate_receipt_identity_domain(
    session: &GameSession,
    identity: &RetailReceiptIdentityV2,
) -> Result<(), SessionError> {
    let envelope = &identity.local_key.transition.envelope;
    if !session.accounts.contains_key(&envelope.account) {
        return Err(SessionError::InvalidSave(format!(
            "saved receipt {} refers to unknown account {}",
            identity.index, envelope.account.0
        )));
    }
    if !session.markets.contains_key(&envelope.stock) {
        return Err(SessionError::InvalidSave(format!(
            "saved receipt {} refers to unknown stock {:?}",
            identity.index, envelope.stock
        )));
    }
    if envelope.order.0 == 0 || envelope.order.0 >= session.next_order_id {
        return Err(SessionError::InvalidSave(format!(
            "saved receipt {} has invalid order {}; expected 1..{}",
            identity.index, envelope.order.0, session.next_order_id
        )));
    }
    Ok(())
}

/// Atomically hydrates all v2-only authority after the legacy fields and live
/// orders have been restored and cross-validated.
pub fn restore_runtime_v2(
    session: &mut GameSession,
    state: &SaveRuntimeV2,
) -> Result<(), SessionError> {
    validate_runtime_v2(session, state)?;
    let ledger = state.build_ledger()?;
    let identities = state
        .retail_projection_seen
        .iter()
        .map(|identity| {
            identity
                .local_key
                .to_runtime()
                .map(|key| (identity.index, key))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let seen = pipeline::RetailProjectionSeen::from_authoritative_identities(
        identities,
        state.next_receipt_base,
    )
    .map_err(|error| invalid_save("saved retail projection receipt prefix", error))?;
    let strategies = state
        .strategy_states
        .iter()
        .map(|(account, saved)| {
            saved
                .clone()
                .into_strategy()
                .map(crate::account::StoredStrategy::production)
                .map(|strategy| (*account, strategy))
                .map_err(|error| {
                    SessionError::InvalidSave(format!(
                        "saved StrategyState for account {} is invalid: {error}",
                        account.0
                    ))
                })
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;

    let mut accounts = session
        .accounts
        .iter()
        .map(|(account, value)| {
            value
                .clone_for_shadow()
                .map(|value| (*account, value))
                .map_err(|error| {
                    SessionError::InvalidSave(format!(
                        "cannot prepare account {} for atomic strategy restore: {error}",
                        account.0
                    ))
                })
        })
        .collect::<Result<AccountBook, _>>()?;
    for (account, strategy) in strategies {
        accounts
            .get_mut(&account)
            .ok_or_else(|| {
                SessionError::InvalidSave(format!(
                    "saved StrategyState refers to missing account {}",
                    account.0
                ))
            })?
            .strategy = Some(strategy);
    }
    session.accounts = accounts;
    session.envelope_ledger = ledger;
    session.retail_projection_seen = seen;
    session.next_receipt_base = state.next_receipt_base;
    Ok(())
}

impl SaveRuntimeV2 {
    fn build_ledger(&self) -> Result<pipeline::EnvelopeLedger, SessionError> {
        let mut previous = None;
        let mut envelopes = Vec::with_capacity(self.live_envelopes.len());
        for saved in &self.live_envelopes {
            if previous.as_ref().is_some_and(|key| key >= &saved.key) {
                return Err(SessionError::InvalidSave(
                    "saved live envelopes are not in strict canonical key order".to_owned(),
                ));
            }
            previous = Some(saved.key.clone());
            envelopes.push(saved.to_envelope()?);
        }
        let ledger = pipeline::EnvelopeLedger::new(self.next_receipt_base, envelopes)
            .map_err(|error| invalid_save("saved live envelope ledger", error))?;
        ledger
            .validate_complete_evidence()
            .map_err(|error| invalid_save("saved live envelope ledger evidence", error))?;
        Ok(ledger)
    }
}

impl LiveEnvelopeV2 {
    fn from_envelope(envelope: &pipeline::Envelope) -> Self {
        Self {
            key: EnvelopeKeyV2::from_runtime(envelope.key()),
            live: ResourceV2::from_runtime(envelope.live()),
            audit: EnvelopeAuditV2::from_runtime(envelope.audit()),
        }
    }

    fn to_envelope(&self) -> Result<pipeline::Envelope, SessionError> {
        let envelope = pipeline::Envelope::tick_start_existing(
            self.key.to_runtime(),
            self.live.cash,
            self.live.shares,
            self.audit.to_runtime(),
        );
        envelope
            .validate()
            .map_err(|error| invalid_save("saved live envelope", error))?;
        if envelope.live() == pipeline::ResVec::ZERO {
            return Err(SessionError::InvalidSave(format!(
                "saved live envelope {:?} has no live resources",
                envelope.key()
            )));
        }
        Ok(envelope)
    }
}

impl EnvelopeKeyV2 {
    fn from_runtime(key: &pipeline::EnvelopeKey) -> Self {
        Self {
            account: key.account,
            stock: key.stock.clone(),
            order: key.order,
            side: key.side,
        }
    }

    fn to_runtime(&self) -> pipeline::EnvelopeKey {
        pipeline::EnvelopeKey {
            account: self.account,
            stock: self.stock.clone(),
            order: self.order,
            side: self.side,
        }
    }
}

impl Ord for EnvelopeKeyV2 {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let side_rank = |side: Side| match side {
            Side::Buy => 0_u8,
            Side::Sell => 1_u8,
        };
        (
            &self.account,
            &self.stock,
            &self.order,
            side_rank(self.side),
        )
            .cmp(&(
                &other.account,
                &other.stock,
                &other.order,
                side_rank(other.side),
            ))
    }
}

impl PartialOrd for EnvelopeKeyV2 {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl ResourceV2 {
    fn from_runtime(resource: pipeline::ResVec) -> Self {
        Self {
            cash: resource.cash,
            shares: resource.shares,
        }
    }
}

impl EnvelopeAuditV2 {
    fn from_runtime(audit: pipeline::EnvelopeAudit) -> Self {
        Self {
            limit: audit.limit,
            remaining_qty: audit.remaining_qty,
            filled_qty: audit.filled_qty,
            filled_value: audit.filled_value,
            nominal: FeeComponentsV2::from_runtime(audit.nominal),
            charged: FeeComponentsV2::from_runtime(audit.charged),
        }
    }

    fn to_runtime(self) -> pipeline::EnvelopeAudit {
        pipeline::EnvelopeAudit {
            limit: self.limit,
            remaining_qty: self.remaining_qty,
            filled_qty: self.filled_qty,
            filled_value: self.filled_value,
            nominal: self.nominal.to_runtime(),
            charged: self.charged.to_runtime(),
        }
    }
}

impl FeeComponentsV2 {
    fn from_runtime(fees: pipeline::FeeComponents) -> Self {
        Self {
            commission: fees.commission,
            stamp_tax: fees.stamp_tax,
            transfer_fee: fees.transfer_fee,
        }
    }

    fn to_runtime(self) -> pipeline::FeeComponents {
        pipeline::FeeComponents {
            commission: self.commission,
            stamp_tax: self.stamp_tax,
            transfer_fee: self.transfer_fee,
        }
    }

    fn total(self) -> Result<Money, SessionError> {
        self.commission
            .add(self.stamp_tax)
            .and_then(|subtotal| subtotal.add(self.transfer_fee))
            .map_err(|error| invalid_save("saved fee audit total", error))
    }

    fn validate_nonnegative(self, label: &str) -> Result<(), SessionError> {
        if [self.commission, self.stamp_tax, self.transfer_fee]
            .into_iter()
            .any(|amount| amount.cents() < 0)
        {
            return Err(SessionError::InvalidSave(format!(
                "saved {label} fee audit contains a negative component"
            )));
        }
        Ok(())
    }
}

fn validate_live_envelopes_against_orders(
    session: &GameSession,
    state: &SaveRuntimeV2,
    ledger: &pipeline::EnvelopeLedger,
) -> Result<(), SessionError> {
    let expected = session
        .project_live_envelopes()
        .map_err(|error| invalid_save("saved live order projection", error))?
        .into_iter()
        .map(|envelope| (envelope.key().clone(), envelope))
        .collect::<BTreeMap<_, _>>();
    let actual = ledger
        .iter()
        .map(|(key, envelope)| (key.clone(), envelope))
        .collect::<BTreeMap<_, _>>();
    if actual.len() != state.live_envelopes.len() || actual.len() != expected.len() {
        return Err(SessionError::InvalidSave(
            "saved live envelope set does not exactly match live order ownership".to_owned(),
        ));
    }
    for (key, projected) in expected {
        let saved = actual.get(&key).ok_or_else(|| {
            SessionError::InvalidSave(format!("live order {key:?} has no saved envelope"))
        })?;
        let projected_audit = projected.audit();
        let saved_audit = saved.audit();
        if saved.live() != projected.live()
            || saved_audit.limit != projected_audit.limit
            || saved_audit.remaining_qty != projected_audit.remaining_qty
            || saved_audit.filled_qty != projected_audit.filled_qty
            || saved_audit.filled_value != projected_audit.filled_value
        {
            return Err(SessionError::InvalidSave(format!(
                "saved envelope {key:?} disagrees with its live order"
            )));
        }
        let nominal = nominal_fees(&session.setup.config, key.side, saved_audit.filled_value)?;
        if saved_audit.nominal != nominal
            || !charged_fees_are_valid(
                key.side,
                nominal,
                saved_audit.charged,
                saved_audit.filled_value,
            )?
        {
            return Err(SessionError::InvalidSave(format!(
                "saved envelope {key:?} has inconsistent cumulative nominal/charged fee audit"
            )));
        }
    }
    Ok(())
}

fn nominal_fees(
    config: &GameConfig,
    side: Side,
    filled_value: Money,
) -> Result<pipeline::FeeComponents, SessionError> {
    if filled_value.cents() < 0 {
        return Err(SessionError::InvalidSave(
            "saved filled value cannot be negative".to_owned(),
        ));
    }
    if filled_value == Money::ZERO {
        return Ok(pipeline::FeeComponents::ZERO);
    }
    Ok(pipeline::FeeComponents {
        commission: config
            .commission(filled_value)
            .map_err(|error| invalid_save("saved cumulative commission", error))?,
        stamp_tax: match side {
            Side::Buy => Money::ZERO,
            Side::Sell => config
                .stamp_tax(filled_value)
                .map_err(|error| invalid_save("saved cumulative stamp tax", error))?,
        },
        transfer_fee: config
            .transfer_fee(filled_value)
            .map_err(|error| invalid_save("saved cumulative transfer fee", error))?,
    })
}

fn charged_fees_are_valid(
    side: Side,
    nominal: pipeline::FeeComponents,
    charged: pipeline::FeeComponents,
    filled_value: Money,
) -> Result<bool, SessionError> {
    let nominal = FeeComponentsV2::from_runtime(nominal);
    let charged = FeeComponentsV2::from_runtime(charged);
    nominal.validate_nonnegative("nominal")?;
    charged.validate_nonnegative("charged")?;
    if side == Side::Buy {
        return Ok(charged == nominal);
    }
    let component_bounds_hold = charged.commission <= nominal.commission
        && charged.stamp_tax <= nominal.stamp_tax
        && charged.transfer_fee <= nominal.transfer_fee;
    let charged_total = charged.total()?;
    Ok(component_bounds_hold && charged_total <= nominal.total()? && charged_total <= filled_value)
}

impl ReceiptLocalKeyV2 {
    fn from_runtime(key: &pipeline::ReceiptLocalKey) -> Result<Self, StepFatal> {
        let encoded = serde_json::to_value(key).map_err(|error| {
            invariant(format!(
                "cannot project authoritative receipt local key: {error}"
            ))
        })?;
        let mirror: RuntimeReceiptLocalKey = serde_json::from_value(encoded).map_err(|error| {
            invariant(format!(
                "authoritative receipt local key has an unsupported shape: {error}"
            ))
        })?;
        Ok(mirror.into_v2())
    }

    fn to_runtime(&self) -> Result<pipeline::ReceiptLocalKey, SessionError> {
        let journal = match self.journal {
            JournalRankV2::PreSeal => pipeline::JournalRank::PreSeal,
            JournalRankV2::SealedBatch => pipeline::JournalRank::SealedBatch,
        };
        let source = match self.source {
            ReceiptSourceV2::SealedIntent(value) => pipeline::ReceiptSource::SealedIntent(value),
            ReceiptSourceV2::P0Expiry(value) => pipeline::ReceiptSource::P0Expiry(value),
            ReceiptSourceV2::Auction(value) => pipeline::ReceiptSource::Auction(value),
            ReceiptSourceV2::DayEnd(value) => pipeline::ReceiptSource::DayEnd(value),
        };
        pipeline::ReceiptLocalKey::new(
            journal,
            source,
            pipeline::ReceiptTransition {
                envelope: self.transition.envelope.to_runtime(),
                ordinal: self.transition.ordinal,
            },
        )
        .map_err(|error| invalid_save("saved receipt local key", error))
    }
}

#[derive(Deserialize)]
struct RuntimeReceiptLocalKey {
    journal: JournalRankV2,
    source: RuntimeReceiptSource,
    transition: RuntimeReceiptTransition,
}

#[derive(Deserialize)]
enum RuntimeReceiptSource {
    SealedIntent(u64),
    P0Expiry(u32),
    Auction(u32),
    DayEnd(u32),
}

#[derive(Deserialize)]
struct RuntimeReceiptTransition {
    envelope: EnvelopeKeyV2,
    ordinal: u64,
}

impl RuntimeReceiptLocalKey {
    fn into_v2(self) -> ReceiptLocalKeyV2 {
        ReceiptLocalKeyV2 {
            journal: self.journal,
            source: match self.source {
                RuntimeReceiptSource::SealedIntent(value) => ReceiptSourceV2::SealedIntent(value),
                RuntimeReceiptSource::P0Expiry(value) => ReceiptSourceV2::P0Expiry(value),
                RuntimeReceiptSource::Auction(value) => ReceiptSourceV2::Auction(value),
                RuntimeReceiptSource::DayEnd(value) => ReceiptSourceV2::DayEnd(value),
            },
            transition: ReceiptTransitionV2 {
                envelope: self.transition.envelope,
                ordinal: self.transition.ordinal,
            },
        }
    }
}

fn invariant(description: String) -> StepFatal {
    StepFatal::InvariantViolation {
        description,
        location: "session::persistence::v2".to_owned(),
    }
}

fn session_error_as_invariant(error: SessionError) -> StepFatal {
    invariant(error.to_string())
}

fn invalid_save(context: &str, error: impl std::fmt::Display) -> SessionError {
    SessionError::InvalidSave(format!("{context} is invalid: {error}"))
}
