//! Replays a sealed, exogenous controlled script on the current production path.
//! It never restores a legacy slot or reconstructs an old engine execution.
#[path = "escrow_verification_harness/digest.rs"]
mod digest;

use engine::{
    session::pipeline::{EnvelopeReceipt, FeeComponents, ResVec, TickCommitEvidence},
    session::{
        pipeline::{Envelope, EnvelopeKey, ReceiptKind},
        protocol::{ProtocolSession, TickFrame},
    },
    verification_evidence::{
        project_conservation_snapshot, project_controlled_sell_corpus, project_corpus_surface,
        ControlledContinuationBytes, ControlledSellInput, ControlledSellSurface,
        CorpusSurfaceInput, EnvelopeChainInput, FeedbackAuditInput, RuntimeUpdateRef,
        SaveLiveOrderIdentity, SaveRestoreContinuationCommand, SaveRestoreLiveOrderInput,
        SellerCostBasisInput, Sha256Provider, TradeRole, UpdateStreamProjector,
    },
    AccountId, AccountSnap, EnvelopeAuditV2, EnvelopeKeyV2, Event, FeeComponentsV2, Intent,
    LiveEnvelopeV2, Money, Order, OrderId, PositionSnap, ResourceV2, SessionSetup, Side, StockCode,
    SIMULATION_POLICY_ID_V2,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, Write},
    process,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    scenario: String,
    seed: String,
    setup: SessionSetup,
    initial_accounts: BTreeMap<AccountId, AccountSnap>,
    sealed_exogenous_script: Vec<(u64, Vec<Intent>)>,
    ticks: u64,
    restore_at_ticks: Vec<u64>,
    #[serde(default)]
    historical_surface_hints: Option<HistoricalSurfaceHints>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoricalSurfaceHints {
    #[serde(default)]
    controlled_sell: Option<ControlledSellHint>,
    #[serde(default)]
    acceptance_flip_legacy_sell_reservation_cents: Option<String>,
    #[serde(default)]
    three_leg_legacy_sell_reservation_cents: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ControlledSellHint {
    account: String,
    stock: StockCode,
    legacy_sell_reservation_cents: String,
}

#[derive(Clone)]
struct AccumulatedChain {
    envelope: Envelope,
    receipts: Vec<EnvelopeReceipt>,
}

struct ControlledCandidate {
    update_count: usize,
    envelope: Envelope,
    receipts: Vec<EnvelopeReceipt>,
    account_after: AccountSnap,
    submission_cash: Money,
    legacy_sell_reservation: Money,
    cost_basis: SellerCostBasisInput,
    sealed_exogenous_script: Vec<u8>,
    strategy_state: Vec<u8>,
    plan_state: Vec<u8>,
    pending_intents: Vec<u8>,
    restore_order: Vec<u8>,
    rng_cursor: u64,
}

struct ControlledSaveCandidate {
    update_count: usize,
    envelope_key: EnvelopeKey,
    save_bytes: Vec<u8>,
    restored_resave_bytes: Vec<u8>,
    uninterrupted_authority: Vec<u8>,
    restored_authority: Vec<u8>,
    continuation_input: Vec<u8>,
    uninterrupted_continuation: Vec<u8>,
    restored_continuation: Vec<u8>,
    continuation_frame: TickFrame,
    sealed_exogenous_script: Vec<u8>,
    strategy_state: Vec<u8>,
    plan_state: Vec<u8>,
    pending_intents: Vec<u8>,
    restore_order: Vec<u8>,
    rng_cursor: u64,
}

struct Sha256;

impl Sha256Provider for Sha256 {
    fn digest_hex(&self, bytes: &[u8]) -> String {
        digest::digest_hex(bytes)
    }
}

fn normalized(mut value: Value) -> Value {
    match &mut value {
        Value::Number(number) if number.is_i64() || number.is_u64() => {
            Value::String(number.to_string())
        }
        Value::Number(number)
            if number.as_f64().is_some_and(|value| {
                value.fract() == 0.0 && value.abs() <= 9_007_199_254_740_991.0
            }) =>
        {
            // JSON does not preserve 0 versus 0.0. The sealed JS adapter uses
            // decimal strings for either integral representation as well.
            Value::String(format!("{:.0}", number.as_f64().expect("guarded float")))
        }
        Value::Array(values) => {
            Value::Array(std::mem::take(values).into_iter().map(normalized).collect())
        }
        Value::Object(values) => Value::Object(
            std::mem::take(values)
                .into_iter()
                .map(|(key, value)| (key, normalized(value)))
                .collect(),
        ),
        _ => value,
    }
}

fn shared_state(session: &ProtocolSession) -> Result<Value, String> {
    let mut value = serde_json::to_value(session.game().save().map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    let state = value.as_object_mut().ok_or("save is not an object")?;
    // New schema/runtime representation is checked independently. Historical
    // streams only captured this explicit common semantic projection.
    state.remove("schema_version");
    state.remove("runtime_v2");
    state.insert(
        "strategy_profiles".into(),
        serde_json::to_value(session.game().account_strategy_profiles())
            .map_err(|error| error.to_string())?,
    );
    state
        .get_mut("snapshot")
        .and_then(Value::as_object_mut)
        .ok_or("missing snapshot")?
        .remove("seq");
    // #7 metadata is outside behavioral comparison, never a wildcard subtree.
    state
        .get_mut("setup")
        .and_then(Value::as_object_mut)
        .ok_or("missing setup")?
        .remove("simulation_policy_id");
    Ok(normalized(value))
}

fn semantic_bytes(value: &impl Serialize) -> Result<Vec<u8>, String> {
    let value = serde_json::to_value(value).map_err(|error| error.to_string())?;
    serde_json::to_vec(&normalized(value)).map_err(|error| error.to_string())
}

fn resource_projection(value: ResVec) -> Value {
    json!({
        "cash_cents": value.cash.cents().to_string(),
        "shares": value.shares.to_string(),
    })
}

fn fee_projection(value: FeeComponents) -> Value {
    json!({
        "commission_cents": value.commission.cents().to_string(),
        "stamp_tax_cents": value.stamp_tax.cents().to_string(),
        "transfer_fee_cents": value.transfer_fee.cents().to_string(),
    })
}

fn receipt_projection(receipt: &EnvelopeReceipt) -> Value {
    normalized(json!({
        "index": receipt.index.to_string(),
        "local_key": receipt.local_key,
        "envelope": receipt.envelope,
        "kind": format!("{:?}", receipt.kind),
        "qty_before": receipt.qty_before.to_string(),
        "qty_after": receipt.qty_after.to_string(),
        "value_before_cents": receipt.value_before.cents().to_string(),
        "value_after_cents": receipt.value_after.cents().to_string(),
        "spent": resource_projection(receipt.delta.spent),
        "released": resource_projection(receipt.delta.released),
        "live_after": resource_projection(receipt.delta.live_after),
        "nominal": fee_projection(receipt.nominal),
        "charged": fee_projection(receipt.charged),
        "charged_before": fee_projection(receipt.charged_before),
        "charged_after": fee_projection(receipt.charged_after),
        "deliver_qty": receipt.deliver_qty.to_string(),
        "deliver_cash_cents": receipt.deliver_cash.cents().to_string(),
    }))
}

/// Captures the committed P9 seam verbatim enough to construct reviewed
/// surface projections later. This is diagnostic source evidence only: it
/// deliberately carries no legacy values, mappings, or acceptance status.
fn commit_evidence_projection(tick: u64, evidence: &TickCommitEvidence) -> Value {
    normalized(json!({
        "tick": tick.to_string(),
        "next_receipt_index": evidence.next_receipt_index().to_string(),
        "p0_receipt_count": evidence.p0_receipts().len().to_string(),
        "receipts": evidence
            .receipts()
            .iter()
            .map(receipt_projection)
            .collect::<Vec<_>>(),
        "envelope_chains": evidence
            .envelope_chains()
            .iter()
            .map(|chain| json!({
                "envelope": chain.envelope(),
                "receipts": chain
                    .receipts()
                    .iter()
                    .map(receipt_projection)
                    .collect::<Vec<_>>(),
                "terminal": chain.is_terminal(),
            }))
            .collect::<Vec<_>>(),
        "finalizers": evidence
            .b2_finalizers()
            .iter()
            .map(|execution| json!({
                "stock": execution.stock().0,
                "auction_tail_passes": execution.auction_tail_passes().to_string(),
                "auction_completion_passes": execution.auction_completion_passes().to_string(),
                "day_end_passes": execution.day_end_passes().to_string(),
            }))
            .collect::<Vec<_>>(),
    }))
}

fn retain_chains(
    accumulated: &mut BTreeMap<EnvelopeKey, AccumulatedChain>,
    evidence: &TickCommitEvidence,
) {
    for chain in evidence.envelope_chains() {
        let key = chain.envelope().key().clone();
        let retained = accumulated.entry(key).or_insert_with(|| AccumulatedChain {
            envelope: chain.envelope().clone(),
            receipts: Vec::new(),
        });
        retained.envelope = chain.envelope().clone();
        retained.receipts.extend(chain.receipts().iter().cloned());
    }
}

fn controlled_candidate(
    request: &Request,
    frames: &[TickFrame],
    accumulated: &BTreeMap<EnvelopeKey, AccumulatedChain>,
    save: &engine::SaveSlot,
) -> Result<Option<ControlledCandidate>, String> {
    let Some(hints) = &request.historical_surface_hints else {
        return Ok(None);
    };
    let Some(hint) = &hints.controlled_sell else {
        return Ok(None);
    };
    let hint_account = AccountId(
        hint.account
            .parse::<u64>()
            .map_err(|error| format!("controlled seller account is not an exact u64: {error}"))?,
    );
    let ready = accumulated
        .values()
        .filter(|chain| {
            chain.envelope.key().account == hint_account
                && chain.envelope.key().stock == hint.stock
                && chain.envelope.key().side == Side::Sell
                && chain.envelope.live().shares > 0
                && chain
                    .receipts
                    .iter()
                    .any(|receipt| receipt.kind == ReceiptKind::Rollover)
                && chain
                    .receipts
                    .iter()
                    .filter(|receipt| receipt.kind == ReceiptKind::Fill)
                    .count()
                    >= 2
        })
        .collect::<Vec<_>>();
    if ready.is_empty() {
        return Ok(None);
    }
    if ready.len() != 1 {
        return Err("controlled surface hint matched multiple ready Sell envelopes".into());
    }
    let chain = ready[0];
    let account = save
        .snapshot
        .accounts
        .get(&hint_account)
        .ok_or("controlled seller account missing after commit")?;
    let position = account
        .positions
        .get(&hint.stock)
        .ok_or("controlled seller position missing after commit")?;
    let submission_cash = request
        .initial_accounts
        .get(&hint_account)
        .ok_or("controlled seller account missing at submission")?
        .cash;
    let legacy_reservation = hint
        .legacy_sell_reservation_cents
        .parse::<i64>()
        .map_err(|error| format!("legacy Sell reservation is not an exact i64: {error}"))?;
    if legacy_reservation < 0 {
        return Err("legacy Sell reservation cannot be negative".into());
    }
    Ok(Some(ControlledCandidate {
        update_count: frames.len(),
        envelope: chain.envelope.clone(),
        receipts: chain.receipts.clone(),
        account_after: account.clone(),
        submission_cash,
        legacy_sell_reservation: Money::from_cents(legacy_reservation),
        cost_basis: SellerCostBasisInput {
            invested_cents: position.invested_cents,
            recovered_cents: position.recovered_cents,
        },
        sealed_exogenous_script: semantic_bytes(&request.sealed_exogenous_script)?,
        strategy_state: semantic_bytes(&save.runtime_v2.strategy_states)?,
        plan_state: semantic_bytes(&json!({
            "plans": &save.plans,
            "pending_plan_events": &save.pending_plan_events,
        }))?,
        pending_intents: semantic_bytes(&save.pending_player)?,
        restore_order: semantic_bytes(&json!({
            "auction_orders": &save.auction_orders,
            "resting_orders": &save.resting_orders,
        }))?,
        rng_cursor: save.rng_state,
    }))
}

fn controlled_save_candidate(
    request: &Request,
    frames: &[TickFrame],
    accumulated: &BTreeMap<EnvelopeKey, AccumulatedChain>,
    save: &engine::SaveSlot,
    session: &mut ProtocolSession,
) -> Result<Option<ControlledSaveCandidate>, String> {
    let Some(hint) = request
        .historical_surface_hints
        .as_ref()
        .and_then(|hints| hints.controlled_sell.as_ref())
    else {
        return Ok(None);
    };
    let account = AccountId(
        hint.account
            .parse::<u64>()
            .map_err(|error| format!("controlled seller account is not an exact u64: {error}"))?,
    );
    let ready = accumulated
        .values()
        .filter(|chain| {
            chain.envelope.key().account == account
                && chain.envelope.key().stock == hint.stock
                && chain.envelope.key().side == Side::Sell
                && chain.envelope.live().shares > 0
                && chain
                    .receipts
                    .iter()
                    .any(|receipt| receipt.kind == ReceiptKind::Rollover)
                && chain
                    .receipts
                    .iter()
                    .filter(|receipt| receipt.kind == ReceiptKind::Fill)
                    .count()
                    == 1
        })
        .collect::<Vec<_>>();
    if ready.is_empty() {
        return Ok(None);
    }
    if ready.len() != 1 {
        return Err("controlled save surface matched multiple one-fill live Sell envelopes".into());
    }
    let envelope_key = ready[0].envelope.key().clone();
    let save_bytes = serde_json::to_vec(save).map_err(|error| error.to_string())?;
    let restored = ProtocolSession::restore(save).map_err(|error| error.to_string())?;
    let restored_resave_bytes =
        serde_json::to_vec(&restored.game().save().map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let uninterrupted_authority = serde_json::to_vec(
        &session
            .game()
            .business_state_hash()
            .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let restored_authority = serde_json::to_vec(
        &restored
            .game()
            .business_state_hash()
            .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    if uninterrupted_authority != restored_authority {
        return Err("public restored authority differs from uninterrupted live session".into());
    }
    let continuation = SaveRestoreContinuationCommand {
        account,
        intent: Intent::Cancel {
            code: hint.stock.clone(),
            id: envelope_key.order,
        },
    };
    let continuation_input =
        serde_json::to_vec(&continuation).map_err(|error| error.to_string())?;
    let checkpoint = session.checkpoint().map_err(|error| error.to_string())?;
    session
        .enqueue_player_intent(continuation.account, continuation.intent.clone())
        .map_err(|error| error.to_string())?;
    let uninterrupted_frame = session.step_frame().map_err(|error| error.to_string())?;
    let uninterrupted_save =
        serde_json::to_vec(&session.game().save().map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let uninterrupted_continuation =
        serde_json::to_vec(&(&uninterrupted_frame, &uninterrupted_save))
            .map_err(|error| error.to_string())?;
    session.rollback(checkpoint);
    let mut restored_continuation_session =
        ProtocolSession::restore(save).map_err(|error| error.to_string())?;
    restored_continuation_session
        .enqueue_player_intent(continuation.account, continuation.intent)
        .map_err(|error| error.to_string())?;
    let restored_frame = restored_continuation_session
        .step_frame()
        .map_err(|error| error.to_string())?;
    let restored_save = serde_json::to_vec(
        &restored_continuation_session
            .game()
            .save()
            .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let restored_continuation = serde_json::to_vec(&(&restored_frame, &restored_save))
        .map_err(|error| error.to_string())?;
    Ok(Some(ControlledSaveCandidate {
        update_count: frames.len(),
        envelope_key,
        save_bytes,
        restored_resave_bytes,
        uninterrupted_authority,
        restored_authority,
        continuation_input,
        uninterrupted_continuation,
        restored_continuation,
        continuation_frame: restored_frame,
        sealed_exogenous_script: semantic_bytes(&request.sealed_exogenous_script)?,
        strategy_state: semantic_bytes(&save.runtime_v2.strategy_states)?,
        plan_state: semantic_bytes(&json!({
            "plans": &save.plans,
            "pending_plan_events": &save.pending_plan_events,
        }))?,
        pending_intents: semantic_bytes(&save.pending_player)?,
        restore_order: semantic_bytes(&json!({
            "auction_orders": &save.auction_orders,
            "resting_orders": &save.resting_orders,
        }))?,
        rng_cursor: save.rng_state,
    }))
}

fn controlled_projections(
    request: &Request,
    frames: &[TickFrame],
    candidate: Option<&ControlledCandidate>,
    save_candidate: Option<&ControlledSaveCandidate>,
) -> Result<(Vec<Value>, Vec<Value>), String> {
    if request.scenario != "representation" {
        return Ok((Vec::new(), Vec::new()));
    }
    let Some(candidate) = candidate else {
        return Ok((
            Vec::new(),
            vec![json!({
                "code": "CONTROLLED_SURFACE_EVIDENCE_INCOMPLETE",
                "surfaces": ["auction-rollover", "cross-tick-partial-fill"],
                "detail": "no hinted live Sell envelope reached Rollover plus two cross-tick Fill receipts",
            })],
        ));
    };
    let updates = frames[..candidate.update_count]
        .iter()
        .map(RuntimeUpdateRef::Tick)
        .collect::<Vec<_>>();
    let continuation = ControlledContinuationBytes {
        sealed_exogenous_script: &candidate.sealed_exogenous_script,
        strategy_state: &candidate.strategy_state,
        plan_state: &candidate.plan_state,
        pending_intents: &candidate.pending_intents,
        restore_order: &candidate.restore_order,
        rng_cursor: candidate.rng_cursor,
    };
    let input = ControlledSellInput {
        envelope: &candidate.envelope,
        receipts: &candidate.receipts,
        trade_role: TradeRole::MakerSell,
        submission_cash: candidate.submission_cash,
        account_after: &candidate.account_after,
        legacy_sell_reservation: candidate.legacy_sell_reservation,
        cost_basis: candidate.cost_basis,
        feedback: FeedbackAuditInput::default(),
        continuation,
    };
    let mut projections = Vec::new();
    for (name, surface) in [
        ("auction-rollover", ControlledSellSurface::AuctionRollover),
        (
            "cross-tick-partial-fill",
            ControlledSellSurface::CrossTickPartialFill,
        ),
    ] {
        projections.push(
            serde_json::to_value(
                project_controlled_sell_corpus(
                    &format!("{}-{}-{name}", request.scenario, request.seed),
                    &request.scenario,
                    request
                        .seed
                        .parse::<u64>()
                        .map_err(|error| error.to_string())?,
                    &updates,
                    surface,
                    input.clone(),
                    &Sha256,
                )
                .map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())?,
        );
    }
    let Some(save_candidate) = save_candidate else {
        return Ok((
            projections,
            vec![json!({
                "code": "CONTROLLED_SAVE_SURFACE_EVIDENCE_INCOMPLETE",
                "surfaces": ["save-restore-live-order"],
                "detail": "no hinted live Sell envelope reached Rollover plus exactly one cross-tick Fill receipt",
            })],
        ));
    };
    let mut save_updates = frames[..save_candidate.update_count]
        .iter()
        .map(RuntimeUpdateRef::Tick)
        .collect::<Vec<_>>();
    save_updates.push(RuntimeUpdateRef::Tick(&save_candidate.continuation_frame));
    let save_input = SaveRestoreLiveOrderInput {
        saved_bytes: &save_candidate.save_bytes,
        restored_resave_bytes: &save_candidate.restored_resave_bytes,
        uninterrupted_authority: &save_candidate.uninterrupted_authority,
        restored_authority: &save_candidate.restored_authority,
        continuation_input: &save_candidate.continuation_input,
        uninterrupted_continuation: &save_candidate.uninterrupted_continuation,
        restored_continuation: &save_candidate.restored_continuation,
        expected: SaveLiveOrderIdentity {
            account: AccountId(hints_account(request)?),
            stock: &save_candidate.envelope_key.stock,
            order: save_candidate.envelope_key.order,
            side: Side::Sell,
        },
        legacy_sell_reservation: candidate.legacy_sell_reservation,
        control: ControlledContinuationBytes {
            sealed_exogenous_script: &save_candidate.sealed_exogenous_script,
            strategy_state: &save_candidate.strategy_state,
            plan_state: &save_candidate.plan_state,
            pending_intents: &save_candidate.pending_intents,
            restore_order: &save_candidate.restore_order,
            rng_cursor: save_candidate.rng_cursor,
        },
        sha256: &Sha256,
    };
    projections.push(
        serde_json::to_value(
            project_corpus_surface(
                &format!(
                    "{}-{}-save-restore-live-order",
                    request.scenario, request.seed
                ),
                &request.scenario,
                request
                    .seed
                    .parse::<u64>()
                    .map_err(|error| error.to_string())?,
                &save_updates,
                CorpusSurfaceInput::SaveRestoreLiveOrder(save_input),
            )
            .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?,
    );
    Ok((projections, Vec::new()))
}

/// Project the buyer-side surfaces from the exact production envelope chains
/// captured by the replay.  These are intentionally discovered from commit
/// evidence, never reconstructed from the sealed stream: the envelope and
/// receipt objects are the authoritative current-side witness.
fn equivalence_projections(
    request: &Request,
    frames: &[TickFrame],
    accumulated: &BTreeMap<EnvelopeKey, AccumulatedChain>,
    initial_accounts: &BTreeMap<AccountId, AccountSnap>,
    final_save: &engine::SaveSlot,
) -> Result<Vec<Value>, String> {
    if request.scenario != "equivalence" {
        return Ok(Vec::new());
    }
    let updates = frames
        .iter()
        .map(RuntimeUpdateRef::Tick)
        .collect::<Vec<_>>();
    let seed = request
        .seed
        .parse::<u64>()
        .map_err(|error| error.to_string())?;
    let mut projections = Vec::new();
    let buys = accumulated
        .values()
        .filter(|chain| chain.envelope.key().side == Side::Buy && !chain.receipts.is_empty())
        .collect::<Vec<_>>();
    let Some(first_buy) = buys.first() else {
        return Ok(projections);
    };
    let buy_chains = buys
        .iter()
        .map(|chain| EnvelopeChainInput {
            envelope: &chain.envelope,
            receipts: &chain.receipts,
        })
        .collect::<Vec<_>>();
    let input = |surface_name: &str, surface| -> Result<Value, String> {
        let projection = project_corpus_surface(
            &format!("{}-{}-{surface_name}", request.scenario, request.seed),
            &request.scenario,
            seed,
            &updates,
            surface,
        )
        .map_err(|error| error.to_string())?;
        serde_json::to_value(projection).map_err(|error| error.to_string())
    };
    projections.push(input(
        "buyer-fees",
        CorpusSurfaceInput::BuyerFees {
            chains: &buy_chains,
            trade_role: TradeRole::TakerBuy,
        },
    )?);
    let stock = first_buy.envelope.key().stock.clone();
    let account = first_buy.envelope.key().account;
    let before = initial_accounts
        .get(&account)
        .and_then(|snap| snap.positions.get(&stock))
        .ok_or("buyer-side T1 witness lacks initial position")?;
    let after = final_save
        .snapshot
        .accounts
        .get(&account)
        .and_then(|snap| snap.positions.get(&stock))
        .ok_or("buyer-side T1 witness lacks terminal position")?;
    projections.push(input(
        "t1",
        CorpusSurfaceInput::T1 {
            chains: &buy_chains,
            trade_role: TradeRole::TakerBuy,
            before,
            after,
        },
    )?);
    let continuous = buys
        .iter()
        .copied()
        .find(|chain| {
            chain.envelope.live().shares == 0
                && chain
                    .receipts
                    .iter()
                    .filter(|receipt| receipt.kind == ReceiptKind::Fill)
                    .count()
                    == 1
        })
        .ok_or("buyer-side continuous witness lacks a terminal one-leg Buy envelope")?;
    projections.push(input(
        "continuous-buy-leg",
        CorpusSurfaceInput::ContinuousBuyLeg {
            envelope: &continuous.envelope,
            receipts: &continuous.receipts,
            trade_role: TradeRole::TakerBuy,
        },
    )?);
    Ok(projections)
}

fn initialized_session(request: &Request) -> Result<ProtocolSession, String> {
    session_with_accounts(
        request,
        request.setup.clone(),
        request.initial_accounts.clone(),
    )
}

fn session_with_accounts(
    request: &Request,
    setup: SessionSetup,
    accounts: BTreeMap<AccountId, AccountSnap>,
) -> Result<ProtocolSession, String> {
    let seed = request
        .seed
        .parse::<u64>()
        .map_err(|error| error.to_string())?;
    let fresh = ProtocolSession::new(setup, seed).map_err(|error| error.to_string())?;
    let mut initial = fresh.game().save().map_err(|error| error.to_string())?;
    initial.snapshot.accounts = accounts;
    for attention in initial.npc_attention.values_mut() {
        attention.next_attention_candidate_tick = 100;
    }
    ProtocolSession::restore(&initial).map_err(|error| error.to_string())
}

fn session_with_resting_sell(
    request: &Request,
    setup: SessionSetup,
    accounts: BTreeMap<AccountId, AccountSnap>,
    seller: AccountId,
    stock: &StockCode,
    price: Money,
    qty: u32,
) -> Result<ProtocolSession, String> {
    let seed = request
        .seed
        .parse::<u64>()
        .map_err(|error| error.to_string())?;
    let fresh = ProtocolSession::new(setup, seed).map_err(|error| error.to_string())?;
    let mut initial = fresh.game().save().map_err(|error| error.to_string())?;
    initial.snapshot.accounts = accounts;
    for attention in initial.npc_attention.values_mut() {
        attention.next_attention_candidate_tick = 100;
    }
    let mut seeded = ProtocolSession::restore(&initial).map_err(|error| error.to_string())?;
    for _ in 0..4 {
        seeded.step_frame().map_err(|error| error.to_string())?;
    }
    initial = seeded.game().save().map_err(|error| error.to_string())?;

    let order = Order {
        id: OrderId(1),
        side: Side::Sell,
        price,
        qty,
        original_qty: qty,
        filled_qty: 0,
        filled_value: Money::ZERO,
        owner: seller,
        seq: 1,
    };
    initial.resting_orders.insert(stock.clone(), vec![order]);
    let market = initial
        .snapshot
        .markets
        .get_mut(stock)
        .ok_or_else(|| format!("resting Sell stock is absent from snapshot: {}", stock.0))?;
    market.asks = vec![(price, u64::from(qty))];
    market.best_ask = Some(price);
    initial.next_order_id = 2;

    let seller_account = initial
        .snapshot
        .accounts
        .get_mut(&seller)
        .ok_or_else(|| format!("resting Sell owner is absent from snapshot: {}", seller.0))?;
    seller_account.reserved_cash = Money::ZERO;
    seller_account.reserved_sell_qty.insert(stock.clone(), qty);
    initial.runtime_v2.live_envelopes = vec![LiveEnvelopeV2 {
        key: EnvelopeKeyV2 {
            account: seller,
            stock: stock.clone(),
            order: OrderId(1),
            side: Side::Sell,
        },
        live: ResourceV2 {
            cash: Money::ZERO,
            shares: qty,
        },
        audit: EnvelopeAuditV2 {
            limit: price,
            remaining_qty: qty,
            filled_qty: 0,
            filled_value: Money::ZERO,
            nominal: FeeComponentsV2::default(),
            charged: FeeComponentsV2::default(),
        },
    }];
    ProtocolSession::restore(&initial).map_err(|error| error.to_string())
}

fn legacy_hint_money(value: Option<&String>, field: &str) -> Result<Money, String> {
    let cents = value
        .ok_or_else(|| format!("{field} hint is missing"))?
        .parse::<i64>()
        .map_err(|error| format!("{field} hint is not an exact i64: {error}"))?;
    if cents <= 0 {
        return Err(format!("{field} hint must be positive"));
    }
    Ok(Money::from_cents(cents))
}

fn price_cage_projection(request: &Request) -> Result<Option<Value>, String> {
    if request.scenario != "equivalence" {
        return Ok(None);
    }
    let stock = request
        .setup
        .stocks
        .first()
        .ok_or("price-cage witness requires one configured stock")?;
    let account = AccountId(0);
    let mut session = initialized_session(request)?;
    // The sealed directed witness is at the first continuous tick. Advance the
    // fresh production session to the same phase without importing old state.
    for _ in 0..4 {
        session.step_frame().map_err(|error| error.to_string())?;
    }
    let before = session
        .game()
        .save()
        .map_err(|error| error.to_string())?
        .next_order_id;
    let outside_cents = stock
        .initial_price
        .cents()
        .checked_mul(110)
        .and_then(|value| value.checked_div(100))
        .ok_or("price-cage outside price overflow")?;
    for price in [Money::from_cents(outside_cents), stock.initial_price] {
        session
            .enqueue_player_intent(
                account,
                Intent::PlaceLimit {
                    code: stock.code.clone(),
                    side: Side::Buy,
                    price,
                    qty: request.setup.config.lot_size,
                },
            )
            .map_err(|error| error.to_string())?;
    }
    let frame = session.step_frame().map_err(|error| error.to_string())?;
    let inside_order = frame
        .events
        .iter()
        .find_map(|event| match event {
            Event::OrderAccepted {
                account: event_account,
                code,
                id,
                side: Side::Buy,
                ..
            } if *event_account == account && code == &stock.code => Some(*id),
            _ => None,
        })
        .ok_or("price-cage current witness lacks the inside Buy acceptance")?;
    let after = session
        .game()
        .save()
        .map_err(|error| error.to_string())?
        .next_order_id;
    let projection = project_corpus_surface(
        &format!("{}-{}-price-cage", request.scenario, request.seed),
        &request.scenario,
        request
            .seed
            .parse::<u64>()
            .map_err(|error| error.to_string())?,
        &[RuntimeUpdateRef::Tick(&frame)],
        CorpusSurfaceInput::PriceCage {
            account,
            stock: &stock.code,
            inside_order,
            next_order_id_before: OrderId(before),
            next_order_id_after: OrderId(after),
        },
    )
    .map_err(|error| error.to_string())?;
    Ok(Some(
        serde_json::to_value(projection).map_err(|error| error.to_string())?,
    ))
}

fn acceptance_flip_projection(request: &Request) -> Result<Option<Value>, String> {
    if request.scenario != "divergence-9" {
        return Ok(None);
    }
    let hints = request
        .historical_surface_hints
        .as_ref()
        .ok_or("divergence-9 historical surface hints are missing")?;
    let legacy_reservation = legacy_hint_money(
        hints.acceptance_flip_legacy_sell_reservation_cents.as_ref(),
        "acceptance-flip legacy Sell reservation",
    )?;
    let stock = request
        .setup
        .stocks
        .first()
        .ok_or("acceptance-flip witness requires one configured stock")?;
    let mut setup = request.setup.clone();
    setup.stocks[0].initial_price = Money::from_cents(100);
    let account = AccountId(0);
    let mut zero_cash = request
        .initial_accounts
        .get(&account)
        .cloned()
        .ok_or("acceptance-flip player account is missing")?;
    zero_cash.cash = Money::ZERO;
    zero_cash.reserved_cash = Money::ZERO;
    zero_cash.reserved_sell_qty.clear();
    let mut session =
        session_with_accounts(request, setup, BTreeMap::from([(account, zero_cash)]))?;
    session
        .enqueue_player_intent(
            account,
            Intent::PlaceLimit {
                code: stock.code.clone(),
                side: Side::Sell,
                price: Money::from_cents(100),
                qty: request.setup.config.lot_size,
            },
        )
        .map_err(|error| error.to_string())?;
    let frame = session.step_frame().map_err(|error| error.to_string())?;
    let save = session.game().save().map_err(|error| error.to_string())?;
    let account_after = save
        .snapshot
        .accounts
        .get(&account)
        .ok_or("acceptance-flip committed account is missing")?;
    let projection = project_corpus_surface(
        &format!("{}-{}-acceptance-flip", request.scenario, request.seed),
        &request.scenario,
        request
            .seed
            .parse::<u64>()
            .map_err(|error| error.to_string())?,
        &[RuntimeUpdateRef::Tick(&frame)],
        CorpusSurfaceInput::AcceptanceFlip {
            account,
            stock: &stock.code,
            trade_role: TradeRole::MakerSell,
            submission_cash: Money::ZERO,
            account_after,
            legacy_sell_reservation: legacy_reservation,
            feedback: FeedbackAuditInput::default(),
        },
    )
    .map_err(|error| error.to_string())?;
    Ok(Some(
        serde_json::to_value(projection).map_err(|error| error.to_string())?,
    ))
}

fn three_leg_projection(request: &Request) -> Result<Option<Value>, String> {
    if request.scenario != "divergence-9" {
        return Ok(None);
    }
    let hints = request
        .historical_surface_hints
        .as_ref()
        .ok_or("divergence-9 historical surface hints are missing")?;
    let legacy_reservation = legacy_hint_money(
        hints.three_leg_legacy_sell_reservation_cents.as_ref(),
        "three-leg legacy Sell reservation",
    )?;
    let stock = request
        .setup
        .stocks
        .first()
        .ok_or("three-leg witness requires one configured stock")?;
    let buyer = AccountId(0);
    let seller = AccountId(1);
    let buyer_account = AccountSnap {
        cash: Money::from_cents(10_000_000),
        positions: BTreeMap::new(),
        reserved_cash: Money::ZERO,
        reserved_sell_qty: BTreeMap::new(),
    };
    let seller_account = AccountSnap {
        cash: Money::from_cents(1_481_061),
        positions: BTreeMap::from([(
            stock.code.clone(),
            PositionSnap {
                qty: 2_400,
                t1_locked: 0,
                invested_cents: 2_400,
                recovered_cents: 0,
            },
        )]),
        reserved_cash: Money::ZERO,
        reserved_sell_qty: BTreeMap::new(),
    };
    let mut setup = request.setup.clone();
    setup.npcs.inst_count = 1;
    let mut session = session_with_resting_sell(
        request,
        setup,
        BTreeMap::from([(buyer, buyer_account), (seller, seller_account)]),
        seller,
        &stock.code,
        Money::from_cents(1),
        1_200,
    )?;
    let mut accumulated = BTreeMap::new();
    let mut selected_frames = Vec::new();
    for input_tick in 0..3u64 {
        let buy_qty = match input_tick {
            0 | 1 => Some(100),
            2 => Some(1_000),
            _ => unreachable!("three-leg witness has exactly three input ticks"),
        };
        if let Some(qty) = buy_qty {
            session
                .enqueue_player_intent(
                    buyer,
                    Intent::PlaceLimit {
                        code: stock.code.clone(),
                        side: Side::Buy,
                        price: Money::from_cents(1),
                        qty,
                    },
                )
                .map_err(|error| error.to_string())?;
        }
        let (frame, evidence) = session
            .step_frame_with_commit_evidence()
            .map_err(|error| error.to_string())?;
        retain_chains(&mut accumulated, &evidence);
        selected_frames.push(frame);
    }
    let chain = accumulated
        .values()
        .find(|chain| {
            chain.envelope.key().account == seller
                && chain.envelope.key().stock == stock.code
                && chain.envelope.key().side == Side::Sell
        })
        .ok_or("three-leg current witness lacks the seller envelope")?;
    let save = session.game().save().map_err(|error| error.to_string())?;
    let account_after = save
        .snapshot
        .accounts
        .get(&seller)
        .ok_or("three-leg committed seller account is missing")?;
    let updates = selected_frames
        .iter()
        .map(RuntimeUpdateRef::Tick)
        .collect::<Vec<_>>();
    let projection = project_corpus_surface(
        &format!(
            "{}-{}-three-leg-fee-catchup",
            request.scenario, request.seed
        ),
        &request.scenario,
        request
            .seed
            .parse::<u64>()
            .map_err(|error| error.to_string())?,
        &updates,
        CorpusSurfaceInput::ThreeLegFeeCatchup {
            envelope: &chain.envelope,
            receipts: &chain.receipts,
            trade_role: TradeRole::MakerSell,
            account_after,
            legacy_sell_reservation: legacy_reservation,
            feedback: FeedbackAuditInput::default(),
        },
    )
    .map_err(|error| error.to_string())?;
    Ok(Some(
        serde_json::to_value(projection).map_err(|error| error.to_string())?,
    ))
}

fn hints_account(request: &Request) -> Result<u64, String> {
    request
        .historical_surface_hints
        .as_ref()
        .ok_or("controlled surface hint missing")?
        .controlled_sell
        .as_ref()
        .ok_or("controlled Sell hint missing")?
        .account
        .parse::<u64>()
        .map_err(|error| format!("controlled seller account is not an exact u64: {error}"))
}

fn execute(request: Request) -> Result<Value, String> {
    if request.schema != "escrow-current-corpus-request-v1" {
        return Err("unsupported corpus replay request".into());
    }
    if request.setup.simulation_policy_id != SIMULATION_POLICY_ID_V2 {
        return Err(
            "current replay requires explicit v2 setup; legacy save migration is forbidden".into(),
        );
    }
    if request.setup.npcs.retail_count != 0
        || request.setup.npcs.inst_count != 0
        || request.setup.npcs.hot_count != 0
        || request.initial_accounts.len() != 1
        || !request.initial_accounts.contains_key(&AccountId(0))
    {
        return Err("controlled primary replay supports only the sealed no-NPC/player script; other surfaces need their own actual witnesses".into());
    }
    if request.ticks == 0 || request.ticks > 200 {
        return Err("controlled replay tick bound must be 1..200".into());
    }
    if request
        .sealed_exogenous_script
        .windows(2)
        .any(|pair| pair[0].0 >= pair[1].0)
        || request
            .sealed_exogenous_script
            .iter()
            .any(|(tick, _)| *tick >= request.ticks)
        || request
            .restore_at_ticks
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
    {
        return Err(
            "script or restore checkpoints are duplicated, out of order or outside the run".into(),
        );
    }
    let seed = request
        .seed
        .parse::<u64>()
        .map_err(|error| error.to_string())?;
    let fresh =
        ProtocolSession::new(request.setup.clone(), seed).map_err(|error| error.to_string())?;
    let mut initial = fresh.game().save().map_err(|error| error.to_string())?;
    // Explicit initial allocations from the sealed scenario, not legacy slot
    // deserialization. New company/RNG/order state is constructed independently.
    initial.snapshot.accounts = request.initial_accounts.clone();
    let mut session = ProtocolSession::restore(&initial).map_err(|error| error.to_string())?;
    let initial_state = shared_state(&session)?;
    let mut updates = Vec::new();
    let mut checkpoints = Vec::new();
    let mut restore_checkpoints = Vec::new();
    let mut conservation = Vec::new();
    let mut commit_evidence = Vec::new();
    let mut frames = Vec::new();
    let mut accumulated_chains = BTreeMap::new();
    let mut first_buy_commit_save = None;
    let mut controlled_candidate = None;
    let mut controlled_save_candidate = None;
    let mut projector = UpdateStreamProjector::new();
    for input_tick in 0..request.ticks {
        if let Some((_, intents)) = request
            .sealed_exogenous_script
            .iter()
            .find(|(tick, _)| *tick == input_tick)
        {
            for intent in intents {
                session
                    .enqueue_player_intent(AccountId(0), intent.clone())
                    .map_err(|error| error.to_string())?;
            }
        }
        let (frame, evidence) = session
            .step_frame_with_commit_evidence()
            .map_err(|error| format!("replay tick {input_tick}: {error}"))?;
        let save = session.game().save().map_err(|error| error.to_string())?;
        if first_buy_commit_save.is_none()
            && evidence.envelope_chains().iter().any(|chain| {
                chain.envelope().key().side == Side::Buy
                    && chain
                        .receipts()
                        .iter()
                        .any(|receipt| receipt.kind == ReceiptKind::Fill)
            })
        {
            first_buy_commit_save = Some(save.clone());
        }
        let chains = evidence
            .envelope_chains()
            .iter()
            .map(|chain| EnvelopeChainInput {
                envelope: chain.envelope(),
                receipts: chain.receipts(),
            })
            .collect::<Vec<_>>();
        conservation.push(
            project_conservation_snapshot(
                &request.scenario,
                seed,
                frame.tick,
                &chains,
                &save.snapshot.accounts,
            )
            .map_err(|error| error.to_string())?,
        );
        commit_evidence.push(commit_evidence_projection(frame.tick, &evidence));
        retain_chains(&mut accumulated_chains, &evidence);
        updates.push(
            projector
                .project_update(RuntimeUpdateRef::Tick(&frame))
                .map_err(|error| error.to_string())?,
        );
        frames.push(frame.clone());
        let mut snapshot =
            serde_json::to_value(&save.snapshot).map_err(|error| error.to_string())?;
        snapshot
            .as_object_mut()
            .ok_or("snapshot object missing")?
            .remove("seq");
        checkpoints.push(normalized(json!({"kind":"TickFrame", "tick":frame.tick, "snapshot": snapshot,
            "orders":{"resting":save.resting_orders,"auction":save.auction_orders},
            "rng_state":save.rng_state.to_string(), "next_order_id":save.next_order_id.to_string(),
            "plans":save.plans, "pending_plan_events":save.pending_plan_events, "pending_player":save.pending_player,
            "strategy_profiles":session.game().account_strategy_profiles(),
        })));
        if controlled_save_candidate.is_none() {
            controlled_save_candidate = self::controlled_save_candidate(
                &request,
                &frames,
                &accumulated_chains,
                &save,
                &mut session,
            )?;
        }
        if controlled_candidate.is_none() {
            controlled_candidate =
                self::controlled_candidate(&request, &frames, &accumulated_chains, &save)?;
        }
        if request.restore_at_ticks.contains(&frame.tick) {
            let before = shared_state(&session)?;
            let restored = ProtocolSession::restore(&save).map_err(|error| error.to_string())?;
            let after = shared_state(&restored)?;
            if before != after {
                return Err(format!("restore state differs at {}", frame.tick));
            }
            restore_checkpoints
                .push(json!({"kind":"before_save","tick":frame.tick.to_string(),"state":before}));
            restore_checkpoints
                .push(json!({"kind":"after_restore","tick":frame.tick.to_string(),"state":after}));
            session = restored;
        }
    }
    if restore_checkpoints.len() != request.restore_at_ticks.len() * 2 {
        return Err("a restore checkpoint was never reached".into());
    }
    let primary_state = json!({
        "initial": initial_state,
        "checkpoints": checkpoints,
        "restore_checkpoints": restore_checkpoints,
        "terminal": shared_state(&session)?,
        "sealed_exogenous_script": normalized(
            serde_json::to_value(&request.sealed_exogenous_script)
                .map_err(|error| error.to_string())?
        ),
    });
    let final_save = session.game().save().map_err(|error| error.to_string())?;
    let equivalence_save = first_buy_commit_save.as_ref().unwrap_or(&final_save);
    let mut equivalence_candidates = equivalence_projections(
        &request,
        &frames,
        &accumulated_chains,
        &request.initial_accounts,
        equivalence_save,
    )?;
    if let Some(price_cage) = price_cage_projection(&request)? {
        equivalence_candidates.push(price_cage);
    }
    if let Some(acceptance_flip) = acceptance_flip_projection(&request)? {
        equivalence_candidates.push(acceptance_flip);
    }
    if let Some(three_leg) = three_leg_projection(&request)? {
        equivalence_candidates.push(three_leg);
    }
    let (mut surface_candidates, surface_blockers) = controlled_projections(
        &request,
        &frames,
        controlled_candidate.as_ref(),
        controlled_save_candidate.as_ref(),
    )?;
    surface_candidates.append(&mut equivalence_candidates);
    let full_updates = serde_json::to_value(&updates).map_err(|error| error.to_string())?;
    let mut surface_artifacts = Vec::new();
    for projection in &mut surface_candidates {
        let surface = projection["corpus_control"]["surface"]
            .as_str()
            .ok_or("surface projection name is not a string")?
            .to_owned();
        let auxiliary = matches!(
            surface.as_str(),
            "price-cage" | "acceptance-flip" | "three-leg-fee-catchup"
        );
        if !auxiliary {
            projection["updates"] = full_updates.clone();
        }
        if surface == "save-restore-live-order" {
            let evidence = projection
                .get_mut("state")
                .and_then(Value::as_object_mut)
                .and_then(|state| state.remove("save_restore_live_order_control"))
                .ok_or("save/restore surface lacks its validated artifact evidence")?;
            surface_artifacts.push(json!({ "surface": surface, "evidence": evidence }));
        }
        if auxiliary {
            continue;
        }
        projection
            .get_mut("state")
            .and_then(Value::as_object_mut)
            .ok_or("surface projection state is not an object")?
            .insert("legacy_checkpoints".into(), primary_state.clone());
    }
    Ok(
        json!({"schema":"escrow-current-corpus-run-v1", "scenario":request.scenario,"seed":request.seed,
        "updates":updates,"state":primary_state,
        "conservation":conservation,"commit_evidence":commit_evidence,
        "surface_candidates":surface_candidates,"surface_artifacts":surface_artifacts,
        "surface_blockers":surface_blockers,
        "historical_comparison_performed":false,"task9_acceptance":false}),
    )
}

fn run() -> Result<(), String> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let [input] = args.as_slice() else {
        return Err("usage: escrow_corpus_replay <current-request.json>".into());
    };
    let request = serde_json::from_slice(&fs::read(input).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    let result = execute(request)?;
    let stdout = io::stdout();
    let mut writer = stdout.lock();
    serde_json::to_writer(&mut writer, &result).map_err(|error| error.to_string())?;
    writeln!(writer).map_err(|error| error.to_string())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("current corpus replay FAIL: {error}");
        process::exit(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_preserves_large_identities_and_normalizes_integral_configuration_floats() {
        let projected =
            normalized(json!({"identity":u64::MAX,"fraction":0.25,"zero":0.0,"one":1.0}));
        assert_eq!(projected["identity"], u64::MAX.to_string());
        assert_eq!(projected["zero"], "0");
        assert_eq!(projected["one"], "1");
        assert_eq!(projected["fraction"], 0.25);
    }
}
