use super::super::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const SIMULATION_POLICY_ID: &str = "a-share-simulation";

/// 并行 tick 的权威运行时状态，在 quiet point 以 DTO 保存。
/// 不直接序列化 `EnvelopeLedger`，避免 tick 局部 terminal、守恒行与 local-key
/// 去重证据越过成功提交边界。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedRuntimeState {
    /// poisoned session 不允许生成存档；显式标记让恢复校验能够识别篡改。
    pub poisoned: bool,
    #[serde(with = "super::super::u64_decimal")]
    pub next_receipt_base: u64,
    pub live_envelopes: Vec<SavedLiveEnvelope>,
    pub retail_projection_seen: Vec<SavedRetailReceiptIdentity>,
    /// 完整生产策略状态是权威事实；profile 从 enum value 派生，不重复保存。
    pub strategy_states: BTreeMap<AccountId, crate::strategy::StrategyState>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedLiveEnvelope {
    pub key: SavedEnvelopeKey,
    pub charged: SavedFeeComponents,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedEnvelopeKey {
    pub account: AccountId,
    pub stock: StockCode,
    pub order: OrderId,
    pub side: Side,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedFeeComponents {
    pub commission: Money,
    pub stamp_tax: Money,
    pub transfer_fee: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedRetailReceiptIdentity {
    #[serde(with = "super::super::u64_decimal")]
    pub index: u64,
    pub local_key: SavedReceiptLocalKey,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedReceiptLocalKey {
    pub journal: SavedJournalRank,
    pub source: SavedReceiptSource,
    pub transition: SavedReceiptTransition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SavedJournalRank {
    PreSeal,
    SealedBatch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SavedReceiptSource {
    SealedIntent(#[serde(with = "super::super::u64_decimal")] u64),
    QuoteExpiry(u32),
    Auction(u32),
    DayEnd(u32),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedReceiptTransition {
    pub envelope: SavedEnvelopeKey,
    #[serde(with = "super::super::u64_decimal")]
    pub ordinal: u64,
}

/// 从健康且已提交的 quiet point 捕获全部权威运行状态。
pub fn capture_runtime_state(session: &GameSession) -> Result<SavedRuntimeState, StepFatal> {
    session.require_healthy()?;
    if session.state.setup.simulation_policy_id != SIMULATION_POLICY_ID {
        return Err(invariant(format!(
            "simulation_policy_id {:?} 不允许写入当前存档",
            session.state.setup.simulation_policy_id
        )));
    }
    session.state.envelope_ledger.validate_complete_evidence()?;
    if session.state.envelope_ledger.terminal_count() != 0 {
        return Err(invariant(
            "quiet-point envelope ledger retains terminal tick evidence".to_owned(),
        ));
    }
    if session.state.envelope_ledger.next_receipt_index() != session.state.next_receipt_base {
        return Err(invariant(format!(
            "envelope ledger cursor {} disagrees with next_receipt_base {}",
            session.state.envelope_ledger.next_receipt_index(),
            session.state.next_receipt_base
        )));
    }

    let live_envelopes = session
        .state
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
            Ok(SavedLiveEnvelope::from_envelope(envelope))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut rebased_ledger = session.state.envelope_ledger.clone();
    rebased_ledger.rebase_live_for_next_tick()?;
    if rebased_ledger != session.state.envelope_ledger {
        return Err(invariant(
            "save requires a fully rebased envelope-ledger quiet point".to_owned(),
        ));
    }

    let retail_projection_seen = session
        .state
        .retail_projection_seen
        .authoritative_identities()
        .into_iter()
        .map(|(index, key)| {
            Ok(SavedRetailReceiptIdentity {
                index,
                local_key: SavedReceiptLocalKey::from_runtime(&key)?,
            })
        })
        .collect::<Result<Vec<_>, StepFatal>>()?;

    let strategy_states = session
        .state
        .accounts
        .iter()
        .filter_map(|(account, value)| {
            value.strategy().map(|strategy| {
                strategy
                    .production_state()
                    .map(|state| (*account, state))
                    .map_err(|error| invariant(error.to_string()))
            })
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;

    let state = SavedRuntimeState {
        poisoned: false,
        next_receipt_base: session.state.next_receipt_base,
        live_envelopes,
        retail_projection_seen,
        strategy_states,
    };
    validate_runtime_state(session, &state).map_err(session_error_as_invariant)?;

    let reconstructed = state
        .build_ledger(session)
        .map_err(session_error_as_invariant)?;
    let actual: Vec<_> = session
        .state
        .envelope_ledger
        .iter()
        .map(|(_, envelope)| envelope.clone())
        .collect();
    let projected: Vec<_> = reconstructed
        .iter()
        .map(|(_, envelope)| envelope.clone())
        .collect();
    if actual != projected {
        return Err(invariant("SavedLiveEnvelope DTO 无法无损恢复".to_owned()));
    }
    Ok(state)
}

/// 将 DTO 与已恢复的订单及确定性账户身份交叉校验；不修改或清空损坏数据。
pub fn validate_runtime_state(
    session: &GameSession,
    state: &SavedRuntimeState,
) -> Result<(), SessionError> {
    if state.poisoned {
        return Err(SessionError::InvalidSave(
            "runtime_state 明确拒绝 poisoned session 状态".to_owned(),
        ));
    }
    if session.state.setup.simulation_policy_id != SIMULATION_POLICY_ID {
        return Err(SessionError::InvalidSave(format!(
            "当前存档要求 simulation_policy_id 为 {SIMULATION_POLICY_ID:?}，实际为 {:?}",
            session.state.setup.simulation_policy_id
        )));
    }

    let ledger = state.build_ledger(session)?;
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
            SavedReceiptLocalKey::from_runtime(&local_key)
                .map(|local_key| SavedRetailReceiptIdentity { index, local_key })
                .map_err(|error| invalid_save("saved retail projection canonical identity", error))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if canonical.as_slice() != state.retail_projection_seen.as_slice() {
        return Err(SessionError::InvalidSave(
            "saved retail projection identities are not canonical".to_owned(),
        ));
    }

    let expected_strategy_accounts: BTreeSet<_> = session
        .state
        .accounts
        .iter()
        .filter_map(|(account, value)| value.strategy().map(|_| *account))
        .collect();
    let saved_strategy_accounts: BTreeSet<_> = state.strategy_states.keys().copied().collect();
    if saved_strategy_accounts != expected_strategy_accounts {
        return Err(SessionError::InvalidSave(
            "saved StrategyState account set does not exactly match NPC accounts".to_owned(),
        ));
    }
    for (account, saved_state) in &state.strategy_states {
        let expected_profile = session
            .state
            .accounts
            .get(account)
            .ok_or_else(|| {
                SessionError::InvalidSave(format!(
                    "saved StrategyState refers to missing account {}",
                    account.0
                ))
            })?
            .strategy()
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
            .state
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
    identity: &SavedRetailReceiptIdentity,
) -> Result<(), SessionError> {
    let envelope = &identity.local_key.transition.envelope;
    if !session.state.accounts.contains_key(&envelope.account) {
        return Err(SessionError::InvalidSave(format!(
            "saved receipt {} refers to unknown account {}",
            identity.index, envelope.account.0
        )));
    }
    if !session.state.markets.contains_key(&envelope.stock) {
        return Err(SessionError::InvalidSave(format!(
            "saved receipt {} refers to unknown stock {:?}",
            identity.index, envelope.stock
        )));
    }
    if envelope.order.0 == 0 || envelope.order.0 >= session.state.next_order_id {
        return Err(SessionError::InvalidSave(format!(
            "saved receipt {} has invalid order {}; expected 1..{}",
            identity.index, envelope.order.0, session.state.next_order_id
        )));
    }
    Ok(())
}

/// 基础 SaveSlot 校验完成后，原子恢复全部权威运行状态。
pub fn restore_runtime_state(
    session: &mut GameSession,
    state: &SavedRuntimeState,
) -> Result<(), SessionError> {
    validate_runtime_state(session, state)?;
    let ledger = state.build_ledger(session)?;
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
        .state
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
            .restore_strategy(Some(strategy));
    }
    session.state.accounts = accounts;
    session.state.envelope_ledger = ledger;
    session.state.retail_projection_seen = seen;
    session.state.next_receipt_base = state.next_receipt_base;
    Ok(())
}

impl SavedRuntimeState {
    fn build_ledger(
        &self,
        session: &GameSession,
    ) -> Result<pipeline::EnvelopeLedger, SessionError> {
        let projected = session
            .project_live_envelopes()
            .map_err(|error| invalid_save("saved live order projection", error))?
            .into_iter()
            .map(|envelope| (SavedEnvelopeKey::from_runtime(envelope.key()), envelope))
            .collect::<BTreeMap<_, _>>();
        let mut previous = None;
        let mut envelopes = Vec::with_capacity(self.live_envelopes.len());
        for saved in &self.live_envelopes {
            if previous.as_ref().is_some_and(|key| key >= &saved.key) {
                return Err(SessionError::InvalidSave(
                    "saved live envelopes are not in strict canonical key order".to_owned(),
                ));
            }
            previous = Some(saved.key.clone());
            let order_envelope = projected.get(&saved.key).ok_or_else(|| {
                SessionError::InvalidSave(format!(
                    "saved live envelope {:?} has no matching active order",
                    saved.key
                ))
            })?;
            envelopes.push(saved.to_envelope(order_envelope)?);
        }
        let ledger = pipeline::EnvelopeLedger::new(self.next_receipt_base, envelopes)
            .map_err(|error| invalid_save("saved live envelope ledger", error))?;
        ledger
            .validate_complete_evidence()
            .map_err(|error| invalid_save("saved live envelope ledger evidence", error))?;
        Ok(ledger)
    }
}

impl SavedLiveEnvelope {
    fn from_envelope(envelope: &pipeline::Envelope) -> Self {
        Self {
            key: SavedEnvelopeKey::from_runtime(envelope.key()),
            charged: SavedFeeComponents::from_runtime(envelope.audit().charged),
        }
    }

    fn to_envelope(
        &self,
        projected: &pipeline::Envelope,
    ) -> Result<pipeline::Envelope, SessionError> {
        if projected.key() != &self.key.to_runtime() {
            return Err(SessionError::InvalidSave(format!(
                "saved envelope {:?} disagrees with its active order identity",
                self.key
            )));
        }
        let mut audit = projected.audit();
        audit.charged = self.charged.to_runtime();
        let envelope = pipeline::Envelope::tick_start_existing(
            self.key.to_runtime(),
            projected.live().cash,
            projected.live().shares,
            audit,
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

impl SavedEnvelopeKey {
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

impl Ord for SavedEnvelopeKey {
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

impl PartialOrd for SavedEnvelopeKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl SavedFeeComponents {
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
    state: &SavedRuntimeState,
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
    for key in expected.keys() {
        let saved = actual.get(key).ok_or_else(|| {
            SessionError::InvalidSave(format!("live order {key:?} has no saved envelope"))
        })?;
        let saved_audit = saved.audit();
        let audit = CumulativeFeeAudit {
            side: key.side,
            filled_value: saved_audit.filled_value,
            charged: SavedFeeComponents::from_runtime(saved_audit.charged),
        };
        if !audit.validate(&session.state.setup.config)? {
            return Err(SessionError::InvalidSave(format!(
                "saved envelope {key:?} has inconsistent cumulative charged fee audit"
            )));
        }
    }
    Ok(())
}

/// 活动委托的累计收费审计；GameConfig 与 EnvelopeAudit 仍各自拥有费率和运行时事实。
/// 此处只校验累计边界，不能从累计分量重建逐笔收费优先级。
pub(super) struct CumulativeFeeAudit {
    pub(super) side: Side,
    pub(super) filled_value: Money,
    pub(super) charged: SavedFeeComponents,
}

impl CumulativeFeeAudit {
    pub(super) fn nominal(&self, config: &GameConfig) -> Result<SavedFeeComponents, SessionError> {
        if self.filled_value.cents() < 0 {
            return Err(SessionError::InvalidSave(
                "saved filled value cannot be negative".to_owned(),
            ));
        }
        if self.filled_value == Money::ZERO {
            return Ok(SavedFeeComponents::default());
        }
        Ok(SavedFeeComponents {
            commission: config
                .commission(self.filled_value)
                .map_err(|error| invalid_save("saved cumulative commission", error))?,
            stamp_tax: match self.side {
                Side::Buy => Money::ZERO,
                Side::Sell => config
                    .stamp_tax(self.filled_value)
                    .map_err(|error| invalid_save("saved cumulative stamp tax", error))?,
            },
            transfer_fee: config
                .transfer_fee(self.filled_value)
                .map_err(|error| invalid_save("saved cumulative transfer fee", error))?,
        })
    }

    /// 返回累计分量是否一致，由遍历调用方附加委托身份错误上下文。
    pub(super) fn validate(&self, config: &GameConfig) -> Result<bool, SessionError> {
        let nominal = self.nominal(config)?;
        let charged = self.charged;
        nominal.validate_nonnegative("nominal")?;
        charged.validate_nonnegative("charged")?;
        if self.side == Side::Buy {
            return Ok(charged == nominal);
        }
        let component_bounds_hold = charged.commission <= nominal.commission
            && charged.stamp_tax <= nominal.stamp_tax
            && charged.transfer_fee <= nominal.transfer_fee;
        let charged_total = charged.total()?;
        Ok(component_bounds_hold
            && charged_total <= nominal.total()?
            && charged_total <= self.filled_value)
    }
}

impl SavedReceiptLocalKey {
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
        Ok(mirror.into_saved_receipt_local_key())
    }

    fn to_runtime(&self) -> Result<pipeline::ReceiptLocalKey, SessionError> {
        let journal = match self.journal {
            SavedJournalRank::PreSeal => pipeline::JournalRank::PreSeal,
            SavedJournalRank::SealedBatch => pipeline::JournalRank::SealedBatch,
        };
        let source = match self.source {
            SavedReceiptSource::SealedIntent(value) => pipeline::ReceiptSource::SealedIntent(value),
            SavedReceiptSource::QuoteExpiry(value) => pipeline::ReceiptSource::QuoteExpiry(value),
            SavedReceiptSource::Auction(value) => pipeline::ReceiptSource::Auction(value),
            SavedReceiptSource::DayEnd(value) => pipeline::ReceiptSource::DayEnd(value),
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
    journal: SavedJournalRank,
    source: RuntimeReceiptSource,
    transition: RuntimeReceiptTransition,
}

#[derive(Deserialize)]
enum RuntimeReceiptSource {
    SealedIntent(u64),
    QuoteExpiry(u32),
    Auction(u32),
    DayEnd(u32),
}

#[derive(Deserialize)]
struct RuntimeReceiptTransition {
    envelope: SavedEnvelopeKey,
    ordinal: u64,
}

impl RuntimeReceiptLocalKey {
    fn into_saved_receipt_local_key(self) -> SavedReceiptLocalKey {
        SavedReceiptLocalKey {
            journal: self.journal,
            source: match self.source {
                RuntimeReceiptSource::SealedIntent(value) => {
                    SavedReceiptSource::SealedIntent(value)
                }
                RuntimeReceiptSource::QuoteExpiry(value) => SavedReceiptSource::QuoteExpiry(value),
                RuntimeReceiptSource::Auction(value) => SavedReceiptSource::Auction(value),
                RuntimeReceiptSource::DayEnd(value) => SavedReceiptSource::DayEnd(value),
            },
            transition: SavedReceiptTransition {
                envelope: self.transition.envelope,
                ordinal: self.transition.ordinal,
            },
        }
    }
}

fn invariant(description: String) -> StepFatal {
    StepFatal::InvariantViolation {
        description,
        location: "session::persistence::saved_runtime".to_owned(),
    }
}

fn session_error_as_invariant(error: SessionError) -> StepFatal {
    invariant(error.to_string())
}

fn invalid_save(context: &str, error: impl std::fmt::Display) -> SessionError {
    SessionError::InvalidSave(format!("{context} is invalid: {error}"))
}
