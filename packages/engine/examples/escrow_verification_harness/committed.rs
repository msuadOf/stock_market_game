//! One real public-protocol execution, including its committed receipt journal.
use super::*;
use engine::{
    session::pipeline::{
        with_executor_perturbation, CanonicalMerge, ExecutorBoundary, ExecutorOrderRecord,
        ExecutorPermutation, ExecutorPerturbation, TickCommitEvidence,
    },
    verification_evidence::{
        project_conservation_snapshot, ConservationSnapshot, EnvelopeChainInput,
        FinalizerAuditInput, UpdateStreamProjector,
    },
    PositionSnap,
};

pub(super) struct RuntimeCapture {
    pub snapshots: Vec<Value>,
    pub updates: Vec<UpdateProjection>,
    pub final_save: SaveSlot,
    pub restore_slots: Vec<RestoreSlotCapture>,
    pub tick_from: u64,
    pub tick_to: u64,
    pub tick_frames: u64,
    pub civil_updates: u64,
    pub auction_completed: u64,
    pub day_boundaries: u64,
    pub receipts: Vec<Value>,
    pub conservation: Vec<ConservationSnapshot>,
    pub finalizers: Vec<FinalizerAuditInput>,
    pub executor_records: Vec<ExecutorOrderRecord>,
}

#[derive(Default)]
struct Accumulator {
    snapshots: Vec<Value>,
    updates: Vec<UpdateProjection>,
    projector: UpdateStreamProjector,
    receipts: Vec<Value>,
    conservation: Vec<ConservationSnapshot>,
    finalizers: Vec<FinalizerAuditInput>,
    tick_from: Option<u64>,
    tick_to: u64,
    tick_frames: u64,
    civil_updates: u64,
    auction_completed: u64,
    day_boundaries: u64,
}

fn count_events(events: &[Event], predicate: impl Fn(&Event) -> bool) -> Result<u64, String> {
    u64::try_from(events.iter().filter(|event| predicate(event)).count())
        .map_err(|error| format!("event count overflow: {error}"))
}

pub(super) fn execute(config: &Config) -> Result<CaptureBundle, String> {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(config.budget.thread_count()?)
        .thread_name(|index| format!("escrow-evidence-{index}"))
        .build()
        .map_err(|error| format!("cannot build rayon pool: {error}"))?;
    let permutation = if config.mode == Mode::Canonical {
        ExecutorPermutation::Canonical
    } else {
        ExecutorPermutation::Reverse
    };
    let policy = ExecutorPerturbation {
        account_shards: permutation,
        stock_shards: permutation,
        // Delivery and dispatch are perturbed independently of trade identity.
        worker_results: permutation,
        disable_merge: config.disabled_merge.map(|dimension| match dimension {
            MergeDimension::Completion => CanonicalMerge::Completion,
        }),
    };
    if config.mode == Mode::NegativeControl {
        return pool.install(|| negative_control(config, policy, pool.current_num_threads()));
    }
    let (capture, records) = pool.install(|| {
        with_executor_perturbation(policy, || capture_runtime(config.seed))
            .map_err(|error| error.to_string())
    })?;
    let mut capture = capture?;
    capture.executor_records = records;
    super::assemble(config, pool.current_num_threads(), capture)
}

fn negative_control(
    config: &Config,
    policy: ExecutorPerturbation,
    threads: usize,
) -> Result<CaptureBundle, String> {
    let mut session = initial_session(config.seed)?;
    let mut capture = Accumulator::default();
    let mut records = Vec::new();
    for _ in 0..13 {
        let before = session.game().save().map_err(|error| error.to_string())?;
        let before_bytes = serde_json::to_vec(&before).map_err(|error| error.to_string())?;
        let (result, tick_records) =
            with_executor_perturbation(policy, || step_scripted(&mut session))
                .map_err(|error| error.to_string())?;
        records.extend(tick_records.iter().cloned());
        match result {
            Ok(frame) => capture.frame(config.seed, frame, &session)?,
            Err(error) => {
                // The same public operation must pass with ONLY the disabled
                // merge restored. This excludes unrelated fixture/runtime errors.
                let mut control =
                    ProtocolSession::restore(&before).map_err(|error| error.to_string())?;
                let control_policy = ExecutorPerturbation {
                    disable_merge: None,
                    ..policy
                };
                let (control_result, _) =
                    with_executor_perturbation(control_policy, || step_scripted(&mut control))
                        .map_err(|error| error.to_string())?;
                control_result.map_err(|control_error| format!("negative control also fails with merge enabled: {control_error}; disabled error: {error}"))?;
                // Scripted enqueueing precedes the step checkpoint. Compare the
                // post-enqueue authority, not the pre-enqueue SaveSlot.
                let after = session.game().save().map_err(|error| error.to_string())?;
                let mut expected =
                    ProtocolSession::restore(&before).map_err(|error| error.to_string())?;
                enqueue_script(&mut expected)?;
                let expected_bytes =
                    serde_json::to_vec(&expected.game().save().map_err(|error| error.to_string())?)
                        .map_err(|error| error.to_string())?;
                let after_bytes = serde_json::to_vec(&after).map_err(|error| error.to_string())?;
                if expected_bytes != after_bytes {
                    return Err(format!("disabled merge leaked authority: {error}"));
                }
                let target = match config
                    .disabled_merge
                    .ok_or("negative control missing dimension")?
                {
                    MergeDimension::Completion => vec![
                        ExecutorBoundary::P3WorkerResults,
                        ExecutorBoundary::P5ReceiptResults,
                    ],
                };
                if !tick_records.iter().any(|record| {
                    target.contains(&record.boundary)
                        && record.identities.len() >= 2
                        && record.item_counts.iter().all(|count| *count > 0)
                }) {
                    return Err(format!(
                        "disabled merge failure lacks actual nonempty affected boundary: {error}"
                    ));
                }
                let authoritative_state = serde_json::to_vec(&serde_json::json!({
                    "committed_prefix": capture.snapshots, "before_failed_step": serde_json::from_slice::<Value>(&expected_bytes).map_err(|error| error.to_string())?,
                    "after_failed_step": serde_json::from_slice::<Value>(&after_bytes).map_err(|error| error.to_string())?,
                })).map_err(|error| error.to_string())?;
                let event_stream =
                    serde_json::to_vec(&capture.updates).map_err(|error| error.to_string())?;
                let receipts =
                    serde_json::to_vec(&capture.receipts).map_err(|error| error.to_string())?;
                let mut artifacts = BTreeMap::new();
                for (name, file, bytes) in [
                    (
                        "authoritative_state",
                        "authoritative-state.json",
                        &authoritative_state,
                    ),
                    ("event_stream", "event-stream.json", &event_stream),
                    ("receipts", "receipts.json", &receipts),
                    ("save_slot", "save-slot.json", &after_bytes),
                ] {
                    artifacts.insert(
                        name,
                        ArtifactFile {
                            file,
                            receipt: ArtifactReceipt::from_bytes(bytes),
                        },
                    );
                }
                return Ok(CaptureBundle {
                    report: CaptureReport {
                        schema: CAPTURE_SCHEMA, status: CaptureStatus::Pass,
                        configuration: RuntimeConfiguration { scenario: config.scenario.clone(), seed: config.seed.to_string(),
                            budget: config.budget.label().to_owned(), actual_rayon_threads: threads.to_string(), repeat: config.repeat.to_string(),
                            mode: config.mode, requested_scheduler_merge_disabled: config.disabled_merge },
                        authority_path: "ProtocolSession::step_frame_with_commit_evidence -> actual disabled canonical merge -> typed rejection and checkpoint rollback",
                        artifacts,
                        runtime_coverage: RuntimeCoverage { tick_from: capture.tick_from.unwrap_or(before.snapshot.tick).to_string(),
                            tick_to: capture.tick_to.to_string(), tick_frames: capture.tick_frames.to_string(), civil_updates: "0".to_owned(),
                            auction_completed_events: capture.auction_completed.to_string(), day_boundary_events: capture.day_boundaries.to_string(),
                            stock_codes: before.setup.stocks.iter().map(|stock| stock.code.0.clone()).collect(),
                            account_ids: before.snapshot.accounts.keys().map(|account| account.0.to_string()).collect(), restore_slots: Vec::new() },
                        scheduler_precanonical_order: serde_json::json!({"available": true, "records": records}),
                        public_payload_order_probe: PublicPayloadOrderProbe { scope: "not used; actual production rejection is the negative control",
                            account_payload_order: Vec::new(), stock_payload_order: Vec::new(), event_payload_order: Vec::new(),
                            canonicalized_bytes: ArtifactReceipt::from_bytes(&[]), probe_disabled_sort_changed_bytes: None },
                        producer_readiness: ProducerReadiness { update_projection_schema: "verification_evidence::UpdateProjection",
                            update_projection_count: capture.updates.len().to_string(), full_update_stream_ordinal_scope: UnavailableCapture { available: true, reason: "committed prefix only" },
                            determinism_observation: None, corpus_projection: None, conservation_snapshots: capture.conservation.iter().map(serde_json::to_value).collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())? },
                        blockers: Vec::new(), scope: "negative-control committed prefix and rejected tick; no claim of full-run coverage",
                        negative_control: Some(serde_json::json!({"detected": true, "kind": "typed-rejection-with-rollback",
                            "disabled_merge": config.disabled_merge, "error": error, "failed_tick": (before.snapshot.tick + 1).to_string(),
                            "enabled_merge_same_step_passed": true, "before_failed_step": ArtifactReceipt::from_bytes(&expected_bytes),
                            "after_failed_step": ArtifactReceipt::from_bytes(&after_bytes),
                            "pre_script": ArtifactReceipt::from_bytes(&before_bytes), "failed_step_published_events": "0"})),
                    }, authoritative_state, event_stream, receipts, save_slot: after_bytes,
                });
            }
        }
    }
    Err(format!(
        "disabled {:?} canonical merge was never detected on the real public path",
        config.disabled_merge
    ))
}

pub(super) fn capture_runtime(seed: u64) -> Result<RuntimeCapture, String> {
    let mut session = initial_session(seed)?;
    let mut capture = Accumulator::default();
    let mut restore_slots = Vec::new();
    for _ in 0..2 {
        let frame = step_scripted(&mut session)?;
        capture.frame(seed, frame, &session)?;
    }
    restore_slots.push(capture.restore_slot(seed, "intraday-quiet-point", &mut session)?);
    while !session
        .civil_day_ready()
        .map_err(|error| error.to_string())?
    {
        let frame = step_scripted(&mut session)?;
        capture.frame(seed, frame, &session)?;
    }
    let civil = session
        .end_civil_day_update()
        .map_err(|error| error.to_string())?;
    capture.updates.push(
        capture
            .projector
            .project_update(RuntimeUpdateRef::Civil(&civil))
            .map_err(|error| format!("complete-stream CivilUpdate projection: {error}"))?,
    );
    capture.snapshots.push(authoritative_checkpoint(&session)?);
    capture.civil_updates += 1;
    restore_slots.push(capture.restore_slot(seed, "post-civil-quiet-point", &mut session)?);
    Ok(RuntimeCapture {
        snapshots: capture.snapshots,
        updates: capture.updates,
        final_save: session.game().save().map_err(|error| error.to_string())?,
        restore_slots,
        tick_from: capture
            .tick_from
            .ok_or("runtime produced no committed TickFrame")?,
        tick_to: capture.tick_to,
        tick_frames: capture.tick_frames,
        civil_updates: capture.civil_updates,
        auction_completed: capture.auction_completed,
        day_boundaries: capture.day_boundaries,
        receipts: capture.receipts,
        conservation: capture.conservation,
        finalizers: capture.finalizers,
        executor_records: Vec::new(),
    })
}

impl Accumulator {
    fn frame(
        &mut self,
        seed: u64,
        (frame, evidence): (TickFrame, TickCommitEvidence),
        session: &ProtocolSession,
    ) -> Result<(), String> {
        frame.validate().map_err(|error| error.to_string())?;
        self.tick_from.get_or_insert(frame.tick);
        self.tick_to = frame.tick;
        self.tick_frames += 1;
        self.auction_completed += count_events(&frame.events, |event| {
            matches!(event, Event::AuctionCompleted { .. })
        })?;
        self.day_boundaries += count_events(&frame.events, |event| {
            matches!(event, Event::DayBoundary { .. })
        })?;
        self.updates.push(
            self.projector
                .project_update(RuntimeUpdateRef::Tick(&frame))
                .map_err(|error| {
                    format!(
                        "complete-stream TickFrame projection at {}: {error}",
                        frame.tick
                    )
                })?,
        );
        let snapshot = session
            .game()
            .save()
            .map_err(|error| format!("tick {} committed save: {error}", frame.tick))?
            .snapshot;
        let chains = evidence
            .envelope_chains()
            .iter()
            .map(|chain| EnvelopeChainInput {
                envelope: chain.envelope(),
                receipts: chain.receipts(),
            })
            .collect::<Vec<_>>();
        self.conservation.push(
            project_conservation_snapshot(
                crate::cli::SCENARIO,
                seed,
                frame.tick,
                &chains,
                &snapshot.accounts,
            )
            .map_err(|error| format!("committed conservation at {}: {error}", frame.tick))?,
        );
        for execution in evidence.b2_finalizers() {
            if execution.auction_tail_passes() != 1
                || execution.auction_completion_passes() > 1
                || execution.day_end_passes() > 1
            {
                return Err(format!(
                    "invalid executed finalizer counts at {} for {}",
                    frame.tick,
                    execution.stock().0
                ));
            }
            self.finalizers.push(FinalizerAuditInput {
                auction_finalizations: execution.auction_completion_passes(),
                day_end_finalizations: execution.day_end_passes(),
            });
        }
        self.receipts.push(receipt_journal(frame.tick, &evidence)?);
        self.snapshots.push(authoritative_checkpoint(session)?);
        Ok(())
    }

    fn restore_slot(
        &mut self,
        seed: u64,
        name: &str,
        session: &mut ProtocolSession,
    ) -> Result<RestoreSlotCapture, String> {
        let saved = authoritative_checkpoint(session)?;
        let saved = serde_json::to_vec(&saved).map_err(|error| error.to_string())?;
        let decoded: SaveSlot =
            serde_json::from_slice(&saved).map_err(|error| error.to_string())?;
        let mut restored = ProtocolSession::restore(&decoded).map_err(|error| error.to_string())?;
        let restored_bytes = serde_json::to_vec(&authoritative_checkpoint(&restored)?)
            .map_err(|error| error.to_string())?;
        if saved != restored_bytes {
            return Err(format!("{name}: restored SaveSlot bytes differ"));
        }
        let original = step_scripted(session)?;
        let replay = step_scripted(&mut restored)?;
        let uninterrupted_continuation = continuation_bytes(&original, session)?;
        let restored_continuation = continuation_bytes(&replay, &restored)?;
        if uninterrupted_continuation != restored_continuation {
            return Err(format!(
                "{name}: restored full frame/receipt/state continuation differs"
            ));
        }
        self.frame(seed, original, session)?;
        Ok(RestoreSlotCapture {
            slot: name.to_owned(),
            saved,
            restored: restored_bytes,
            uninterrupted_continuation,
            restored_continuation,
        })
    }
}

fn authoritative_checkpoint(session: &ProtocolSession) -> Result<Value, String> {
    serde_json::to_value(session.game().save().map_err(|error| error.to_string())?)
        .map_err(|error| format!("authoritative state serialization: {error}"))
}

fn continuation_bytes(
    (frame, evidence): &(TickFrame, TickCommitEvidence),
    session: &ProtocolSession,
) -> Result<Vec<u8>, String> {
    serde_json::to_vec(&(
        frame,
        receipt_journal(frame.tick, evidence)?,
        authoritative_checkpoint(session)?,
    ))
    .map_err(|error| format!("continuation serialization: {error}"))
}

/// Every receipt field is captured from the successful commit, before ledger rebase.
/// Decimal strings retain exact financial and identity integers for JS consumers.
fn receipt_journal(tick: u64, evidence: &TickCommitEvidence) -> Result<Value, String> {
    use engine::session::pipeline::{FeeComponents, ResVec};
    let resource = |value: ResVec| {
        serde_json::json!({
        "cash_cents": value.cash.cents().to_string(), "shares": value.shares.to_string() })
    };
    let fees = |value: FeeComponents| {
        serde_json::json!({
        "commission_cents": value.commission.cents().to_string(),
        "stamp_tax_cents": value.stamp_tax.cents().to_string(),
        "transfer_fee_cents": value.transfer_fee.cents().to_string() })
    };
    let receipts = evidence.receipts().iter().map(|receipt| {
        serde_json::json!({
            "index": receipt.index.to_string(),
            "local_key": receipt.local_key,
            "envelope": receipt.envelope,
            "kind": format!("{:?}", receipt.kind),
            "qty_before": receipt.qty_before.to_string(), "qty_after": receipt.qty_after.to_string(),
            "value_before_cents": receipt.value_before.cents().to_string(),
            "value_after_cents": receipt.value_after.cents().to_string(),
            "spent": resource(receipt.delta.spent),
            "released": resource(receipt.delta.released), "live_after": resource(receipt.delta.live_after),
            "nominal": fees(receipt.nominal), "charged": fees(receipt.charged),
            "charged_before": fees(receipt.charged_before), "charged_after": fees(receipt.charged_after),
            "deliver_qty": receipt.deliver_qty.to_string(),
            "deliver_cash_cents": receipt.deliver_cash.cents().to_string(),
        })
    }).collect::<Vec<_>>();
    Ok(decimal_identity_json(serde_json::json!({
        "tick": tick.to_string(), "next_receipt_index": evidence.next_receipt_index().to_string(),
        "p0_receipt_count": evidence.p0_receipts().len().to_string(), "receipts": receipts,
        "finalizers": evidence.b2_finalizers().iter().map(|execution| serde_json::json!({
            "stock": execution.stock().0, "auction_tail_passes": execution.auction_tail_passes().to_string(),
            "auction_completion_passes": execution.auction_completion_passes().to_string(),
            "day_end_passes": execution.day_end_passes().to_string(),
        })).collect::<Vec<_>>(),
    })))
}

// ReceiptLocalKey also contains nested u64 source/transition identities. Do not
// leave those as JSON numbers while advertising an exact JS-consumable journal.
fn decimal_identity_json(value: Value) -> Value {
    match value {
        Value::Number(number) if number.is_i64() || number.is_u64() => {
            Value::String(number.to_string())
        }
        Value::Array(values) => {
            Value::Array(values.into_iter().map(decimal_identity_json).collect())
        }
        Value::Object(values) => Value::Object(
            values
                .into_iter()
                .map(|(key, value)| (key, decimal_identity_json(value)))
                .collect(),
        ),
        other => other,
    }
}

fn initial_session(seed: u64) -> Result<ProtocolSession, String> {
    let session = ProtocolSession::new(frozen_setup()?, seed).map_err(|error| error.to_string())?;
    let mut initial = session.game().save().map_err(|error| error.to_string())?;
    // A declared initial allocation, not a runtime mutation: transfer 1,000
    // already-settled shares per stock from an NPC to the player. Total supply
    // is unchanged. Production restore validates the complete initial slot.
    for spec in &initial.setup.stocks {
        let donor = initial
            .snapshot
            .accounts
            .iter()
            .find_map(|(id, account)| {
                (*id != AccountId(0)
                    && account
                        .positions
                        .get(&spec.code)
                        .is_some_and(|position| position.qty - position.t1_locked >= 1_000))
                .then_some(*id)
            })
            .ok_or_else(|| format!("no settled allocation donor for {}", spec.code.0))?;
        let donor_position = initial
            .snapshot
            .accounts
            .get_mut(&donor)
            .expect("selected donor")
            .positions
            .get_mut(&spec.code)
            .expect("selected position");
        donor_position.qty -= 1_000;
        initial
            .snapshot
            .accounts
            .get_mut(&AccountId(0))
            .ok_or("missing player")?
            .positions
            .insert(
                spec.code.clone(),
                PositionSnap {
                    qty: 1_000,
                    t1_locked: 0,
                    invested_cents: 0,
                    recovered_cents: 0,
                },
            );
    }
    ProtocolSession::restore(&initial)
        .map_err(|error| format!("frozen initial allocation restore: {error}"))
}

fn enqueue_script(session: &mut ProtocolSession) -> Result<(), String> {
    let tick = session.game().tick();
    if tick == 0 || tick == 5 {
        // Two sell legs and one incoming buy per stock guarantee an actual
        // multi-leg receipt chain, including the opening auction. Self crossing
        // is the existing game simplification, not a claim about A-share legality.
        for (code, price) in [("600101", 1_120), ("000812", 885)] {
            for (side, qty) in [(Side::Sell, 100), (Side::Sell, 200), (Side::Buy, 300)] {
                session
                    .enqueue_player_intent(
                        AccountId(0),
                        Intent::PlaceLimit {
                            code: StockCode(code.to_owned()),
                            side,
                            price: Money::from_cents(price),
                            qty,
                        },
                    )
                    .map_err(|error| error.to_string())?;
            }
        }
    }
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: StockCode("600999".to_owned()),
                side: Side::Buy,
                price: Money::from_cents(1_000),
                qty: 100,
            },
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn step_scripted(session: &mut ProtocolSession) -> Result<(TickFrame, TickCommitEvidence), String> {
    let tick = session.game().tick();
    enqueue_script(session)?;
    session
        .step_frame_with_commit_evidence()
        .map_err(|error| format!("committed tick {tick}: {error}"))
}

pub(super) fn identity_orders(
    records: &[ExecutorOrderRecord],
) -> Result<PrecanonicalIdentities, String> {
    let select = |boundaries: &[ExecutorBoundary]| {
        records
            .iter()
            .find(|record| {
                boundaries.contains(&record.boundary)
                    && record.identities.len() >= 2
                    && record.item_counts.iter().all(|count| *count > 0)
            })
            .map(|record| record.identities.clone())
            .ok_or_else(|| format!("no real nonempty multi-shard evidence at {boundaries:?}"))
    };
    Ok(PrecanonicalIdentities {
        accounts: select(&[ExecutorBoundary::P3AccountShards])?,
        stocks: select(&[
            ExecutorBoundary::P4AuctionStockShards,
            ExecutorBoundary::P4ContinuousStockShards,
        ])?,
        completions: select(&[ExecutorBoundary::P5ReceiptResults])?,
    })
}
