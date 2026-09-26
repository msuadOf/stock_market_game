import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { lstat, readFile, realpath } from "node:fs/promises";
import path from "node:path";

const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
const canonical = (value) => Array.isArray(value) ? value.map(canonical)
  : value !== null && typeof value === "object"
    ? Object.fromEntries(Object.keys(value).sort().map((key) => [key, canonical(value[key])])) : value;
const hashValue = (value) => sha256(JSON.stringify(canonical(value)));
const semanticHash = (value) => hashValue(projected(value));
const integer = (value, label) => {
  assert((typeof value === "number" && Number.isSafeInteger(value))
    || (typeof value === "string" && /^(0|[1-9][0-9]*)$/.test(value)), `${label}: expected exact integer`);
  return BigInt(value);
};

function projected(value) {
  if (typeof value === "number" && Number.isInteger(value)) {
    assert(Number.isSafeInteger(value), "legacy JSON contains an unsafe integer; refusing rounded evidence");
    return String(value);
  }
  if (Array.isArray(value)) return value.map(projected);
  if (value !== null && typeof value === "object") return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, projected(item)]));
  return value;
}

async function artifact(root, relative) {
  assert(typeof relative === "string" && relative.length > 0 && !path.isAbsolute(relative)
    && relative.split(/[\\/]/).every((part) => part !== ".." && part !== "." && part !== ""), "unsafe corpus artifact path");
  let cursor = root;
  for (const part of relative.split("/")) {
    cursor = path.join(cursor, part);
    assert(!(await lstat(cursor)).isSymbolicLink(), `symlinked corpus artifact: ${relative}`);
  }
  assert((await lstat(cursor)).isFile(), `corpus artifact is not a regular file: ${relative}`);
  return readFile(cursor);
}

export async function loadSealedCorpusRun(rootPath, scenario, seed) {
  const root = await realpath(rootPath);
  const indexBytes = await artifact(root, "manifest.json");
  const index = JSON.parse(indexBytes);
  assert.equal(index.schema, 2, "sealed index schema");
  assert.equal(index.kind, "sealed-corpus-index", "not a sealed corpus index");
  const manifestBytes = await artifact(root, index.manifest);
  assert.equal(sha256(manifestBytes), index.manifest_sha256, "sealed manifest SHA-256 mismatch");
  const manifest = JSON.parse(manifestBytes);
  assert.equal(manifest.status, "sealed", "manifest is not sealed");
  assert.equal(manifest.corpus_digest, index.corpus_digest, "sealed corpus digest mismatch");
  const matches = manifest.runs.filter((run) => run.scenario === scenario && String(run.seed) === String(seed));
  assert.equal(matches.length, 1, "sealed run identity absent or duplicate");
  const run = matches[0];
  const runPath = path.posix.join(path.posix.dirname(index.manifest), run.file);
  // Validate the original run path before joining, so traversal cannot normalize away.
  assert(!run.file.includes("/") && !run.file.includes("\\") && run.file !== "..", "unsafe run file name");
  const bytes = await artifact(root, runPath);
  assert.equal(sha256(bytes), run.sha256, "sealed run SHA-256 mismatch");
  assert.equal(run.sha256, run.repeat_sha256, "sealed run repeat hash differs");
  const lines = bytes.toString("utf8").trimEnd().split("\n");
  assert(lines.length > 0 && lines.every((line) => line.length > 0), "empty corpus row");
  const records = lines.map((line) => JSON.parse(line));
  assert.equal(records[0].kind, "configuration", "missing initial configuration");
  assert.equal(String(records[0].seed), String(seed), "configuration seed differs from sealed run");
  projected(records); // Check exact-integer safety before any semantic projection.
  return { scenario, seed: String(seed), records, provenance: {
    index_sha256: sha256(indexBytes), manifest_sha256: index.manifest_sha256,
    corpus_digest: index.corpus_digest, run_file: runPath, run_sha256: run.sha256,
  } };
}

function eventDomain(variant, payload) {
  if (["Trade", "AuctionTick", "AuctionCompleted", "PriceTick"].includes(variant)) {
    assert(/^[0-9]{6}$/.test(payload.code), "invalid stock event code");
    return { phase: 4, entity: `Stock:${payload.code}`, source: variant === "Trade" ? "Sealed" : "PriceTick",
      stable: variant === "Trade" ? `${payload.maker}:${payload.taker}` : payload.code };
  }
  if (["OrderAccepted", "OrderCanceled", "IntentRejected", "SettlementError"].includes(variant)) {
    integer(payload.account, "event account");
    return { phase: 4, entity: `Account:${payload.account}`, source: "Sealed",
      stable: ["OrderAccepted", "OrderCanceled"].includes(variant) ? String(payload.id) : payload.code };
  }
  if (variant === "DayBoundary") return { phase: 5, entity: "Session", source: "DayEnd", stable: String(payload.day) };
  // This adapter reads sealed historical evidence only. Preserve its removed
  // ResourceLimit fact verbatim; current-runtime contracts no longer accept it.
  if (["CivilDateAdvanced", "CompanyDisclosurePublished", "ResourceLimit"].includes(variant)) {
    return { phase: 6, entity: "Session", source: "Session", stable: null };
  }
  throw new Error(`legacy event variant has no exhaustive mapping: ${variant}`);
}

function normalizeFrame(frame, counters, derivations) {
  assert.equal(frame.kind, "TickFrame", "legacy primary stream must contain TickFrame rows");
  const tick = integer(frame.tick, "frame tick").toString();
  const events = [...frame.events].sort((a, b) => Number(integer(Object.values(a.event)[0].seq, "seq") - integer(Object.values(b.event)[0].seq, "seq")));
  const oldOrdinals = new Map();
  const facts = events.map((fact) => {
    assert.deepEqual(Object.keys(fact).sort(), ["canonical_session_ordinal", "event", "identity"], "legacy fact fields");
    const variants = Object.entries(fact.event);
    assert.equal(variants.length, 1, "legacy fact must have one variant");
    const [variant, payload] = variants[0];
    const domain = eventDomain(variant, payload);
    assert(Array.isArray(fact.identity) && fact.identity.length === 5, "legacy identity must have five parts");
    assert.equal(String(fact.identity[0]), tick, "legacy event tick mismatch");
    assert.equal(fact.identity[1], variant, "legacy variant mismatch");
    assert.equal(fact.identity[2], domain.entity, "legacy entity mismatch");
    if (domain.stable !== null) assert.equal(fact.identity[3], domain.stable, "legacy stable identity payload mismatch");
    const oldScope = JSON.stringify(fact.identity.slice(1, 4));
    const oldOrdinal = oldOrdinals.get(oldScope) ?? 0;
    assert.equal(fact.identity[4], oldOrdinal, "legacy occurrence ordinal mismatch");
    oldOrdinals.set(oldScope, oldOrdinal + 1);
    const scope = `${tick}:${domain.phase}:${domain.entity}:${domain.source}`;
    const ordinal = counters.get(scope) ?? 0;
    counters.set(scope, ordinal + 1);
    if (domain.phase === 6) assert.equal(fact.canonical_session_ordinal, ordinal, "legacy shared Session ordinal mismatch");
    else assert.equal(fact.canonical_session_ordinal, null, "unexpected legacy Session ordinal");
    const key = [tick, `${domain.phase}:${variant}`, domain.entity, String(ordinal)];
    derivations.push({ legacy_identity: fact.identity, legacy_seq: String(payload.seq), comparison_event_key: key });
    return { comparison_event_key: key, event: projected(fact.event) };
  });
  const from = integer(frame.seq_from, "seq_from");
  const to = integer(frame.seq_to, "seq_to");
  assert.equal(to - from + 1n, BigInt(facts.length), "legacy event cardinality mismatch");
  facts.forEach((fact, index) => assert.equal(BigInt(Object.values(fact.event)[0].seq), from + BigInt(index), "legacy seq gap"));
  return { kind: "TickFrame", tick, seq_from: from.toString(), seq_to: to.toString(), events: facts,
    timeseries_payload: normalizeTimeseries(frame, facts) };
}

function normalizeTimeseries(frame, facts) {
  const series = frame.timeseries_payload;
  const auction = {};
  const continuous = {};
  for (const fact of facts) {
    const [variant, payload] = Object.entries(fact.event)[0];
    if (variant === "PriceTick") {
      // Frame snapshot is the post-step phase: the last continuous tick may
      // already expose ClosingAuction. PriceTick itself is the continuous fact.
      continuous[payload.code] = { tick: String(frame.tick), phase: "Continuous",
        last_price: payload.last_price, cumulative_volume: payload.daily_candle.volume,
        bids: payload.bids, asks: payload.asks };
    } else if (variant === "AuctionTick" || variant === "AuctionCompleted") {
      const points = auction[payload.code] ?? [];
      points.push({ key: { phase_rank: "4", entity: { Stock: payload.code }, source: "PriceTick", local_event_index: fact.comparison_event_key[3] },
        tick: String(frame.tick), kind: variant === "AuctionTick" ? "Indication" : "Completion",
        phase: payload.phase, indicative_price: variant === "AuctionTick" ? payload.indicative_price : payload.clearing_price,
        matched_volume: payload.matched_volume, imbalance: variant === "AuctionTick" ? payload.imbalance : null });
      auction[payload.code] = points;
    }
  }
  const closed = {};
  if (facts.some((fact) => Object.hasOwn(fact.event, "DayBoundary"))) {
    for (const [code, candles] of Object.entries(series.daily_candles)) {
      assert(candles.length > 0, "DayBoundary has no closed candle");
      closed[code] = projected(candles.at(-1));
    }
  }
  return { markets: projected(series.markets), active_daily_candles: projected(series.active_daily_candles),
    closed_daily_candles: closed, auction_points: auction, continuous_points: continuous };
}

function businessState(state) {
  const copy = projected(state);
  if (copy.snapshot) delete copy.snapshot.seq; // Only the global transport cursor, never Order.seq/FIFO.
  // Exact #7 metadata normalization, not a wildcard save-state exemption.
  if (copy.setup) delete copy.setup.simulation_policy_id;
  if (copy.state) copy.state = businessState(copy.state); // before_save / after_restore wrapper
  return copy;
}

export function adaptLegacyStream(run) {
  const { records } = run;
  const configuration = records[0];
  const derivations = [];
  const counters = new Map();
  const frames = records.filter((row) => row.kind === "TickFrame");
  assert(frames.length > 0, "legacy stream has no frames");
  const updates = frames.map((frame) => normalizeFrame(frame, counters, derivations));
  for (let index = 1; index < updates.length; index++) {
    assert.equal(BigInt(updates[index].tick), BigInt(updates[index - 1].tick) + 1n, "legacy tick gap");
    assert.equal(BigInt(updates[index].seq_from), BigInt(updates[index - 1].seq_to) + 1n, "legacy seq coverage gap");
  }
  // The raw history container is represented completely by normalized updates
  // and snapshot state; current runtime exposes the same semantic series there.
  const sellerFeeProjectionRows = frames
    .filter((frame) => frame.seller_fee_projection !== null && frame.seller_fee_projection !== undefined)
    .map((frame) => ({ tick: integer(frame.tick, "seller fee projection tick").toString(),
      sha256: hashValue(frame.seller_fee_projection) }));
  // `seller_fee_projection` was injected by the historical verification
  // harness. It is extracted into reviewed surface controls and must not also
  // masquerade as authority state in the common checkpoint comparison.
  const checkpoints = frames.map(({ events, seq_from, seq_to, timeseries_payload,
    seller_fee_projection: _sellerFeeProjection, ...checkpoint }) => businessState(checkpoint));
  const restore = records.filter((row) => ["before_save", "after_restore"].includes(row.kind)).map((row) => businessState(row));
  const terminal = records.filter((row) => row.kind === "terminal");
  assert.equal(terminal.length, 1, "legacy stream must have one terminal state");
  return { updates, state: { initial: businessState(configuration.initial_state), checkpoints,
    restore_checkpoints: restore, terminal: businessState(terminal[0].state),
    sealed_exogenous_script: projected(configuration.sealed_exogenous_script) },
  provenance: { ...run.provenance, event_key_derivations: derivations,
    excluded_global_cursor: "snapshot.seq only; order arrival seq retained",
    representation_normalization: ["setup.simulation_policy_id (#7 metadata)",
      "checkpoint.timeseries_payload (raw representation; normalized series retained in updates and snapshot)",
      "checkpoint.seller_fee_projection (historical harness witness; SHA-256 retained and values extracted into reviewed surface controls)"],
    seller_fee_projection_rows: sellerFeeProjectionRows,
    auxiliary_records: records.map((row, index) => ({ row, index })).filter(({ row }) => !["configuration", "TickFrame", "before_save", "after_restore", "terminal"].includes(row.kind))
      .map(({ row, index }) => ({ index, kind: row.kind, sha256: hashValue(row) })) } };
}

/** Generate only a fresh current-engine setup + sealed exogenous input script.
 * No legacy SaveSlot is restored and no current state is copied from an old
 * checkpoint. The current endpoint independently builds companies and RNG. */
export function currentReplayRequest(run) {
  const configuration = run.records[0];
  assert.equal(configuration.setup.simulation_policy_id, "a-share-simulation-v1", "unexpected sealed policy");
  assert(["equivalence", "divergence-9", "representation"].includes(run.scenario),
    "current primary replay needs an explicitly supported sealed scenario");
  const setup = structuredClone(configuration.setup);
  assert.equal(setup.npcs.retail_count + setup.npcs.inst_count + setup.npcs.hot_count, 0,
    "primary replay must not reconstruct state-dependent NPC decisions");
  setup.simulation_policy_id = "a-share-simulation-v2";
  const frames = run.records.filter((row) => row.kind === "TickFrame");
  assert(frames.length > 0 && frames.length <= 200, "controlled replay requires 1..200 frames");
  assert(frames.every((frame, index) => String(frame.tick) === String(index + 1)), "primary replay must begin at tick 1 without gaps");
  const request = { schema: "escrow-current-corpus-request-v1", scenario: run.scenario, seed: run.seed,
    setup, initial_accounts: structuredClone(configuration.initial_state.snapshot.accounts),
    sealed_exogenous_script: structuredClone(configuration.sealed_exogenous_script), ticks: frames.length,
    restore_at_ticks: run.records.filter((row) => row.kind === "before_save").map((row) => row.tick) };
  if (run.scenario === "representation") {
    const sellCommands = configuration.sealed_exogenous_script.flatMap(([, intents]) => intents)
      .filter((intent) => intent.PlaceLimit?.side === "Sell");
    assert.equal(sellCommands.length, 1, "controlled representation needs exactly one external Sell");
    const account = "0";
    const stock = sellCommands[0].PlaceLimit.code;
    const observed = frames.find((frame) => frame.snapshot.accounts[account]?.reserved_sell_qty?.[stock] > 0);
    assert(observed, "controlled representation never observed the historical live Sell");
    const reservation = integer(observed.snapshot.accounts[account].reserved_cash,
      "controlled legacy Sell reservation").toString();
    request.historical_surface_hints = { controlled_sell: { account, stock,
      legacy_sell_reservation_cents: reservation } };
  }
  if (run.scenario === "divergence-9") {
    const hints = {};
    if (run.records.some((row) => row.kind === "acceptance_boundary")) {
      hints.acceptance_flip_legacy_sell_reservation_cents =
        extractLegacyAcceptanceFlipSurface(run).surface.corpus_control.old_sell_reservation_cents;
    }
    if (run.records.some((row) => row.kind === "independent_seller_fee_control")) {
      hints.three_leg_legacy_sell_reservation_cents =
        extractLegacyThreeLegFeeCatchupSurface(run).surface.corpus_control.old_sell_reservation_cents;
    }
    if (Object.keys(hints).length > 0) request.historical_surface_hints = hints;
  }
  return request;
}

function frameFacts(frame) {
  assert(Array.isArray(frame.events), `legacy tick ${frame.tick} events must be an array`);
  return frame.events.map((fact) => {
    assert(fact !== null && typeof fact === "object" && !Array.isArray(fact), `legacy tick ${frame.tick} fact is invalid`);
    const variants = Object.entries(fact.event ?? {});
    assert.equal(variants.length, 1, `legacy tick ${frame.tick} fact must have one event variant`);
    const [variant, payload] = variants[0];
    return { variant, payload };
  });
}

function oneAuxiliaryRecord(run, kind) {
  const matches = run.records.filter((record) => record.kind === kind);
  assert.equal(matches.length, 1, `sealed corpus needs exactly one ${kind} record`);
  return matches[0];
}

function oneNamedAuxiliaryRecord(run, kind, name) {
  const matches = run.records.filter((record) => record.kind === kind && record.name === name);
  assert.equal(matches.length, 1, `sealed corpus needs exactly one ${kind}/${name} record`);
  return matches[0];
}

function normalizeAuxiliaryFrames(frames, label) {
  assert(Array.isArray(frames) && frames.length > 0, `${label} needs at least one frame`);
  const counters = new Map();
  const derivations = [];
  const updates = frames.map((frame) => normalizeFrame(frame, counters, derivations));
  for (let index = 1; index < updates.length; index++) {
    assert.equal(BigInt(updates[index].tick), BigInt(updates[index - 1].tick) + 1n,
      `${label} tick gap`);
    assert.equal(BigInt(updates[index].seq_from), BigInt(updates[index - 1].seq_to) + 1n,
      `${label} sequence gap`);
  }
  return { updates, derivations };
}

function sellerCorpusControl(surface, legacyReservation, feePrefixes, acceptance, zeroCash, subject = null) {
  return {
    surface, seller_order_count: "1", old_sell_reservation_cents: legacyReservation.toString(),
    fee_prefixes: feePrefixes, feedback: { strategy_generated_intents: "0", plan_generated_intents: "0",
      state_dependent_intents: "0" }, acceptance, zero_cash_acceptance: zeroCash,
    sealed_exogenous_script_sha256: null, rng_cursor: null, strategy_state_sha256: null,
    plan_state_sha256: null, pending_intents_sha256: null, restore_order_sha256: null,
    comparison_points: ["post-commit"], surface_evidence: subject,
  };
}

function exactSignedInteger(value, label) {
  assert((typeof value === "number" && Number.isSafeInteger(value))
    || (typeof value === "string" && /^-?(0|[1-9][0-9]*)$/.test(value)), `${label}: expected exact signed integer`);
  return BigInt(value);
}

function decimalRate(value, label) {
  assert(typeof value === "number" && Number.isFinite(value) && value >= 0,
    `${label}: expected a finite non-negative decimal rate`);
  const text = String(value);
  const match = /^(\d+)(?:\.(\d+))?$/.exec(text);
  assert(match, `${label}: exponential rate notation is not accepted as evidence`);
  const fractional = match[2] ?? "";
  return { numerator: BigInt(`${match[1]}${fractional}`), denominator: 10n ** BigInt(fractional.length) };
}

function applyRate(cents, rate, label) {
  const { numerator, denominator } = decimalRate(rate, label);
  const scaled = cents * numerator;
  const quotient = scaled / denominator;
  const remainder = scaled % denominator;
  const doubled = remainder * 2n;
  return quotient + (doubled > denominator || (doubled === denominator && quotient % 2n === 1n) ? 1n : 0n);
}

function legacyFee(config, gross, side, label) {
  const commission = [applyRate(gross, config.commission_rate, `${label} commission rate`),
    integer(config.commission_min, `${label} commission minimum`)].reduce((left, right) => left > right ? left : right);
  const stampTax = side === "Sell" ? applyRate(gross, config.stamp_tax_rate, `${label} stamp-tax rate`) : 0n;
  // The frozen v1 engine contract charges both sides 0.01 per mille. Keep the
  // rational here so extraction never depends on binary floating-point math.
  const transferFee = applyRate(gross, 0.00001, `${label} transfer-fee rate`);
  return { commission, stampTax, transferFee };
}

function restingSell(frame, account, stock, orderId) {
  const orders = frame.orders?.resting?.[stock] ?? [];
  assert(Array.isArray(orders), `legacy tick ${frame.tick} resting orders must be an array`);
  const matches = orders.filter((order) => String(order.owner) === account && order.side === "Sell"
    && String(order.id) === orderId);
  assert(matches.length <= 1, `legacy tick ${frame.tick} duplicated the controlled Sell order`);
  return matches[0] ?? null;
}

function validateLegacySellerFeeProjection(frame, previousCash) {
  const fee = frame.seller_fee_projection;
  assert(fee !== null && typeof fee === "object" && !Array.isArray(fee),
    `legacy tick ${frame.tick} is missing its sealed seller fee projection`);
  assert.deepEqual(Object.keys(fee).sort(), [
    "gross_cents", "new_expected_charged_cents", "new_expected_net_cents",
    "observed_combined_account_cash_delta", "old_charged_cents", "old_net_cents",
    "unchanged_counterparty_buy_fee_cents",
  ].sort(), `legacy tick ${frame.tick} seller fee projection fields`);
  const gross = integer(fee.gross_cents, `legacy tick ${frame.tick} gross`);
  const oldCharged = integer(fee.old_charged_cents, `legacy tick ${frame.tick} old charged`);
  const oldNet = exactSignedInteger(fee.old_net_cents, `legacy tick ${frame.tick} old net`);
  const newCharged = integer(fee.new_expected_charged_cents, `legacy tick ${frame.tick} new charged`);
  const newNet = exactSignedInteger(fee.new_expected_net_cents, `legacy tick ${frame.tick} new net`);
  const buyerFee = integer(fee.unchanged_counterparty_buy_fee_cents,
    `legacy tick ${frame.tick} counterparty buy fee`);
  const observedDelta = exactSignedInteger(fee.observed_combined_account_cash_delta,
    `legacy tick ${frame.tick} observed cash delta`);
  const cash = integer(frame.snapshot?.accounts?.["0"]?.cash, `legacy tick ${frame.tick} controlled account cash`);
  assert.equal(oldNet, gross - oldCharged, `legacy tick ${frame.tick} old net fee equation`);
  assert.equal(newNet, gross - newCharged, `legacy tick ${frame.tick} projected new net fee equation`);
  assert.equal(observedDelta, oldNet - gross - buyerFee,
    `legacy tick ${frame.tick} combined self-trade cash equation`);
  assert.equal(cash - previousCash, observedDelta, `legacy tick ${frame.tick} cash delta`);
  return { gross, oldCharged, cash };
}

/**
 * Mechanically extracts the two approved controlled-live-sell surfaces from the
 * sealed representation run. `seller_fee_projection` is a frozen harness
 * witness, not a receipt; every value is therefore cross-checked against Trade,
 * order, account and position checkpoints before it enters the surface block.
 */
export function extractLegacyControlledSellSurface(run, surface) {
  assert(["auction-rollover", "cross-tick-partial-fill", "save-restore-live-order"].includes(surface),
    "controlled legacy extractor supports only the three reviewed representation surfaces");
  if (surface === "save-restore-live-order") {
    const base = extractLegacyControlledSellSurface(run, "cross-tick-partial-fill");
    const before = run.records.filter((record) => record.kind === "before_save");
    const after = run.records.filter((record) => record.kind === "after_restore");
    assert.equal(before.length, 1, "controlled save surface needs exactly one before_save record");
    assert.equal(after.length, 1, "controlled save surface needs exactly one after_restore record");
    assert.equal(String(before[0].tick), String(after[0].tick), "save/restore checkpoints must share a tick");
    assert.deepEqual(businessState(before[0].state), businessState(after[0].state),
      "after_restore must preserve the complete legacy save checkpoint");
    const saveState = before[0].state;
    assert(saveState && typeof saveState === "object" && !Array.isArray(saveState),
      "before_save must carry a state object");
    let representation = saveState.save_representation
      ?? saveState.state?.save_representation;
    if (representation === undefined
      && saveState.schema_version === undefined
      && saveState.runtime_v2 === undefined
      && saveState.setup?.simulation_policy_id === "a-share-simulation-v1") {
      representation = { schema: "v1" };
    }
    assert(representation && typeof representation === "object" && !Array.isArray(representation),
      "save/restore surface must expose explicit save_representation metadata");
    const stock = run.records[0].sealed_exogenous_script.flatMap(([, intents]) => intents)
      .find((intent) => intent.PlaceLimit?.side === "Sell")?.PlaceLimit.code;
    assert(typeof stock === "string", "controlled save surface lacks the sealed Sell stock");
    const savedAccount = saveState.snapshot?.accounts?.["0"] ?? saveState.state?.snapshot?.accounts?.["0"];
    assert(savedAccount, "before_save lacks controlled seller account snapshot");
    const savedPosition = savedAccount.positions?.[stock] ?? savedAccount.positions?.[String(stock)];
    assert(savedPosition, "before_save lacks controlled seller position");
    assert(BigInt(savedPosition.qty) >= 0n, "before_save seller position quantity is invalid");
    const orderBooks = [saveState.orders?.resting, saveState.orders?.auction,
      saveState.resting_orders, saveState.auction_orders].filter(Boolean);
    const savedOrders = orderBooks.flatMap((book) => book?.[stock] ?? book?.[String(stock)] ?? []);
    const savedOrder = savedOrders.find((order) => String(order.owner ?? order.account) === "0"
      && String(order.id) === base.state.order_id && order.side === "Sell");
    assert(savedOrder, "before_save lacks the controlled live Sell order identity");
    const liveShares = integer(savedOrder.qty, "before_save live Sell quantity");
    const reservedSellShares = integer(savedAccount.reserved_sell_qty?.[stock]
      ?? savedAccount.reserved_sell_qty?.[String(stock)],
    "before_save reserved Sell quantity");
    assert.equal(reservedSellShares, liveShares,
      "before_save reserved Sell quantity differs from the controlled live order");
    const continuation = run.records.filter((record) => record.kind === "TickFrame"
      && BigInt(record.tick) > BigInt(before[0].tick));
    assert(continuation.length > 0, "save/restore surface needs a post-restore continuation tick");
    const orderId = base.state.order_id;
    const continuationFacts = continuation.flatMap((frame) => frameFacts(frame));
    const canceled = continuationFacts.some(({ variant, payload }) => variant === "OrderCanceled"
      && String(payload.id) === orderId);
    const terminalContinuation = continuation.at(-1);
    const terminalStillLive = restingSell(terminalContinuation, "0", stock, orderId) !== null;
    assert(canceled || !terminalStillLive,
      "post-restore continuation must cancel or otherwise remove the controlled live Sell");
    const gross = integer(savedOrder.filled_value, "before_save filled value");
    assert(gross > 0n, "before_save live Sell has no historical fill value");
    const firstPrefix = base.corpus_control.fee_prefixes[0];
    assert(firstPrefix, "controlled save surface lacks the validated first fee prefix");
    const nominal = integer(firstPrefix.nominal_cents, "before_save nominal fee");
    const charged = integer(firstPrefix.charged_cents, "before_save charged fee");
    const continuationControl = {
      sealed_exogenous_script_sha256: semanticHash(run.records[0].sealed_exogenous_script),
      strategy_state_sha256: semanticHash(saveState.strategy_profiles),
      plan_state_sha256: semanticHash({ plans: saveState.plans,
        pending_plan_events: saveState.pending_plan_events }),
      pending_intents_sha256: semanticHash(saveState.pending_player),
      restore_order_sha256: semanticHash({ auction_orders: saveState.auction_orders,
        resting_orders: saveState.resting_orders }),
      rng_cursor: integer(saveState.rng_state, "before_save RNG cursor").toString(),
    };
    return {
      case_id: `${run.scenario}-${run.seed}-save-restore-live-order`,
      class: "controlled-live-sell",
      state: { live_shares: liveShares.toString(), order_id: base.state.order_id,
        reserved_cash_cents: integer(savedAccount.reserved_cash,
          "before_save seller reservation").toString(),
        save_representation: projected(representation) },
      seller_fee_control: {
        charged_final_cents: charged.toString(), fills: [], gross_cents: gross.toString(),
        invested_cents: exactSignedInteger(savedPosition.invested_cents,
          "before_save invested cost").toString(), nominal_final_cents: nominal.toString(),
        recovered_cents: exactSignedInteger(savedPosition.recovered_cents,
          "before_save recovered value").toString(), terminal_cash_cents: integer(savedAccount.cash,
          "before_save seller cash").toString(),
      },
      corpus_control: {
        acceptance: "accepted", fee_prefixes: [{ charged_cents: charged.toString(),
          nominal_cents: nominal.toString() }],
        feedback: { plan_generated_intents: "0", state_dependent_intents: "0",
          strategy_generated_intents: "0" },
        old_sell_reservation_cents: base.corpus_control.old_sell_reservation_cents,
        ...continuationControl,
        seller_order_count: "1", surface, surface_evidence: null,
        zero_cash_acceptance: false,
        comparison_points: ["pre-save", "post-restore", "post-continuation-tick"],
      },
    };
  }
  assert.equal(run.scenario, "representation", "controlled legacy surface needs the representation run");
  const configuration = run.records[0];
  assert.equal(configuration.kind, "configuration", "controlled legacy surface lacks configuration");
  assert.equal(configuration.class, "iv-controlled-live-sell", "controlled legacy surface class mismatch");
  const sellCommands = configuration.sealed_exogenous_script.flatMap(([, intents]) => intents)
    .filter((intent) => intent.PlaceLimit?.side === "Sell");
  assert.equal(sellCommands.length, 1, "controlled legacy surface needs exactly one external Sell");
  const stock = sellCommands[0].PlaceLimit.code;
  const submittedQty = integer(sellCommands[0].PlaceLimit.qty, "controlled legacy Sell quantity");
  const account = "0";
  const frames = run.records.filter((record) => record.kind === "TickFrame")
    .sort((left, right) => Number(integer(left.tick, "legacy frame tick") - integer(right.tick, "legacy frame tick")));
  assert(frames.length >= 2, "controlled legacy surface needs committed frames");
  const accepted = frames.flatMap((frame) => frameFacts(frame).map((fact) => ({ frame, ...fact })))
    .filter(({ variant, payload }) => variant === "OrderAccepted" && String(payload.account) === account
      && payload.code === stock && payload.side === "Sell");
  assert.equal(accepted.length, 1, "controlled legacy Sell acceptance is absent or ambiguous");
  const orderId = integer(accepted[0].payload.id, "controlled legacy Sell order id").toString();
  assert.equal(integer(accepted[0].payload.remaining_qty, "controlled legacy accepted quantity"), submittedQty,
    "controlled legacy accepted quantity differs from the sealed command");

  const acceptedAccount = accepted[0].frame.snapshot?.accounts?.[account];
  const legacyReservation = integer(acceptedAccount?.reserved_cash,
    "controlled legacy Sell reservation");
  assert(legacyReservation > 0n, "controlled legacy Sell reservation must expose divergence #9");
  assert.equal(integer(acceptedAccount?.reserved_sell_qty?.[stock], "controlled legacy reserved Sell shares"),
    submittedQty, "controlled legacy reserved shares differ from the accepted Sell");

  const rollover = frames.find((frame) => frameFacts(frame).some(({ variant, payload }) =>
    variant === "AuctionCompleted" && payload.code === stock)
    && restingSell(frame, account, stock, orderId) !== null
    && integer(restingSell(frame, account, stock, orderId).qty,
      `legacy tick ${frame.tick} rollover quantity`) === submittedQty);
  assert(rollover, "controlled legacy Sell has no mechanically bound auction rollover");
  const auctionBefore = frames.some((frame) => integer(frame.tick, "legacy frame tick") < integer(rollover.tick, "rollover tick")
    && (frame.orders?.auction?.[stock] ?? []).some((order) => String(order.owner) === account
      && order.side === "Sell" && integer(order.qty, "controlled legacy auction quantity") === submittedQty));
  assert(auctionBefore, "controlled legacy Sell was not observed in the auction before rollover");

  const fills = [];
  let previousFilled = 0n;
  let previousCash = integer(configuration.initial_state.snapshot.accounts[account].cash,
    "controlled legacy initial cash");
  let charged = 0n;
  let gross = 0n;
  let checkpoint = null;
  const feePrefixes = [];
  for (const frame of frames) {
    const accountAfter = frame.snapshot?.accounts?.[account];
    assert(accountAfter, `legacy tick ${frame.tick} lacks the controlled account`);
    const cashAfter = integer(accountAfter.cash, `legacy tick ${frame.tick} account cash`);
    const order = restingSell(frame, account, stock, orderId);
    if (order !== null) {
      const filled = integer(order.filled_qty, `legacy tick ${frame.tick} filled quantity`);
      assert(filled >= previousFilled, `legacy tick ${frame.tick} regressed controlled Sell fill quantity`);
      if (filled > previousFilled) {
        const delta = filled - previousFilled;
        const trades = frameFacts(frame).filter(({ variant, payload }) => variant === "Trade"
          && payload.code === stock && String(payload.maker) === account);
        assert.equal(trades.length, 1, `legacy tick ${frame.tick} controlled Sell Trade is absent or ambiguous`);
        assert.equal(integer(trades[0].payload.qty, `legacy tick ${frame.tick} Trade quantity`), delta,
          `legacy tick ${frame.tick} Trade does not match the Sell order fill delta`);
        const fee = validateLegacySellerFeeProjection(frame, previousCash);
        assert.equal(fee.gross, integer(trades[0].payload.price, `legacy tick ${frame.tick} Trade price`) * delta,
          `legacy tick ${frame.tick} Trade gross differs from the sealed fee witness`);
        charged += fee.oldCharged;
        gross += fee.gross;
        fills.push({ order_id: orderId, price_cents: integer(trades[0].payload.price,
          `legacy tick ${frame.tick} Trade price`).toString(), qty: delta.toString() });
        feePrefixes.push({ charged_cents: charged.toString(), nominal_cents: charged.toString() });
        previousFilled = filled;
        if (fills.length === 2 && integer(order.qty, `legacy tick ${frame.tick} live Sell quantity`) > 0n) {
          checkpoint = { frame, order, account: accountAfter };
          break;
        }
      }
    }
    previousCash = cashAfter;
  }
  assert(checkpoint && fills.length >= 2, "controlled legacy Sell lacks two cross-tick partial fills with live remainder");
  assert.notEqual(fills[0].qty, "0", "controlled legacy first fill is empty");
  const checkpointPosition = checkpoint.account.positions?.[stock];
  assert(checkpointPosition, "controlled legacy checkpoint lacks seller cost basis");
  const checkpointGross = fills.reduce((sum, fill) => sum + BigInt(fill.price_cents) * BigInt(fill.qty), 0n);
  assert.equal(checkpointGross, gross, "controlled legacy fill gross differs from fee-prefix gross");
  assert.equal(integer(checkpoint.order.qty, "controlled legacy live remainder") + previousFilled,
    submittedQty, "controlled legacy live/fill quantities do not conserve the submitted Sell");

  const continuation = {
    sealed_exogenous_script_sha256: semanticHash(configuration.sealed_exogenous_script),
    strategy_state_sha256: semanticHash(checkpoint.frame.strategy_profiles),
    plan_state_sha256: semanticHash({ plans: checkpoint.frame.plans,
      pending_plan_events: checkpoint.frame.pending_plan_events }),
    pending_intents_sha256: semanticHash(checkpoint.frame.pending_player),
    restore_order_sha256: semanticHash({ auction_orders: checkpoint.frame.orders.auction,
      resting_orders: checkpoint.frame.orders.resting }),
  };
  const comparisonPoints = surface === "auction-rollover"
    ? ["post-auction-rollover", "post-continuation-tick"]
    : ["post-partial-fill", "post-continuation-tick"];
  return {
    case_id: `${run.scenario}-${run.seed}-${surface}`,
    class: "controlled-live-sell",
    state: { live_shares: integer(checkpoint.order.qty, "controlled legacy live shares").toString(),
      order_id: orderId, reserved_cash_cents: integer(checkpoint.account.reserved_cash,
        "controlled legacy checkpoint reservation").toString() },
    seller_fee_control: {
      charged_final_cents: charged.toString(), fills: fills.slice(0, 2), gross_cents: checkpointGross.toString(),
      invested_cents: exactSignedInteger(checkpointPosition.invested_cents,
        "controlled legacy invested cost").toString(), nominal_final_cents: charged.toString(),
      recovered_cents: exactSignedInteger(checkpointPosition.recovered_cents,
        "controlled legacy recovered value").toString(), terminal_cash_cents: integer(checkpoint.account.cash,
        "controlled legacy checkpoint cash").toString(),
    },
    corpus_control: {
      acceptance: "accepted", comparison_points: comparisonPoints, fee_prefixes: feePrefixes.slice(0, 2),
      feedback: { plan_generated_intents: "0", state_dependent_intents: "0", strategy_generated_intents: "0" },
      old_sell_reservation_cents: legacyReservation.toString(), ...continuation,
      rng_cursor: integer(checkpoint.frame.rng_state, "controlled legacy RNG cursor").toString(),
      seller_order_count: "1", surface, surface_evidence: null, zero_cash_acceptance: false,
    },
  };
}

/**
 * Extract the funded, same-account buyer-fee surface that is actually present
 * in the sealed equivalence run. Trade does not carry order IDs, so this
 * deliberately emits one aggregate Buy surface for all Buy intents in the
 * single sealed self-trade batch. The script, trade gross, frozen v1 fee
 * configuration and total account cash equation must all agree.
 */
export function extractLegacyBuyerFeeSurface(run) {
  assert.equal(run.scenario, "equivalence", "buyer-fees needs the sealed equivalence run");
  const configuration = run.records[0];
  assert.equal(configuration.kind, "configuration", "buyer-fees lacks configuration");
  assert.equal(configuration.class, "i-equivalence", "buyer-fees sealed class mismatch");
  const scriptEntries = configuration.sealed_exogenous_script;
  assert(Array.isArray(scriptEntries) && scriptEntries.length === 1,
    "buyer-fees needs one sealed exogenous batch");
  const [scriptTick, intents] = scriptEntries[0];
  integer(scriptTick, "buyer-fees script tick");
  assert(Array.isArray(intents), "buyer-fees sealed intents must be an array");
  const limits = intents.map((intent) => intent.PlaceLimit).filter(Boolean);
  assert.equal(limits.length, intents.length, "buyer-fees accepts only sealed PlaceLimit intents");
  const sells = limits.filter((order) => order.side === "Sell");
  const buys = limits.filter((order) => order.side === "Buy");
  assert.equal(sells.length, 1, "buyer-fees needs exactly one resting Sell");
  assert(buys.length >= 1, "buyer-fees needs at least one Buy execution leg");
  const stock = sells[0].code;
  assert(/^[0-9]{6}$/.test(stock), "buyer-fees stock code is invalid");
  assert(limits.every((order) => order.code === stock), "buyer-fees intents span multiple stocks");
  const sellQty = integer(sells[0].qty, "buyer-fees Sell quantity");
  const buyQty = buys.reduce((total, order) => total + integer(order.qty, "buyer-fees Buy quantity"), 0n);
  assert.equal(buyQty, sellQty, "buyer-fees Buy and Sell quantities do not close");

  const frames = run.records.filter((record) => record.kind === "TickFrame");
  const executionFrames = frames.filter((frame) => frameFacts(frame).some(({ variant }) => variant === "Trade"));
  assert.equal(executionFrames.length, 1, "buyer-fees trades must occur in exactly one committed frame");
  const frame = executionFrames[0];
  assert.equal(integer(frame.tick, "buyer-fees frame tick"), integer(scriptTick, "buyer-fees script tick") + 1n,
    "buyer-fees frame is not bound to its sealed script tick");
  const frameIndex = frames.indexOf(frame);
  const account = "0";
  const before = frameIndex === 0 ? configuration.initial_state.snapshot.accounts?.[account]
    : frames[frameIndex - 1].snapshot?.accounts?.[account];
  const after = frame.snapshot?.accounts?.[account];
  assert(before && after, "buyer-fees lacks before/after account checkpoints");
  const trades = frameFacts(frame).filter(({ variant, payload }) => variant === "Trade"
    && payload.code === stock && String(payload.maker) === account && String(payload.taker) === account);
  assert.equal(trades.length, buys.length,
    "buyer-fees Trade facts cannot be bound one-for-one to the sealed Buy legs");
  const buyGrosses = buys.map((order, index) => {
    const trade = trades[index].payload;
    const qty = integer(order.qty, `buyer-fees Buy ${index} quantity`);
    const price = integer(order.price, `buyer-fees Buy ${index} price`);
    assert.equal(integer(trade.qty, `buyer-fees Trade ${index} quantity`), qty,
      `buyer-fees Trade ${index} quantity differs from the sealed Buy`);
    assert.equal(integer(trade.price, `buyer-fees Trade ${index} price`), price,
      `buyer-fees Trade ${index} price differs from the sealed Buy`);
    return price * qty;
  });
  const gross = buyGrosses.reduce((total, value) => total + value, 0n);
  assert.equal(gross, integer(sells[0].price, "buyer-fees Sell price") * sellQty,
    "buyer-fees Trade gross differs from the resting Sell gross");

  const config = configuration.setup?.config;
  assert(config, "buyer-fees lacks frozen fee configuration");
  const buyFees = buyGrosses.map((value, index) => legacyFee(config, value, "Buy", `buyer-fees Buy ${index}`));
  const commission = buyFees.reduce((total, fee) => total + fee.commission, 0n);
  const transfer = buyFees.reduce((total, fee) => total + fee.transferFee, 0n);
  const sellFees = legacyFee(config, gross, "Sell", "buyer-fees Sell");
  const observedCashDelta = exactSignedInteger(after.cash, "buyer-fees cash after")
    - exactSignedInteger(before.cash, "buyer-fees cash before");
  const expectedCashDelta = -(commission + transfer + sellFees.commission
    + sellFees.stampTax + sellFees.transferFee);
  assert.equal(observedCashDelta, expectedCashDelta,
    "buyer-fees total self-trade cash equation does not prove the derived fee fields");
  const spent = gross + commission + transfer;
  const evidence = {
    account_id: account, stock_code: stock, side: "Buy", trade_role: "taker-buy",
    gross_cents: gross.toString(), commission_cents: commission.toString(),
    transfer_fee_cents: transfer.toString(), spent_cash_cents: spent.toString(),
  };
  return {
    case_id: `${run.scenario}-${run.seed}-buyer-fees`, class: "equivalence",
    state: { buyer_fee_control: evidence }, seller_fee_control: null,
    corpus_control: {
      surface: "buyer-fees", seller_order_count: "0", old_sell_reservation_cents: "0",
      fee_prefixes: [], feedback: { strategy_generated_intents: "0", plan_generated_intents: "0",
        state_dependent_intents: "0" }, acceptance: "accepted", zero_cash_acceptance: false,
      sealed_exogenous_script_sha256: null, rng_cursor: null, strategy_state_sha256: null,
      plan_state_sha256: null, pending_intents_sha256: null, restore_order_sha256: null,
      comparison_points: ["post-commit"], surface_evidence: evidence,
    },
  };
}

function legacyEquivalenceExecution(run, label) {
  assert.equal(run.scenario, "equivalence", `${label} needs the sealed equivalence run`);
  const configuration = run.records[0];
  assert.equal(configuration.kind, "configuration", `${label} lacks configuration`);
  assert.equal(configuration.class, "i-equivalence", `${label} sealed class mismatch`);
  assert(Array.isArray(configuration.sealed_exogenous_script)
    && configuration.sealed_exogenous_script.length === 1,
  `${label} needs one sealed exogenous batch`);
  const [scriptTick, intents] = configuration.sealed_exogenous_script[0];
  const limits = intents.map((intent) => intent.PlaceLimit).filter(Boolean);
  assert.equal(limits.length, intents.length, `${label} accepts only sealed PlaceLimit intents`);
  assert(limits.length > 1, `${label} needs multiple sealed intents`);
  const stock = limits[0].code;
  assert(/^[0-9]{6}$/.test(stock) && limits.every((order) => order.code === stock),
    `${label} intents do not identify one valid stock`);
  const frames = run.records.filter((record) => record.kind === "TickFrame");
  const executionFrames = frames.filter((frame) => frameFacts(frame).some(({ variant }) => variant === "Trade"));
  assert.equal(executionFrames.length, 1, `${label} trades must occur in exactly one committed frame`);
  const frame = executionFrames[0];
  assert.equal(integer(frame.tick, `${label} frame tick`), integer(scriptTick, `${label} script tick`) + 1n,
    `${label} frame is not bound to its sealed script tick`);
  const frameIndex = frames.indexOf(frame);
  const account = "0";
  const before = frameIndex === 0 ? configuration.initial_state.snapshot.accounts?.[account]
    : frames[frameIndex - 1].snapshot?.accounts?.[account];
  const after = frame.snapshot?.accounts?.[account];
  assert(before && after, `${label} lacks before/after account checkpoints`);
  const trades = frameFacts(frame).filter(({ variant, payload }) => variant === "Trade"
    && payload.code === stock);
  assert(trades.length > 0, `${label} execution frame contains no related Trade`);
  return { configuration, limits, stock, frames, frame, before, after, trades, account };
}

/**
 * Extract the T+1 surface from the real same-account execution in the sealed
 * equivalence run. Both sides of each self-trade affect total quantity, while
 * only the Buy leg increases t1_locked. Keeping sold_qty explicit prevents the
 * quantity equation from silently assuming a different-account counterparty.
 */
export function extractLegacyT1Surface(run) {
  const { limits, stock, before, after, trades, account } = legacyEquivalenceExecution(run, "t1");
  const buyIntents = limits.filter((order) => order.side === "Buy");
  const sellIntents = limits.filter((order) => order.side === "Sell");
  assert(buyIntents.length > 0 && sellIntents.length > 0, "t1 needs both Buy and Sell intents");
  const bought = trades.filter(({ payload }) => String(payload.taker) === account)
    .reduce((total, { payload }) => total + integer(payload.qty, "t1 Buy Trade quantity"), 0n);
  const sold = trades.filter(({ payload }) => String(payload.maker) === account)
    .reduce((total, { payload }) => total + integer(payload.qty, "t1 Sell Trade quantity"), 0n);
  const scriptedBought = buyIntents.reduce((total, order) => total + integer(order.qty, "t1 Buy intent quantity"), 0n);
  const scriptedSold = sellIntents.reduce((total, order) => total + integer(order.qty, "t1 Sell intent quantity"), 0n);
  assert.equal(bought, scriptedBought, "t1 Buy Trade quantity differs from the sealed Buy intents");
  assert.equal(sold, scriptedSold, "t1 Sell Trade quantity differs from the sealed Sell intents");
  const beforePosition = before.positions?.[stock];
  const afterPosition = after.positions?.[stock];
  assert(beforePosition && afterPosition, "t1 lacks before/after position checkpoints");
  const qtyBefore = integer(beforePosition.qty, "t1 quantity before");
  const qtyAfter = integer(afterPosition.qty, "t1 quantity after");
  const lockedBefore = integer(beforePosition.t1_locked, "t1 locked quantity before");
  const lockedAfter = integer(afterPosition.t1_locked, "t1 locked quantity after");
  assert(qtyBefore + bought >= sold, "t1 self-trade quantity equation underflows");
  assert.equal(qtyAfter, qtyBefore + bought - sold,
    "t1 position quantity does not equal before + bought - sold");
  assert.equal(lockedAfter, lockedBefore + bought,
    "t1 bought shares did not become locked exactly once");
  assert(lockedAfter <= qtyAfter, "t1 locked quantity exceeds final position quantity");
  const evidence = {
    account_id: account, stock_code: stock, side: "Buy", trade_role: "taker-buy",
    qty_before: qtyBefore.toString(), bought_qty: bought.toString(), sold_qty: sold.toString(),
    qty_after: qtyAfter.toString(), t1_locked_before: lockedBefore.toString(),
    t1_locked_after: lockedAfter.toString(),
  };
  return {
    case_id: `${run.scenario}-${run.seed}-t1`, class: "equivalence",
    state: { t1_control: evidence }, seller_fee_control: null,
    corpus_control: {
      surface: "t1", seller_order_count: "0", old_sell_reservation_cents: "0",
      fee_prefixes: [], feedback: { strategy_generated_intents: "0", plan_generated_intents: "0",
        state_dependent_intents: "0" }, acceptance: "accepted", zero_cash_acceptance: false,
      sealed_exogenous_script_sha256: null, rng_cursor: null, strategy_state_sha256: null,
      plan_state_sha256: null, pending_intents_sha256: null, restore_order_sha256: null,
      comparison_points: ["post-commit"], surface_evidence: evidence,
    },
  };
}

/**
 * Extract one unambiguous immediate-full-fill Buy leg. The public event stream
 * legally omits OrderAccepted for that Buy, so identity is proved by the
 * canonical ID interval plus the unique Trade quantity/value and terminal
 * absence from the book. The extractor never invents a Trade order ID.
 */
export function extractLegacyContinuousBuyLegSurface(run) {
  const { limits, stock, frame, frames, trades, account } = legacyEquivalenceExecution(run,
    "continuous-buy-leg");
  assert.equal(limits[0].side, "Sell", "continuous-buy-leg expects the resting Sell first");
  const buys = limits.slice(1);
  assert(buys.length > 0 && buys.every((order) => order.side === "Buy"),
    "continuous-buy-leg expects only Buy intents after the resting Sell");
  const frameIndex = frames.indexOf(frame);
  const nextBefore = frameIndex === 0 ? 1n
    : integer(frames[frameIndex - 1].next_order_id, "continuous-buy-leg next_order_id before");
  const nextAfter = integer(frame.next_order_id, "continuous-buy-leg next_order_id after");
  assert.equal(nextAfter, nextBefore + BigInt(limits.length),
    "continuous-buy-leg order-ID interval does not cover every sealed Place");
  const sellAccepted = frameFacts(frame).filter(({ variant, payload }) => variant === "OrderAccepted"
    && String(payload.account) === account && payload.code === stock && payload.side === "Sell");
  assert.equal(sellAccepted.length, 1, "continuous-buy-leg resting Sell acceptance is absent or ambiguous");
  assert.equal(integer(sellAccepted[0].payload.id, "continuous-buy-leg Sell order id"), nextBefore,
    "continuous-buy-leg first canonical ID does not belong to the Sell");

  const selected = buys.map((order, index) => {
    const qty = integer(order.qty, `continuous-buy-leg Buy ${index} quantity`);
    const price = integer(order.price, `continuous-buy-leg Buy ${index} price`);
    const matches = trades.filter(({ payload }) => String(payload.taker) === account
      && integer(payload.qty, `continuous-buy-leg Trade ${index} quantity`) === qty
      && integer(payload.price, `continuous-buy-leg Trade ${index} price`) === price);
    return { order, qty, price, matches, orderId: nextBefore + 1n + BigInt(index) };
  }).find(({ matches }) => matches.length === 1);
  assert(selected, "continuous-buy-leg has no uniquely Trade-bound Buy intent");
  const buyAccepted = frameFacts(frame).filter(({ variant, payload }) => variant === "OrderAccepted"
    && String(payload.account) === account && payload.code === stock && payload.side === "Buy"
    && integer(payload.id, "continuous-buy-leg accepted Buy order id") === selected.orderId);
  assert.equal(buyAccepted.length, 0,
    "continuous-buy-leg sealed witness unexpectedly emitted Buy OrderAccepted");
  const liveOrders = [...(frame.orders?.auction?.[stock] ?? []), ...(frame.orders?.resting?.[stock] ?? [])]
    .filter((order) => String(order.owner) === account && String(order.id) === selected.orderId.toString());
  assert.equal(liveOrders.length, 0, "continuous-buy-leg selected Buy is not terminal after its Trade");
  const evidence = {
    account_id: account, stock_code: stock, side: "Buy", trade_role: "taker-buy",
    order_id: selected.orderId.toString(), trade_leg_count: "1", stable_order_identity_count: "1",
    order_accepted_event_count: "0", terminal_filled_qty: selected.qty.toString(),
    terminal_filled_value_cents: (selected.qty * selected.price).toString(), terminal_live_qty: "0",
    trade_legs: [{ price_cents: selected.price.toString(), qty: selected.qty.toString() }],
  };
  return {
    case_id: `${run.scenario}-${run.seed}-continuous-buy-leg`, class: "equivalence",
    state: { continuous_buy_leg_control: evidence }, seller_fee_control: null,
    corpus_control: {
      surface: "continuous-buy-leg", seller_order_count: "0", old_sell_reservation_cents: "0",
      fee_prefixes: [], feedback: { strategy_generated_intents: "0", plan_generated_intents: "0",
        state_dependent_intents: "0" }, acceptance: "accepted", zero_cash_acceptance: false,
      sealed_exogenous_script_sha256: null, rng_cursor: null, strategy_state_sha256: null,
      plan_state_sha256: null, pending_intents_sha256: null, restore_order_sha256: null,
      comparison_points: ["post-commit"], surface_evidence: evidence,
    },
  };
}

/**
 * Extract the directed price-cage witness. The old P4 rejection does not
 * consume an ID, while approved divergence #3 requires the current rejection
 * to consume exactly one. Both the accepted order and post-frame cursor stay
 * in evidence so the comparator must map the literal +1 differences.
 */
export function extractLegacyPriceCageSurface(run) {
  assert.equal(run.scenario, "equivalence", "price-cage needs the sealed equivalence run");
  assert.equal(run.records[0].class, "i-equivalence", "price-cage sealed class mismatch");
  const witness = oneNamedAuxiliaryRecord(run, "divergence_witness",
    "cage-reject-then-valid-control");
  const facts = frameFacts(witness.frame);
  const rejected = facts.filter(({ variant, payload }) => variant === "IntentRejected"
    && payload.reason === "PriceCageExceeded");
  const accepted = facts.filter(({ variant, payload }) => variant === "OrderAccepted"
    && payload.side === "Buy");
  assert.equal(rejected.length, 1, "price-cage lacks one PriceCageExceeded rejection");
  assert.equal(accepted.length, 1, "price-cage lacks one inside Buy acceptance");
  const account = String(rejected[0].payload.account);
  const stock = rejected[0].payload.code;
  assert.equal(String(accepted[0].payload.account), account,
    "price-cage rejection and acceptance accounts differ");
  assert.equal(accepted[0].payload.code, stock,
    "price-cage rejection and acceptance stocks differ");
  const before = integer(witness.before_next_order_id, "price-cage next_order_id before");
  const inside = integer(accepted[0].payload.id, "price-cage inside order id");
  const after = integer(witness.frame.next_order_id, "price-cage next_order_id after");
  assert.equal(inside, before, "price-cage legacy rejection unexpectedly consumed an ID");
  assert.equal(after, inside + 1n, "price-cage legacy accepted order did not advance the cursor once");
  assert.equal(integer(witness.new_expected_next_order_id, "price-cage current expected cursor"),
    after + 1n, "price-cage sealed current expectation is not the approved #3 +1 cursor");
  const { updates, derivations } = normalizeAuxiliaryFrames([witness.frame], "price-cage");
  const evidence = {
    account_id: account, stock_code: stock, side: "Buy",
    outside_rejection: "PriceCageExceeded", inside_acceptance: "accepted",
    inside_order_id: inside.toString(), next_order_id_before: before.toString(),
    next_order_id_after: after.toString(),
  };
  return {
    surface: {
      case_id: `${run.scenario}-${run.seed}-price-cage`, class: "equivalence",
      state: { price_cage_control: evidence }, seller_fee_control: null,
      corpus_control: {
        surface: "price-cage", seller_order_count: "0", old_sell_reservation_cents: "0",
        fee_prefixes: [], feedback: { strategy_generated_intents: "0", plan_generated_intents: "0",
          state_dependent_intents: "0" }, acceptance: "accepted", zero_cash_acceptance: false,
        sealed_exogenous_script_sha256: null, rng_cursor: null, strategy_state_sha256: null,
        plan_state_sha256: null, pending_intents_sha256: null, restore_order_sha256: null,
        comparison_points: ["post-commit"], surface_evidence: evidence,
      },
    },
    updates,
    provenance: { ...run.provenance,
      auxiliary_records: [{ kind: witness.kind, name: witness.name, sha256: hashValue(witness) }],
      event_key_derivations: derivations },
  };
}

/** Extract the actual zero-cash legacy rejection paired with its funded
 * reservation witness. The positive reservation is evidence from old_funded;
 * the rejection update is old_zero_cash. They are never spliced into one fake
 * runtime frame. */
export function extractLegacyAcceptanceFlipSurface(run) {
  assert.equal(run.scenario, "divergence-9", "acceptance-flip needs the sealed divergence-9 run");
  assert.equal(run.records[0].class, "ii-isolated-9", "acceptance-flip sealed class mismatch");
  const boundary = oneAuxiliaryRecord(run, "acceptance_boundary");
  assert.deepEqual(boundary.allowed_paths, ["events.IntentRejected->OrderAccepted",
    "snapshot.accounts.0.reserved_cash", "orders"], "acceptance-flip allowed path contract changed");
  assert.deepEqual(boundary.new_expected, { accepted: true, reserved_cash: 0 },
    "acceptance-flip current expectation changed");
  const funded = boundary.old_funded;
  const zeroCash = boundary.old_zero_cash;
  assert.equal(integer(funded.tick, "acceptance-flip funded tick"),
    integer(zeroCash.tick, "acceptance-flip zero-cash tick"), "acceptance-flip witness ticks differ");
  const fundedAccount = funded.snapshot?.accounts?.["0"];
  const zeroAccount = zeroCash.snapshot?.accounts?.["0"];
  assert(fundedAccount && zeroAccount, "acceptance-flip account checkpoints are absent");
  assert(integer(fundedAccount.cash, "acceptance-flip funded cash") > 0n,
    "acceptance-flip funded witness is not funded");
  assert.equal(integer(zeroAccount.cash, "acceptance-flip submission cash"), 0n,
    "acceptance-flip zero-cash witness is not zero cash");
  const accepted = frameFacts(funded).filter(({ variant, payload }) => variant === "OrderAccepted"
    && String(payload.account) === "0" && payload.side === "Sell");
  assert.equal(accepted.length, 1, "acceptance-flip funded Sell acceptance is absent or ambiguous");
  const subject = { account_id: "0", stock_code: accepted[0].payload.code, side: "Sell",
    trade_role: "maker-sell" };
  const rejections = frameFacts(zeroCash).filter(({ variant, payload }) => variant === "IntentRejected"
    && String(payload.account) === subject.account_id && payload.code === subject.stock_code
    && (payload.reason === "InsufficientCash" || payload.reason?.InsufficientCash !== undefined));
  assert.equal(rejections.length, 1, "acceptance-flip zero-cash rejection is absent or ambiguous");
  const reservation = integer(fundedAccount.reserved_cash, "acceptance-flip legacy reservation");
  assert(reservation > 0n, "acceptance-flip funded reservation is not positive");
  assert.equal(integer(fundedAccount.reserved_sell_qty?.[subject.stock_code],
    "acceptance-flip funded reserved shares"),
  integer(accepted[0].payload.remaining_qty, "acceptance-flip accepted shares"),
  "acceptance-flip funded reserved shares differ from the accepted Sell");
  assert.equal(integer(boundary.owned_sellable_shares, "acceptance-flip owned shares"),
    integer(zeroAccount.positions?.[subject.stock_code]?.qty, "acceptance-flip zero-cash position"),
    "acceptance-flip owned-share witness differs from the zero-cash position");
  const { updates, derivations } = normalizeAuxiliaryFrames([zeroCash], "acceptance-flip");
  const surface = {
    case_id: `${run.scenario}-${run.seed}-acceptance-flip`, class: "divergence-9",
    state: { reserved_cash: reservation.toString(), acceptance: "rejected" }, seller_fee_control: null,
    corpus_control: sellerCorpusControl("acceptance-flip", reservation, [], "rejected", true, subject),
  };
  return { surface, updates, provenance: { ...run.provenance,
    auxiliary_kind: boundary.kind, auxiliary_sha256: hashValue(boundary), event_key_derivations: derivations } };
}

/** Extract the real independent seller's three legacy fill legs. Every leg is
 * checked against its Trade, account cash, position and live-order deltas. */
export function extractLegacyThreeLegFeeCatchupSurface(run) {
  assert.equal(run.scenario, "divergence-9", "three-leg fee catchup needs the sealed divergence-9 run");
  const configuration = run.records[0];
  assert.equal(configuration.class, "ii-isolated-9", "three-leg fee catchup sealed class mismatch");
  const witness = oneAuxiliaryRecord(run, "independent_seller_fee_control");
  const expectation = oneAuxiliaryRecord(run, "fee_expectation");
  const seller = String(integer(witness.seller, "three-leg seller"));
  const buyer = String(integer(witness.buyer, "three-leg buyer"));
  assert.notEqual(seller, buyer, "three-leg witness must use independent seller and buyer accounts");
  assert(integer(witness.npc_attention_disabled_until_tick, "three-leg NPC disable bound")
    > BigInt(witness.legs.at(-1)?.frame?.tick ?? 0), "three-leg witness does not disable NPC feedback");
  assert.equal(witness.legs.length, 3, "three-leg witness must contain exactly three legs");
  const initialOrderEntries = Object.entries(witness.initial_state?.resting_orders ?? {});
  assert.equal(initialOrderEntries.length, 1, "three-leg witness needs one stock order book");
  const [stock, initialOrders] = initialOrderEntries[0];
  assert(/^[0-9]{6}$/.test(stock), "three-leg stock code is invalid");
  const sellerOrders = initialOrders.filter((order) => String(order.owner) === seller && order.side === "Sell");
  assert.equal(sellerOrders.length, 1, "three-leg witness needs one initial seller order");
  const orderId = String(integer(sellerOrders[0].id, "three-leg seller order id"));
  const initialQty = integer(sellerOrders[0].qty, "three-leg initial live quantity");
  let cumulativeGross = 0n;
  let cumulativeCharged = 0n;
  const prefixes = [];
  const feeLegs = [];
  const frames = [];
  const config = configuration.setup?.config;
  assert(config, "three-leg witness lacks frozen fee configuration");
  for (const [index, leg] of witness.legs.entries()) {
    const facts = frameFacts(leg.frame);
    const trades = facts.filter(({ variant, payload }) => variant === "Trade" && payload.code === stock
      && String(payload.maker) === seller && String(payload.taker) === buyer);
    assert.equal(trades.length, 1, `three-leg ${index} Trade is absent or ambiguous`);
    const trade = trades[0].payload;
    const gross = integer(trade.price, `three-leg ${index} Trade price`)
      * integer(trade.qty, `three-leg ${index} Trade quantity`);
    assert.equal(gross, integer(leg.gross_cents, `three-leg ${index} sealed gross`),
      `three-leg ${index} Trade gross differs from the witness`);
    const charged = integer(leg.observed_fee_cents, `three-leg ${index} charged fee`);
    const net = exactSignedInteger(leg.observed_net_cents, `three-leg ${index} net delivery`);
    assert.equal(net, gross - charged, `three-leg ${index} net/fee equation`);
    feeLegs.push({ charged_cents: charged.toString(), net_delivery_cents: net.toString() });
    assert.equal(exactSignedInteger(leg.seller_after.cash, `three-leg ${index} seller cash after`)
      - exactSignedInteger(leg.seller_before.cash, `three-leg ${index} seller cash before`), net,
    `three-leg ${index} seller cash delta`);
    const beforePosition = leg.seller_before.positions?.[stock];
    const afterPosition = leg.seller_after.positions?.[stock];
    assert(beforePosition && afterPosition, `three-leg ${index} seller position is absent`);
    assert.equal(integer(beforePosition.qty, `three-leg ${index} quantity before`)
      - integer(afterPosition.qty, `three-leg ${index} quantity after`), integer(trade.qty, `three-leg ${index} Trade quantity`),
    `three-leg ${index} position quantity delta`);
    assert.equal(exactSignedInteger(afterPosition.recovered_cents, `three-leg ${index} recovered after`)
      - exactSignedInteger(beforePosition.recovered_cents, `three-leg ${index} recovered before`), gross,
    `three-leg ${index} recovered-cost delta`);
    cumulativeGross += gross;
    cumulativeCharged += charged;
    const nominalFees = legacyFee(config, cumulativeGross, "Sell", `three-leg ${index} cumulative fee`);
    prefixes.push({ nominal_cents: (nominalFees.commission + nominalFees.stampTax
      + nominalFees.transferFee).toString(), charged_cents: cumulativeCharged.toString() });
    const liveOrders = leg.frame.orders?.resting?.[stock] ?? [];
    const live = liveOrders.find((order) => String(order.id) === orderId && String(order.owner) === seller
      && order.side === "Sell");
    const cumulativeQty = witness.legs.slice(0, index + 1).reduce((total, item) => total
      + integer(frameFacts(item.frame).find(({ variant }) => variant === "Trade").payload.qty,
        `three-leg ${index} cumulative Trade quantity`),
    0n);
    if (cumulativeQty === initialQty) assert.equal(live, undefined, "terminal three-leg order remained live");
    else {
      assert(live, `three-leg ${index} live remainder is absent`);
      assert.equal(integer(live.qty, `three-leg ${index} live remainder`) + cumulativeQty, initialQty,
        `three-leg ${index} order quantity does not conserve`);
    }
    frames.push(leg.frame);
  }
  assert.deepEqual(expectation.gross_legs_cents.map(String), witness.legs.map((leg) => String(leg.gross_cents)),
    "three-leg fee expectation gross changed");
  assert.deepEqual(expectation.old_charged_legs_cents.map(String), witness.legs.map((leg) => String(leg.observed_fee_cents)),
    "three-leg fee expectation charged legs changed");
  assert.deepEqual(expectation.old_net_legs_cents.map(String), witness.legs.map((leg) => String(leg.observed_net_cents)),
    "three-leg fee expectation net legs changed");
  assert.deepEqual(expectation.nominal_cumulative_cents.map(String), prefixes.map((prefix) => prefix.nominal_cents),
    "three-leg nominal cumulative fees differ from the frozen fee formula");
  const reservation = integer(witness.legs[0].seller_before.reserved_cash,
    "three-leg legacy Sell reservation");
  assert(reservation > 0n, "three-leg witness lacks the positive legacy Sell reservation");
  const { updates, derivations } = normalizeAuxiliaryFrames(frames, "three-leg-fee-catchup");
  const netDelivery = witness.legs.reduce((total, leg) => total
    + exactSignedInteger(leg.observed_net_cents, "three-leg net delivery"), 0n);
  const surface = {
    case_id: `${run.scenario}-${run.seed}-three-leg-fee-catchup`, class: "divergence-9",
    state: { reserved_cash_cents: String(witness.legs.at(-1).seller_after.reserved_cash),
      charged_total_cents: cumulativeCharged.toString(), net_delivery_cents: netDelivery.toString(),
      fee_legs: feeLegs },
    seller_fee_control: null,
    corpus_control: sellerCorpusControl("three-leg-fee-catchup", reservation, prefixes, "accepted", false),
  };
  return { surface, updates, provenance: { ...run.provenance,
    auxiliary_kind: witness.kind, auxiliary_sha256: hashValue(witness),
    fee_expectation_sha256: hashValue(expectation), event_key_derivations: derivations } };
}

// The adapter does not manufacture the surface's mechanical assertions. They
// must come from a separately reviewed extraction of that sealed run, and every
// semantic checkpoint is retained even when a surface uses only a subset.
export function assembleLegacyProjection(run, surfaceEvidence) {
  assert.deepEqual(Object.keys(surfaceEvidence).sort(), ["case_id", "class", "corpus_control", "seller_fee_control", "state"], "surface evidence fields");
  assert.notEqual(surfaceEvidence.class, "stress", "stress is new-engine-only; historical equivalence is unavailable");
  const classes = { "i-equivalence": "equivalence", "ii-isolated-9": "divergence-9", "iv-controlled-live-sell": "controlled-live-sell" };
  assert.equal(surfaceEvidence.class, classes[run.records[0].class], "surface class does not match sealed construction");
  if (surfaceEvidence.class === "controlled-live-sell"
    && ["auction-rollover", "cross-tick-partial-fill", "save-restore-live-order"].includes(surfaceEvidence.corpus_control.surface)) {
    assert.deepEqual(surfaceEvidence,
      extractLegacyControlledSellSurface(run, surfaceEvidence.corpus_control.surface),
      "controlled surface evidence differs from the reviewed extraction");
  }
  if (surfaceEvidence.class === "equivalence"
    && surfaceEvidence.corpus_control.surface === "buyer-fees") {
    assert.deepEqual(surfaceEvidence, extractLegacyBuyerFeeSurface(run),
      "buyer-fees surface evidence differs from the sealed fee/cash extraction");
  }
  if (surfaceEvidence.class === "equivalence"
    && surfaceEvidence.corpus_control.surface === "t1") {
    assert.deepEqual(surfaceEvidence, extractLegacyT1Surface(run),
      "t1 surface evidence differs from the sealed self-trade extraction");
  }
  if (surfaceEvidence.class === "equivalence"
    && surfaceEvidence.corpus_control.surface === "continuous-buy-leg") {
    assert.deepEqual(surfaceEvidence, extractLegacyContinuousBuyLegSurface(run),
      "continuous-buy-leg evidence differs from the sealed immediate-fill extraction");
  }
  if (surfaceEvidence.class === "equivalence"
    && surfaceEvidence.corpus_control.surface === "price-cage") {
    const extracted = extractLegacyPriceCageSurface(run);
    assert.deepEqual(surfaceEvidence, extracted.surface,
      "price-cage evidence differs from the sealed directed extraction");
    return { projection: { schema: "escrow-corpus-projection-v1", scenario: run.scenario,
      seed: run.seed, updates: extracted.updates, ...surfaceEvidence }, provenance: extracted.provenance };
  }
  if (surfaceEvidence.class === "divergence-9"
    && ["acceptance-flip", "three-leg-fee-catchup"].includes(surfaceEvidence.corpus_control.surface)) {
    const extracted = surfaceEvidence.corpus_control.surface === "acceptance-flip"
      ? extractLegacyAcceptanceFlipSurface(run) : extractLegacyThreeLegFeeCatchupSurface(run);
    assert.deepEqual(surfaceEvidence, extracted.surface,
      `${surfaceEvidence.corpus_control.surface} evidence differs from the sealed auxiliary extraction`);
    return { projection: { schema: "escrow-corpus-projection-v1", scenario: run.scenario,
      seed: run.seed, updates: extracted.updates, ...surfaceEvidence }, provenance: extracted.provenance };
  }
  if (surfaceEvidence.corpus_control.surface === "normal-multi-leg-terminal"
    && surfaceEvidence.corpus_control.zero_cash_acceptance) {
    const configuration = run.records[0];
    const sellCommands = configuration.sealed_exogenous_script.flatMap(([, intents]) => intents)
      .filter((intent) => intent.PlaceLimit?.side === "Sell");
    assert.equal(sellCommands.length, 1, "normal seller extraction requires exactly one external Sell");
    // This sealed harness routes its exogenous script through player AccountId(0).
    assert.equal(configuration.initial_state.snapshot.accounts["0"].cash, 0,
      "sealed normal seller run did not observe zero-cash submission; additional evidence is required");
  }
  const adapted = adaptLegacyStream(run);
  assert(!Object.hasOwn(surfaceEvidence.state, "legacy_checkpoints"), "surface state cannot replace legacy checkpoints");
  return { projection: { schema: "escrow-corpus-projection-v1", case_id: surfaceEvidence.case_id,
    scenario: run.scenario, seed: run.seed, class: surfaceEvidence.class, updates: adapted.updates,
    state: { ...surfaceEvidence.state, legacy_checkpoints: adapted.state },
    seller_fee_control: surfaceEvidence.seller_fee_control, corpus_control: surfaceEvidence.corpus_control },
  provenance: adapted.provenance };
}
