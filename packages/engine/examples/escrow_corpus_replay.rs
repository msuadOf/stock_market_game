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
    AccountId, AccountSnap, Intent, Money, SessionSetup, Side, StockCode, SIMULATION_POLICY_ID_V2,
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
    controlled_sell: ControlledSellHint,
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
    save_bytes: Vec<u8>,
    restored_resave_bytes: Vec<u8>,
    uninterrupted_authority: Vec<u8>,
    restored_authority: Vec<u8>,
    continuation_input: Vec<u8>,
    uninterrupted_continuation: Vec<u8>,
    restored_continuation: Vec<u8>,
    continuation_frame: TickFrame,
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
    session: &mut ProtocolSession,
) -> Result<Option<ControlledCandidate>, String> {
    let Some(hints) = &request.historical_surface_hints else {
        return Ok(None);
    };
    let hint = &hints.controlled_sell;
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
    let save_bytes = serde_json::to_vec(save).map_err(|error| error.to_string())?;
    let restored = ProtocolSession::restore(save).map_err(|error| error.to_string())?;
    let restored_resave_bytes = serde_json::to_vec(
        &restored.game().save().map_err(|error| error.to_string())?,
    )
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
        account: hint_account,
        intent: Intent::Cancel {
            code: hint.stock.clone(),
            id: chain.envelope.key().order,
        },
    };
    let continuation_input = serde_json::to_vec(&continuation).map_err(|error| error.to_string())?;
    // Branching starts from the exact freshly-produced public SaveSlot. One
    // branch is the uninterrupted continuation; the other crosses the public
    // decode/restore boundary before receiving the same cancellation command.
    let checkpoint = session.checkpoint().map_err(|error| error.to_string())?;
    let mut uninterrupted = session;
    uninterrupted
        .enqueue_player_intent(continuation.account, continuation.intent.clone())
        .map_err(|error| error.to_string())?;
    let uninterrupted_frame = uninterrupted.step_frame().map_err(|error| error.to_string())?;
    let uninterrupted_save = serde_json::to_vec(
        &uninterrupted.game().save().map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let uninterrupted_continuation = serde_json::to_vec(&(&uninterrupted_frame, &uninterrupted_save))
        .map_err(|error| error.to_string())?;
    uninterrupted.rollback(checkpoint).map_err(|error| error.to_string())?;
    let mut restored_continuation_session = ProtocolSession::restore(save).map_err(|error| error.to_string())?;
    restored_continuation_session
        .enqueue_player_intent(continuation.account, continuation.intent)
        .map_err(|error| error.to_string())?;
    let restored_frame = restored_continuation_session.step_frame().map_err(|error| error.to_string())?;
    let restored_save = serde_json::to_vec(
        &restored_continuation_session
            .game()
            .save()
            .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let restored_continuation = serde_json::to_vec(&(&restored_frame, &restored_save))
        .map_err(|error| error.to_string())?;
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
        save_bytes,
        restored_resave_bytes,
        uninterrupted_authority,
        restored_authority,
        continuation_input,
        uninterrupted_continuation,
        restored_continuation,
        continuation_frame: restored_frame,
    }))
}

fn controlled_projections(
    request: &Request,
    frames: &[TickFrame],
    candidate: Option<&ControlledCandidate>,
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
    let mut save_updates = frames[..candidate.update_count]
        .iter()
        .map(RuntimeUpdateRef::Tick)
        .collect::<Vec<_>>();
    save_updates.push(RuntimeUpdateRef::Tick(&candidate.continuation_frame));
    let save_input = SaveRestoreLiveOrderInput {
        saved_bytes: &candidate.save_bytes,
        restored_resave_bytes: &candidate.restored_resave_bytes,
        uninterrupted_authority: &candidate.uninterrupted_authority,
        restored_authority: &candidate.restored_authority,
        continuation_input: &candidate.continuation_input,
        uninterrupted_continuation: &candidate.uninterrupted_continuation,
        restored_continuation: &candidate.restored_continuation,
        expected: SaveLiveOrderIdentity {
            account: AccountId(hints_account(request)?),
            stock: &candidate.envelope.key().stock,
            order: candidate.envelope.key().order,
            side: Side::Sell,
        },
        legacy_sell_reservation: candidate.legacy_sell_reservation,
        sha256: &Sha256,
    };
    projections.push(serde_json::to_value(project_corpus_surface(
        &format!("{}-{}-save-restore-live-order", request.scenario, request.seed),
        &request.scenario,
        request.seed.parse::<u64>().map_err(|error| error.to_string())?,
        &save_updates,
        CorpusSurfaceInput::SaveRestoreLiveOrder(save_input),
    ).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?);
    Ok((projections, Vec::new()))
}

fn hints_account(request: &Request) -> Result<u64, String> {
    request
        .historical_surface_hints
        .as_ref()
        .ok_or("controlled surface hint missing")?
        .controlled_sell
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
    let mut controlled_candidate = None;
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
        if controlled_candidate.is_none() {
            controlled_candidate =
                self::controlled_candidate(&request, &frames, &accumulated_chains, &save, &mut session)?;
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
    let (mut surface_candidates, surface_blockers) =
        controlled_projections(&request, &frames, controlled_candidate.as_ref())?;
    let full_updates = serde_json::to_value(&updates).map_err(|error| error.to_string())?;
    for projection in &mut surface_candidates {
        if projection["corpus_control"]["surface"] != "save-restore-live-order" {
            projection["updates"] = full_updates.clone();
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
        "surface_candidates":surface_candidates,"surface_blockers":surface_blockers,
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
