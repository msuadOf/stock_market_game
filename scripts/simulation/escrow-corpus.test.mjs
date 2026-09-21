import assert from "node:assert/strict";
import { test } from "node:test";
import { mkdtemp, mkdir, writeFile, rm } from "node:fs/promises";
import { createHash } from "node:crypto";
import path from "node:path";
import {
  adaptLegacyStream,
  assembleLegacyProjection,
  currentReplayRequest,
  extractLegacyAcceptanceFlipSurface,
  extractLegacyBuyerFeeSurface,
  extractLegacyContinuousBuyLegSurface,
  extractLegacyControlledSellSurface,
  extractLegacyPriceCageSurface,
  extractLegacyT1Surface,
  extractLegacyThreeLegFeeCatchupSurface,
  loadSealedCorpusRun,
} from "./escrow-corpus-adapter.mjs";
import { compareCapturedPrimaryStream, compareExactCorpusCase, corpusUnmappedDiff } from "./escrow-corpus-exact.mjs";

function projection() {
  return {
    schema: "escrow-corpus-projection-v1", case_id: "real-fact-shape", scenario: "controlled", seed: "1", class: "equivalence",
    updates: [{ kind: "TickFrame", tick: "1", seq_from: "1", seq_to: "3", timeseries_payload: {}, events: [
      { comparison_event_key: ["1", "4:Trade", "Stock:600001", "0"], event: { Trade: { seq: "1", code: "600001", maker: "1", taker: "2", price: "1000", qty: "100" } } },
      { comparison_event_key: ["1", "4:Trade", "Stock:600001", "1"], event: { Trade: { seq: "2", code: "600001", maker: "1", taker: "2", price: "1000", qty: "200" } } },
      { comparison_event_key: ["1", "4:PriceTick", "Stock:600001", "0"], event: { PriceTick: { seq: "3", code: "600001", last_price: "1000", tick: "1" } } },
    ] }], state: { invested_cents: "1000", recovered_cents: "0", orders: [{ id: "1", seq: "11" }] }, seller_fee_control: null,
    corpus_control: {
      surface: "normal-multi-leg-terminal", seller_order_count: "1", old_sell_reservation_cents: "0",
      fee_prefixes: [{ nominal_cents: "551", charged_cents: "551" }, { nominal_cents: "653", charged_cents: "653" }],
      feedback: { strategy_generated_intents: "0", plan_generated_intents: "0", state_dependent_intents: "0" },
      acceptance: "accepted", zero_cash_acceptance: true, sealed_exogenous_script_sha256: null, rng_cursor: null,
      strategy_state_sha256: null, plan_state_sha256: null, pending_intents_sha256: null, restore_order_sha256: null,
      comparison_points: ["post-commit"], surface_evidence: null,
    },
  };
}

test("corpus_unmapped_diff rejects deletion, same-variant fixed-identity swap and unmapped mutation", () => {
  const baseline = projection();
  const result = corpusUnmappedDiff(baseline, structuredClone(baseline));
  assert.equal(result.reorder_accepted, true);
  assert.deepEqual(result.negatives, { deleted_event: "rejected", fixed_identity_payload_swap: "rejected", unmapped_payload_mutation: "rejected" });
});

test("empty committed ticks retain tick and seq coverage", () => {
  const baseline = projection();
  baseline.updates.push({ kind: "TickFrame", tick: "2", seq_from: "4", seq_to: "3", timeseries_payload: {}, events: [] });
  const third = structuredClone(baseline.updates[0]);
  third.tick = "3"; third.seq_from = "4"; third.seq_to = "6";
  third.events.forEach((fact, index) => { fact.comparison_event_key[0] = "3"; Object.values(fact.event)[0].seq = String(index + 4); });
  baseline.updates.push(third);
  assert.equal(compareExactCorpusCase(baseline, structuredClone(baseline)).ticks.length, 3);
  const gap = structuredClone(baseline);
  gap.updates[1].seq_from = "5";
  assert.throws(() => compareExactCorpusCase(baseline, gap), /cardinality|gap/);
  const deleted = structuredClone(baseline);
  deleted.updates.splice(1, 1);
  assert.throws(() => compareExactCorpusCase(baseline, deleted), /tick gap/);
});

function catchup() {
  const legacy = projection();
  legacy.class = "divergence-9";
  legacy.corpus_control.surface = "three-leg-fee-catchup";
  legacy.corpus_control.zero_cash_acceptance = false;
  legacy.corpus_control.fee_prefixes = [500, 500, 501].map((value) => ({ nominal_cents: String(value), charged_cents: String(value) }));
  legacy.state.charged_total_cents = "501";
  legacy.state.net_delivery_cents = "699";
  const current = structuredClone(legacy);
  current.corpus_control.fee_prefixes.forEach((prefix, index) => { prefix.charged_cents = ["100", "200", "501"][index]; });
  // A checkpoint before final catch-up: exact values intentionally differ.
  legacy.state.charged_checkpoint_cents = "500";
  current.state.charged_checkpoint_cents = "100";
  legacy.state.net_cash_cents = "-400";
  current.state.net_cash_cents = "0";
  const mappings = [
    { path: "/state/charged_checkpoint_cents", divergence: 9, effect: "fee_charged", legacy: "500", current: "100" },
    { path: "/state/net_cash_cents", divergence: 9, effect: "net_delivery", legacy: "-400", current: "0" },
  ];
  return { legacy, current, mappings };
}

test("#9 exact mappings accept sealed negative legacy net and reject fee-value forgery", () => {
  const { legacy, current, mappings } = catchup();
  assert.equal(compareExactCorpusCase(legacy, current, mappings).differences.length, 2);
  const forged = structuredClone(current);
  forged.state.charged_checkpoint_cents = "99";
  assert.throws(() => compareExactCorpusCase(legacy, forged, mappings), /unmapped corpus current value/);
  for (const field of ["invested_cents", "recovered_cents"]) {
    const corrupt = structuredClone(current); corrupt.state[field] = "9";
    assert.throws(() => compareExactCorpusCase(legacy, corrupt, mappings), /unmapped/);
  }
  const fifo = structuredClone(current); fifo.state.orders[0].seq = "12";
  assert.throws(() => compareExactCorpusCase(legacy, fifo, mappings), /unmapped/);
});

test("exact mappings reject wildcard, subtree, duplicate, stale and wrong-old values", () => {
  const { legacy, current, mappings } = catchup();
  for (const invalid of [
    [{ ...mappings[0], path: "/state/*" }, mappings[1]],
    [{ ...mappings[0], current: { arbitrary: "subtree" } }, mappings[1]],
    [...mappings, mappings[0]],
    [{ ...mappings[0], legacy: "499" }, mappings[1]],
    [...mappings, { divergence: 9, effect: "fee_charged", path: "/state/charged_total_cents", legacy: "500", current: "501" }],
  ]) assert.throws(() => compareExactCorpusCase(legacy, current, invalid));
});

test("controlled #9 terminal cash and #7 metadata cannot hide cost-chain or restore-order corruption", () => {
  const legacy = projection();
  legacy.class = "controlled-live-sell";
  Object.assign(legacy.corpus_control, {
    surface: "save-restore-live-order", zero_cash_acceptance: false,
    fee_prefixes: [{ nominal_cents: "500", charged_cents: "500" }],
    comparison_points: ["pre-save", "post-restore", "post-continuation-tick"],
    sealed_exogenous_script_sha256: "a".repeat(64), strategy_state_sha256: "b".repeat(64),
    plan_state_sha256: "c".repeat(64), pending_intents_sha256: "d".repeat(64), restore_order_sha256: "e".repeat(64),
  });
  legacy.seller_fee_control = { gross_cents: "100", nominal_final_cents: "500", charged_final_cents: "500",
    terminal_cash_cents: "1000", invested_cents: "1000", recovered_cents: "100", fills: [{ order_id: "1", qty: "100", price_cents: "1" }] };
  legacy.state.save_representation = { schema: "v1" };
  legacy.state.legacy_checkpoints = { checkpoints: [{ snapshot: { accounts: { 0: { cash: "1000" } } } }] };
  const current = structuredClone(legacy);
  current.corpus_control.fee_prefixes[0].charged_cents = "100";
  current.seller_fee_control.charged_final_cents = "100";
  current.seller_fee_control.terminal_cash_cents = "1400";
  current.state.save_representation.schema = "v2";
  current.state.legacy_checkpoints.checkpoints[0].snapshot.accounts[0].cash = "1400";
  const mappings = [
    { divergence: 9, effect: "fee_charged", path: "/seller_fee_control/charged_final_cents", legacy: "500", current: "100" },
    { divergence: 9, effect: "terminal_cash_equation", path: "/seller_fee_control/terminal_cash_cents", legacy: "1000", current: "1400" },
    { divergence: 9, effect: "terminal_cash_equation", path: "/state/legacy_checkpoints/checkpoints/0/snapshot/accounts/0/cash", legacy: "1000", current: "1400" },
    { divergence: 7, effect: "save_representation", path: "/state/save_representation/schema", legacy: "v1", current: "v2" },
  ];
  assert.equal(compareExactCorpusCase(legacy, current, mappings).differences.length, 4);
  const badCash = structuredClone(current); badCash.seller_fee_control.terminal_cash_cents = "1401";
  assert.throws(() => compareExactCorpusCase(legacy, badCash, mappings), /uncollected final fee/);
  const badOrder = structuredClone(current); badOrder.corpus_control.restore_order_sha256 = "f".repeat(64);
  assert.throws(() => compareExactCorpusCase(legacy, badOrder, mappings), /restore_order/);
  const badSlot = structuredClone(current); badSlot.state.save_slot = { invested_cents: "999" };
  assert.throws(() => compareExactCorpusCase(legacy, badSlot, [...mappings, {
    divergence: 7, effect: "save_representation", path: "/state/save_slot/invested_cents", legacy: { absent: true }, current: "999",
  }]), /representation metadata/);
});

function legacyRun() {
  const p = projection();
  const frame = {
    kind: "TickFrame", tick: 1, seq_from: 1, seq_to: 3, snapshot: { seq: 3, phase: "Continuous", accounts: {} },
    orders: [{ id: 1, seq: 11 }], plans: {}, pending_player: [], rng_state: "1", next_order_id: "2",
    timeseries_payload: { markets: {}, active_daily_candles: {}, daily_candles: {}, points: {} },
    events: p.updates[0].events.map(({ comparison_event_key: key, event }, index) => {
      const variant = Object.keys(event)[0];
      const payload = structuredClone(Object.values(event)[0]);
      payload.seq = index + 1;
      if (variant === "PriceTick") { payload.daily_candle = { volume: 300 }; payload.bids = []; payload.asks = []; }
      return { identity: [1, variant, key[2], variant === "Trade" ? "1:2" : "600001", variant === "Trade" ? index : 0],
        canonical_session_ordinal: null, event: { [variant]: payload } };
    }),
  };
  return { scenario: "equivalence", seed: "1", provenance: {}, records: [
    { kind: "configuration", seed: "1", class: "i-equivalence", initial_state: { snapshot: { seq: 0 }, orders: [{ seq: 11 }] }, sealed_exogenous_script: [] },
    frame, { kind: "terminal", state: { snapshot: { seq: 3 }, rng_state: "1" } },
  ] };
}

test("legacy 5-part identities derive stable 4-part keys and retain all checkpoints/FIFO", () => {
  const run = legacyRun();
  const result = adaptLegacyStream(run);
  assert.deepEqual(result.updates[0].events[1].comparison_event_key, ["1", "4:Trade", "Stock:600001", "1"]);
  assert.equal(result.state.checkpoints[0].orders[0].seq, "11");
  assert.equal(Object.hasOwn(result.state.checkpoints[0].snapshot, "seq"), false);
  assert.equal(result.provenance.event_key_derivations.length, 3);
  const reordered = structuredClone(run); reordered.records[1].events.reverse();
  assert.deepEqual(adaptLegacyStream(reordered), result);
  const swapped = structuredClone(run);
  swapped.records[1].events[0].event.Trade.maker = "8";
  assert.throws(() => adaptLegacyStream(swapped), /stable identity/);
});

test("common checkpoint normalization removes only named representation fields, including restore wrappers", () => {
  const run = legacyRun();
  run.records[0].initial_state.setup = { simulation_policy_id: "a-share-simulation-v1", t1_enabled: true };
  run.records.push({ kind: "before_save", tick: 1, state: {
    snapshot: { seq: 3, accounts: { 0: { cash: 10 } } },
    setup: { simulation_policy_id: "a-share-simulation-v1", t1_enabled: true },
    resting_orders: [{ id: 1, seq: 11 }],
  } });
  const adapted = adaptLegacyStream(run);
  assert.deepEqual(adapted.state.initial.setup, { t1_enabled: true });
  assert.equal(Object.hasOwn(adapted.state.checkpoints[0], "timeseries_payload"), false);
  assert(adapted.updates[0].timeseries_payload.continuous_points["600001"]);
  assert.deepEqual(adapted.state.restore_checkpoints[0].state, {
    snapshot: { accounts: { 0: { cash: "10" } } }, setup: { t1_enabled: true },
    resting_orders: [{ id: "1", seq: "11" }],
  });
});

test("a last continuous PriceTick is not relabelled with the next snapshot phase", () => {
  const run = legacyRun();
  run.records[1].snapshot.phase = "ClosingAuction";
  assert.equal(adaptLegacyStream(run).updates[0].timeseries_payload.continuous_points["600001"].phase, "Continuous");
});

test("legacy fee harness projections are hashed then removed from common semantic checkpoints", () => {
  const run = legacyRun();
  run.records[1].seller_fee_projection = {
    gross_cents: 100, new_expected_charged_cents: 100, new_expected_net_cents: 0,
    observed_combined_account_cash_delta: -1000, old_charged_cents: 500,
    old_net_cents: -400, unchanged_counterparty_buy_fee_cents: 500,
  };
  const adapted = adaptLegacyStream(run);
  assert.equal(Object.hasOwn(adapted.state.checkpoints[0], "seller_fee_projection"), false);
  assert.deepEqual(adapted.provenance.seller_fee_projection_rows, [{
    tick: "1", sha256: "85afeacfdb6fb043485eb98c16d6091cc79a42b350f01762d2bbd75c229cc8b8",
  }]);
  assert(adapted.provenance.representation_normalization.some((entry) => entry.includes("seller_fee_projection")));
});

test("primary capture matching never claims surface acceptance and reports non-fee corruption", () => {
  const run = legacyRun();
  const adapted = adaptLegacyStream(run);
  const current = { schema: "escrow-current-corpus-run-v1", scenario: run.scenario, seed: run.seed,
    updates: adapted.updates, state: adapted.state, historical_comparison_performed: false,
    conservation: [{ schema: "escrow-conservation-snapshot-v1", scenario: run.scenario, seed: run.seed, tick: "1", envelopes: [],
      accounts: [{ account_id: "0", cash_cents: "10", positions: [], aggregate: {
        left: { cash_cents: "0", shares: "0" }, right: { cash_cents: "0", shares: "0" },
      } }],
    }],
  };
  assert.equal(compareCapturedPrimaryStream(run, current).status, "CAPTURE_MATCH");
  assert.equal(compareCapturedPrimaryStream(run, current).task9_acceptance, false);
  const corrupted = structuredClone(current); corrupted.state.checkpoints[0].orders[0].seq = "12";
  assert.deepEqual(compareCapturedPrimaryStream(run, corrupted).differences, [
    { path: "/state/checkpoints/0/orders/0/seq", legacy: "11", current: "12" },
  ]);
  corrupted.conservation = [];
  assert.throws(() => compareCapturedPrimaryStream(run, corrupted), /conservation/);
});

test("current replay request uses only fresh setup, initial allocation and the sealed external script", () => {
  const run = legacyRun();
  run.records[0].setup = { simulation_policy_id: "a-share-simulation-v1", npcs: { retail_count: 0, inst_count: 0, hot_count: 0 } };
  run.records[0].initial_state.snapshot.accounts = { 0: { cash: 1000 } };
  const request = currentReplayRequest(run);
  assert.equal(request.setup.simulation_policy_id, "a-share-simulation-v2");
  assert.equal(run.records[0].setup.simulation_policy_id, "a-share-simulation-v1");
  assert.deepEqual(Object.keys(request).sort(), ["schema", "scenario", "seed", "setup", "initial_accounts", "sealed_exogenous_script", "ticks", "restore_at_ticks"].sort());
  assert.equal(request.ticks, 1);
  assert.equal(Object.hasOwn(request, "rng_state"), false);
  const npc = structuredClone(run); npc.records[0].setup.npcs.retail_count = 1;
  assert.throws(() => currentReplayRequest(npc), /state-dependent/);
  const gap = structuredClone(run); gap.records[1].tick = 2;
  assert.throws(() => currentReplayRequest(gap), /without gaps/);
});

test("controlled representation request carries only the sealed legacy Sell reservation hint", () => {
  const run = legacyRun();
  run.scenario = "representation";
  run.records[0].class = "iv-controlled-live-sell";
  run.records[0].setup = { simulation_policy_id: "a-share-simulation-v1",
    npcs: { retail_count: 0, inst_count: 0, hot_count: 0 } };
  run.records[0].initial_state.snapshot.accounts = { 0: { cash: 1000 } };
  run.records[0].sealed_exogenous_script = [[0, [{ PlaceLimit: {
    code: "600001", side: "Sell", price: 1, qty: 1200,
  } }]]];
  run.records[1].snapshot.accounts = { 0: {
    cash: 1000, reserved_cash: 499, reserved_sell_qty: { 600001: 1200 },
  } };
  assert.deepEqual(currentReplayRequest(run).historical_surface_hints, {
    controlled_sell: { account: "0", stock: "600001", legacy_sell_reservation_cents: "499" },
  });
  const missing = structuredClone(run);
  missing.records[1].snapshot.accounts[0].reserved_sell_qty = {};
  assert.throws(() => currentReplayRequest(missing), /historical live Sell/);
});

function controlledSurfaceRun() {
  const account = (cash, reservedCash, reservedShares, invested, recovered, locked) => ({
    cash, positions: { 600001: { invested_cents: invested, qty: 2400, recovered_cents: recovered, t1_locked: locked } },
    reserved_cash: reservedCash, reserved_sell_qty: reservedShares === 0 ? {} : { 600001: reservedShares },
  });
  const order = (filled, live) => ({
    id: 1, owner: 0, side: "Sell", price: 1, qty: live, original_qty: 1200,
    filled_qty: filled, filled_value: filled, seq: 0,
  });
  const common = {
    next_order_id: "2", plans: { next_plan_seq: 0, plans: {}, policy: {
      reverse_revision_threshold_bp: 2000, review_price_change_bp: 200, review_signal_delta_bp: 1000,
    } }, pending_plan_events: [], pending_player: [], strategy_profiles: {}, rng_state: "1",
  };
  const frame = (tick, cash, reservedCash, reservedShares, filled, events, fee = null) => ({
    kind: "TickFrame", tick, events, snapshot: { accounts: { 0: account(cash, reservedCash, reservedShares,
      2400 + filled, filled, filled) } },
    orders: { auction: {}, resting: { 600001: [order(filled, 1200 - filled)] } },
    seller_fee_projection: fee, ...common,
  });
  const script = [
    [0, [{ PlaceLimit: { code: "600001", price: 1, qty: 1200, side: "Sell" } }]],
    [4, [{ PlaceLimit: { code: "600001", price: 1, qty: 100, side: "Buy" } }]],
    [5, [{ PlaceLimit: { code: "600001", price: 1, qty: 100, side: "Buy" } }]],
    [6, [{ PlaceLimit: { code: "600001", price: 1, qty: 1000, side: "Buy" } }]],
  ];
  const records = [
    { kind: "configuration", class: "iv-controlled-live-sell", seed: "1",
      setup: { npcs: { retail_count: 0, inst_count: 0, hot_count: 0 } },
      initial_state: { snapshot: { accounts: { 0: account(10_000_000, 0, 0, 2400, 0, 0) } } },
      sealed_exogenous_script: script },
    frame(1, 10_000_000, 499, 1200, 0, [{ event: { OrderAccepted: {
      account: 0, code: "600001", id: 1, price: 1, remaining_qty: 1200, seq: 1, side: "Sell",
    } } }]),
    frame(2, 10_000_000, 499, 1200, 0, [{ event: { AuctionCompleted: {
      clearing_price: null, code: "600001", matched_volume: 0, phase: "CallAuction", seq: 2, tick: 2,
    } } }]),
    frame(5, 9_999_000, 0, 1100, 100, [{ event: { Trade: {
      code: "600001", maker: 0, price: 1, qty: 100, seq: 3, taker: 0,
    } } }], { gross_cents: 100, new_expected_charged_cents: 100, new_expected_net_cents: 0,
      observed_combined_account_cash_delta: -1000, old_charged_cents: 500, old_net_cents: -400,
      unchanged_counterparty_buy_fee_cents: 500 }),
    frame(6, 9_998_500, 0, 1000, 200, [{ event: { Trade: {
      code: "600001", maker: 0, price: 1, qty: 100, seq: 4, taker: 0,
    } } }], { gross_cents: 100, new_expected_charged_cents: 100, new_expected_net_cents: 0,
      observed_combined_account_cash_delta: -500, old_charged_cents: 0, old_net_cents: 100,
      unchanged_counterparty_buy_fee_cents: 500 }),
  ];
  records[1].orders = { auction: { 600001: [{ arrival_seq: 1, limit: 1, owner: 0, qty: 1200,
    side: "Sell" }] }, resting: { 600001: [] } };
  return { scenario: "representation", seed: "1", provenance: {}, records };
}

test("controlled legacy extractor derives all reviewed representation surfaces without inventing receipts", () => {
  const run = controlledSurfaceRun();
  const auction = extractLegacyControlledSellSurface(run, "auction-rollover");
  const crossTick = extractLegacyControlledSellSurface(run, "cross-tick-partial-fill");
  assert.deepEqual(auction.state, { live_shares: "1000", order_id: "1", reserved_cash_cents: "0" });
  assert.deepEqual(auction.seller_fee_control, {
    charged_final_cents: "500",
    fills: [
      { order_id: "1", price_cents: "1", qty: "100" },
      { order_id: "1", price_cents: "1", qty: "100" },
    ],
    gross_cents: "200", invested_cents: "2600", nominal_final_cents: "500",
    recovered_cents: "200", terminal_cash_cents: "9998500",
  });
  assert.deepEqual(auction.corpus_control.fee_prefixes, [
    { charged_cents: "500", nominal_cents: "500" },
    { charged_cents: "500", nominal_cents: "500" },
  ]);
  assert.equal(auction.corpus_control.old_sell_reservation_cents, "499");
  assert.equal(auction.corpus_control.restore_order_sha256, "41d88079d98435969dde72a7800c612cb934e5b211d062997a58a76c4f3409dd");
  assert.deepEqual(auction.corpus_control.comparison_points, ["post-auction-rollover", "post-continuation-tick"]);
  assert.deepEqual(crossTick.corpus_control.comparison_points, ["post-partial-fill", "post-continuation-tick"]);
  assert.equal(crossTick.case_id, "representation-1-cross-tick-partial-fill");

  const saveRestore = controlledSurfaceRun();
  const state = { save_representation: { schema: "v1" }, snapshot: { accounts: { 0: {
    cash: 9_998_500, positions: { 600001: { qty: 2_200, invested_cents: 2_600, recovered_cents: 200 } },
  } } }, orders: { resting: { 600001: [{ owner: 0, id: 1, side: "Sell", qty: 1_000 }] } },
  envelope_receipts: [{ envelope: { order_id: 1 }, kind: "Fill" }] };
  saveRestore.records.push(
    { kind: "before_save", tick: 6, state: structuredClone(state) },
    { kind: "after_restore", tick: 6, state: structuredClone(state) },
    { kind: "TickFrame", tick: 7, events: [{ event: { OrderCanceled: {
      account: 0, code: "600001", id: 1, seq: 5,
    } } }], orders: { auction: {}, resting: { 600001: [] } } },
  );
  const save = extractLegacyControlledSellSurface(saveRestore, "save-restore-live-order");
  assert.equal(save.case_id, "representation-1-save-restore-live-order");
  assert.deepEqual(save.state.save_representation, { schema: "v1" });
  assert.deepEqual(save.corpus_control.comparison_points,
    ["pre-save", "post-restore", "post-continuation-tick"]);
});

test("controlled legacy extractor rejects a detached fee projection or missing rollover", () => {
  const badCash = controlledSurfaceRun();
  badCash.records.at(-1).seller_fee_projection.observed_combined_account_cash_delta = -499;
  assert.throws(() => extractLegacyControlledSellSurface(badCash, "cross-tick-partial-fill"), /cash (?:delta|equation)/);
  const noRollover = controlledSurfaceRun();
  noRollover.records[2].events = [];
  assert.throws(() => extractLegacyControlledSellSurface(noRollover, "auction-rollover"), /rollover/);
  const noRestore = controlledSurfaceRun();
  assert.throws(() => extractLegacyControlledSellSurface(noRestore, "save-restore-live-order"), /before_save/);
  const noBoundOrder = controlledSurfaceRun();
  const state = { save_representation: { schema: "v1" }, snapshot: { accounts: { 0: { positions: { 600001: { qty: 2_200 } } } } },
    envelope_receipts: [{ envelope: { order_id: 1 } }] };
  noBoundOrder.records.push({ kind: "before_save", tick: 6, state }, { kind: "after_restore", tick: 6, state: structuredClone(state) },
    { kind: "TickFrame", tick: 7, events: [{ event: { OrderCanceled: { id: 1 } } }], orders: { auction: {}, resting: { 600001: [] } } });
  assert.throws(() => extractLegacyControlledSellSurface(noBoundOrder, "save-restore-live-order"), /live Sell order identity/);
});

test("controlled projection assembly rejects stale or hand-edited extracted evidence", () => {
  const run = controlledSurfaceRun();
  const evidence = extractLegacyControlledSellSurface(run, "auction-rollover");
  const stale = structuredClone(evidence);
  stale.seller_fee_control.terminal_cash_cents = "9998501";
  assert.throws(() => assembleLegacyProjection(run, stale), /reviewed extraction/);
});

function buyerFeeRun() {
  const run = legacyRun();
  const account = (cash) => ({ cash, reserved_cash: 0, reserved_sell_qty: {}, positions: {
    600001: { invested_cents: 2_700_000, qty: 2400, recovered_cents: 300_000, t1_locked: 300 },
  } });
  run.records[0].class = "i-equivalence";
  run.records[0].setup = { simulation_policy_id: "a-share-simulation-v1", config: {
    commission_min: 500, commission_rate: 0.00025, stamp_tax_rate: 0.0005,
  }, npcs: { retail_count: 0, inst_count: 0, hot_count: 0 } };
  run.records[0].initial_state.snapshot.accounts = { 0: account(10_000_000) };
  run.records[0].sealed_exogenous_script = [[0, [
    { PlaceLimit: { code: "600001", price: 1000, qty: 300, side: "Sell" } },
    { PlaceLimit: { code: "600001", price: 1000, qty: 100, side: "Buy" } },
    { PlaceLimit: { code: "600001", price: 1000, qty: 200, side: "Buy" } },
  ]]];
  const frame = run.records[1];
  frame.seq_from = 1; frame.seq_to = 4; frame.snapshot.accounts = { 0: account(9_998_344) };
  frame.events = [
    { identity: [1, "OrderAccepted", "Account:0", "1", 0], canonical_session_ordinal: null,
      event: { OrderAccepted: { account: 0, code: "600001", id: 1, price: 1000,
        remaining_qty: 300, seq: 1, side: "Sell" } } },
    { identity: [1, "Trade", "Stock:600001", "0:0", 0], canonical_session_ordinal: null,
      event: { Trade: { code: "600001", maker: 0, price: 1000, qty: 100, seq: 2, taker: 0 } } },
    { identity: [1, "Trade", "Stock:600001", "0:0", 1], canonical_session_ordinal: null,
      event: { Trade: { code: "600001", maker: 0, price: 1000, qty: 200, seq: 3, taker: 0 } } },
    { identity: [1, "PriceTick", "Stock:600001", "600001", 0], canonical_session_ordinal: null,
      event: { PriceTick: { code: "600001", last_price: 1000, tick: 1, seq: 4,
        daily_candle: { volume: 300 }, bids: [], asks: [] } } },
  ];
  frame.timeseries_payload = { markets: {}, active_daily_candles: {}, daily_candles: {}, points: {} };
  run.records[2].state.snapshot.accounts = { 0: account(9_998_344) };
  return run;
}

test("buyer-fees extractor binds aggregate Buy fees to sealed intents, trades and total cash", () => {
  const run = buyerFeeRun();
  const surface = extractLegacyBuyerFeeSurface(run);
  assert.deepEqual(surface.state.buyer_fee_control, {
    account_id: "0", stock_code: "600001", side: "Buy", trade_role: "taker-buy",
    gross_cents: "300000", commission_cents: "1000", transfer_fee_cents: "3",
    spent_cash_cents: "301003",
  });
  assert.deepEqual(surface.corpus_control.surface_evidence, surface.state.buyer_fee_control);
  const assembled = assembleLegacyProjection(run, surface).projection;
  assert.equal(assembled.state.legacy_checkpoints.checkpoints.length, 1);
});

test("buyer-fees extractor rejects an unbound Trade leg, cash drift and stale evidence", () => {
  const detached = buyerFeeRun();
  detached.records[1].events[1].event.Trade.qty = 99;
  assert.throws(() => extractLegacyBuyerFeeSurface(detached), /quantity differs/);

  const cashDrift = buyerFeeRun();
  cashDrift.records[1].snapshot.accounts[0].cash += 1;
  assert.throws(() => extractLegacyBuyerFeeSurface(cashDrift), /cash equation/);

  const run = buyerFeeRun();
  const stale = extractLegacyBuyerFeeSurface(run);
  stale.state.buyer_fee_control.transfer_fee_cents = "2";
  assert.throws(() => assembleLegacyProjection(run, stale), /sealed fee\/cash extraction/);
});

test("t1 extractor accounts for both sides of a same-account Trade without weakening the lock equation", () => {
  const run = buyerFeeRun();
  run.records[0].initial_state.snapshot.accounts[0].positions[600001].t1_locked = 0;
  const surface = extractLegacyT1Surface(run);
  assert.deepEqual(surface.state.t1_control, {
    account_id: "0", stock_code: "600001", side: "Buy", trade_role: "taker-buy",
    qty_before: "2400", bought_qty: "300", sold_qty: "300", qty_after: "2400",
    t1_locked_before: "0", t1_locked_after: "300",
  });
  assert.deepEqual(assembleLegacyProjection(run, surface).projection.state.t1_control,
    surface.state.t1_control);
});

test("t1 extractor rejects position, Buy-leg and Sell-leg drift independently", () => {
  const positionDrift = buyerFeeRun();
  positionDrift.records[0].initial_state.snapshot.accounts[0].positions[600001].t1_locked = 0;
  positionDrift.records[1].snapshot.accounts[0].positions[600001].qty += 1;
  assert.throws(() => extractLegacyT1Surface(positionDrift), /before \+ bought - sold/);

  const buyDrift = buyerFeeRun();
  buyDrift.records[0].initial_state.snapshot.accounts[0].positions[600001].t1_locked = 0;
  buyDrift.records[1].events[1].event.Trade.taker = 9;
  assert.throws(() => extractLegacyT1Surface(buyDrift), /Buy Trade quantity/);

  const sellDrift = buyerFeeRun();
  sellDrift.records[0].initial_state.snapshot.accounts[0].positions[600001].t1_locked = 0;
  sellDrift.records[1].events[1].event.Trade.maker = 9;
  assert.throws(() => extractLegacyT1Surface(sellDrift), /Sell Trade quantity/);
});

test("continuous Buy extractor proves terminal identity even when immediate full fill omits OrderAccepted", () => {
  const run = buyerFeeRun();
  run.records[1].next_order_id = 4;
  const surface = extractLegacyContinuousBuyLegSurface(run);
  assert.deepEqual(surface.state.continuous_buy_leg_control, {
    account_id: "0", stock_code: "600001", side: "Buy", trade_role: "taker-buy",
    order_id: "2", trade_leg_count: "1", stable_order_identity_count: "1",
    order_accepted_event_count: "0", terminal_filled_qty: "100",
    terminal_filled_value_cents: "100000", terminal_live_qty: "0",
    trade_legs: [{ price_cents: "1000", qty: "100" }],
  });
  assert.deepEqual(assembleLegacyProjection(run, surface).projection.state.continuous_buy_leg_control,
    surface.state.continuous_buy_leg_control);
});

test("continuous Buy extractor rejects a detached ID interval, ambiguous Trade and live remainder", () => {
  const idDrift = buyerFeeRun();
  idDrift.records[1].next_order_id = 5;
  assert.throws(() => extractLegacyContinuousBuyLegSurface(idDrift), /order-ID interval/);

  const ambiguous = buyerFeeRun();
  ambiguous.records[1].next_order_id = 4;
  ambiguous.records[0].sealed_exogenous_script[0][1][2].PlaceLimit.qty = 100;
  ambiguous.records[1].events[2].event.Trade.qty = 100;
  assert.throws(() => extractLegacyContinuousBuyLegSurface(ambiguous), /uniquely Trade-bound/);

  const live = buyerFeeRun();
  live.records[1].next_order_id = 4;
  live.records[1].orders = { auction: {}, resting: { 600001: [{
    id: 2, owner: 0, side: "Buy", price: 1000, qty: 1, original_qty: 100,
    filled_qty: 99, filled_value: 99000, seq: 1,
  }] } };
  assert.throws(() => extractLegacyContinuousBuyLegSurface(live), /not terminal/);
});

function priceCageRun() {
  const run = legacyRun();
  const priceTick = { canonical_session_ordinal: null,
    identity: [5, "PriceTick", "Stock:600001", "600001", 0], event: { PriceTick: {
      asks: [], bids: [[1000, 100]], code: "600001", daily_candle: { volume: 0 },
      last_price: 1000, seq: 7, tick: 5,
    } } };
  run.records.push({ kind: "divergence_witness", name: "cage-reject-then-valid-control",
    ids: [3, 8], before_next_order_id: 1, new_expected_next_order_id: 3,
    frame: { kind: "TickFrame", tick: 5, seq_from: 5, seq_to: 7, next_order_id: 2,
      events: [
        { canonical_session_ordinal: null,
          identity: [5, "IntentRejected", "Account:0", "600001", 0], event: { IntentRejected: {
            account: 0, code: "600001", reason: "PriceCageExceeded", seq: 5,
          } } },
        { canonical_session_ordinal: null,
          identity: [5, "OrderAccepted", "Account:0", "1", 0], event: { OrderAccepted: {
            account: 0, code: "600001", id: 1, price: 1000, remaining_qty: 100,
            seq: 6, side: "Buy",
          } } },
        priceTick,
      ], snapshot: { accounts: { 0: { cash: 10_000_000 } } },
      orders: { auction: {}, resting: { 600001: [] } },
      timeseries_payload: { markets: {}, active_daily_candles: {}, daily_candles: {}, points: {} },
    } });
  return run;
}

test("price-cage extractor retains the old accepted ID and cursor for exact #3 mapping", () => {
  const run = priceCageRun();
  const extracted = extractLegacyPriceCageSurface(run);
  assert.deepEqual(extracted.surface.state.price_cage_control, {
    account_id: "0", stock_code: "600001", side: "Buy",
    outside_rejection: "PriceCageExceeded", inside_acceptance: "accepted",
    inside_order_id: "1", next_order_id_before: "1", next_order_id_after: "2",
  });
  assert.equal(extracted.updates.length, 1);
  assert.equal(assembleLegacyProjection(run, extracted.surface).projection.updates.length, 1);
});

test("price-cage extractor rejects a stale cursor expectation or detached acceptance", () => {
  const cursor = priceCageRun();
  cursor.records.at(-1).new_expected_next_order_id = 4;
  assert.throws(() => extractLegacyPriceCageSurface(cursor), /approved #3/);
  const detached = priceCageRun();
  detached.records.at(-1).frame.events[1].event.OrderAccepted.code = "000001";
  assert.throws(() => extractLegacyPriceCageSurface(detached), /stocks differ/);
});

function acceptanceFlipRun() {
  const run = legacyRun();
  run.scenario = "divergence-9";
  run.records[0].class = "ii-isolated-9";
  const position = { invested_cents: 240_000, qty: 2400, recovered_cents: 0, t1_locked: 0 };
  const priceTick = { canonical_session_ordinal: null,
    identity: [1, "AuctionTick", "Stock:600001", "600001", 0], event: { AuctionTick: {
      code: "600001", imbalance: 0, indicative_price: null, matched_volume: 0,
      phase: "CallAuction", seq: 2, tick: 1,
    } } };
  const frame = (cash, events, reservedCash, reservedShares, auction) => ({
    kind: "TickFrame", tick: 1, seq_from: 1, seq_to: 2, events: [...events, priceTick],
    snapshot: { accounts: { 0: { cash, positions: { 600001: position }, reserved_cash: reservedCash,
      reserved_sell_qty: reservedShares ? { 600001: reservedShares } : {} } } },
    orders: { auction: auction ? { 600001: [{ arrival_seq: 1, limit: 100, owner: 0,
      qty: 100, side: "Sell" }] } : {}, resting: { 600001: [] } },
    timeseries_payload: { markets: {}, active_daily_candles: {}, daily_candles: {}, points: {} },
  });
  const funded = frame(10_000, [{ canonical_session_ordinal: null,
    identity: [1, "OrderAccepted", "Account:0", "1", 0], event: { OrderAccepted: {
      account: 0, code: "600001", id: 1, price: 100, remaining_qty: 100, seq: 1, side: "Sell",
    } } }], 400, 100, true);
  const zero = frame(0, [{ canonical_session_ordinal: null,
    identity: [1, "IntentRejected", "Account:0", "600001", 0], event: { IntentRejected: {
      account: 0, code: "600001", reason: "InsufficientCash", seq: 1,
    } } }], 0, 0, false);
  run.records.splice(-1, 0, { kind: "acceptance_boundary",
    allowed_paths: ["events.IntentRejected->OrderAccepted", "snapshot.accounts.0.reserved_cash", "orders"],
    new_expected: { accepted: true, reserved_cash: 0 }, owned_sellable_shares: 2400,
    old_funded: funded, old_zero_cash: zero });
  return run;
}

test("acceptance-flip extractor keeps funded reservation separate from the zero-cash rejection frame", () => {
  const run = acceptanceFlipRun();
  const extracted = extractLegacyAcceptanceFlipSurface(run);
  assert.deepEqual(extracted.surface.state, { reserved_cash: "400", acceptance: "rejected" });
  assert.equal(extracted.surface.corpus_control.zero_cash_acceptance, true);
  assert.deepEqual(extracted.surface.corpus_control.surface_evidence, {
    account_id: "0", stock_code: "600001", side: "Sell", trade_role: "maker-sell",
  });
  assert.equal(extracted.updates[0].events[0].event.IntentRejected.reason, "InsufficientCash");
  assert.equal(assembleLegacyProjection(run, extracted.surface).projection.updates.length, 1);
});

test("acceptance-flip extractor rejects missing zero cash, reservation or explicit rejection", () => {
  const funded = acceptanceFlipRun();
  funded.records.find((row) => row.kind === "acceptance_boundary").old_funded.snapshot.accounts[0].reserved_cash = 0;
  assert.throws(() => extractLegacyAcceptanceFlipSurface(funded), /reservation/);
  const nonzero = acceptanceFlipRun();
  nonzero.records.find((row) => row.kind === "acceptance_boundary").old_zero_cash.snapshot.accounts[0].cash = 1;
  assert.throws(() => extractLegacyAcceptanceFlipSurface(nonzero), /zero-cash/);
  const accepted = acceptanceFlipRun();
  accepted.records.find((row) => row.kind === "acceptance_boundary").old_zero_cash.events[0]
    .event.IntentRejected.reason = "InvalidOrder";
  assert.throws(() => extractLegacyAcceptanceFlipSurface(accepted), /rejection/);
});

function threeLegRun() {
  const run = legacyRun();
  run.scenario = "divergence-9";
  run.records[0].class = "ii-isolated-9";
  run.records[0].setup = { simulation_policy_id: "a-share-simulation-v1",
    config: { commission_min: 500, commission_rate: 0.00025, stamp_tax_rate: 0.0005 },
    npcs: { retail_count: 0, inst_count: 0, hot_count: 0 } };
  const sellerPosition = (qty, recovered) => ({ invested_cents: 2400, qty,
    recovered_cents: recovered, t1_locked: 0 });
  const sellerAccount = (cash, qty, recovered, reservedCash, reservedShares) => ({
    cash, positions: { 600001: sellerPosition(qty, recovered) }, reserved_cash: reservedCash,
    reserved_sell_qty: reservedShares ? { 600001: reservedShares } : {},
  });
  const legs = [
    { tick: 5, seq: 5, qty: 100, gross: 100, fee: 500, net: -400,
      before: sellerAccount(1_481_061, 2400, 0, 499, 1200),
      after: sellerAccount(1_480_661, 2300, 100, 0, 1100) },
    { tick: 6, seq: 7, qty: 100, gross: 100, fee: 0, net: 100,
      before: sellerAccount(1_480_661, 2300, 100, 0, 1100),
      after: sellerAccount(1_480_761, 2200, 200, 0, 1000) },
    { tick: 7, seq: 9, qty: 1000, gross: 1000, fee: 1, net: 999,
      before: sellerAccount(1_480_761, 2200, 200, 0, 1000),
      after: sellerAccount(1_481_760, 1200, 1200, 0, 0) },
  ].map((leg, index) => ({ gross_cents: leg.gross, observed_fee_cents: leg.fee,
    observed_net_cents: leg.net, seller_before: leg.before, seller_after: leg.after, frame: {
      kind: "TickFrame", tick: leg.tick, seq_from: leg.seq, seq_to: leg.seq + 1,
      events: [
        { canonical_session_ordinal: null, identity: [leg.tick, "Trade", "Stock:600001", "1:0", 0],
          event: { Trade: { code: "600001", maker: 1, price: 1, qty: leg.qty, seq: leg.seq, taker: 0 } } },
        { canonical_session_ordinal: null, identity: [leg.tick, "PriceTick", "Stock:600001", "600001", 0],
          event: { PriceTick: { asks: [], bids: [], code: "600001",
            daily_candle: { volume: [100, 200, 1200][index] }, last_price: 1,
            seq: leg.seq + 1, tick: leg.tick } } },
      ], snapshot: { accounts: { 0: { cash: 10_000_000 }, 1: leg.after } },
      orders: { auction: {}, resting: { 600001: index === 2 ? [] : [{ filled_qty: [100, 200][index],
        filled_value: [100, 200][index], id: 1, original_qty: 1200, owner: 1, price: 1,
        qty: [1100, 1000][index], seq: 0, side: "Sell" }] } },
      timeseries_payload: { markets: {}, active_daily_candles: {}, daily_candles: {}, points: {} },
    } }));
  run.records.splice(-1, 0, { kind: "independent_seller_fee_control", seller: 1, buyer: 0,
    npc_attention_disabled_until_tick: 100, initial_state: { snapshot: { accounts: {
      0: { cash: 10_000_000, positions: {}, reserved_cash: 0, reserved_sell_qty: {} },
      1: sellerAccount(1_481_061, 2400, 0, 0, 0),
    } }, resting_orders: { 600001: [{ filled_qty: 0, filled_value: 0, id: 1,
      original_qty: 1200, owner: 1, price: 1, qty: 1200, seq: 1, side: "Sell" }] } }, legs },
  { kind: "fee_expectation", gross_legs_cents: [100, 100, 1000],
    nominal_cumulative_cents: [500, 500, 501], old_charged_legs_cents: [500, 0, 1],
    old_net_legs_cents: [-400, 100, 999] });
  return run;
}

test("three-leg extractor binds the independent seller fee chain to trades and account deltas", () => {
  const run = threeLegRun();
  const extracted = extractLegacyThreeLegFeeCatchupSurface(run);
  assert.deepEqual(extracted.surface.state, {
    reserved_cash_cents: "0", charged_total_cents: "501", net_delivery_cents: "699",
  });
  assert.deepEqual(extracted.surface.corpus_control.fee_prefixes, [
    { nominal_cents: "500", charged_cents: "500" },
    { nominal_cents: "500", charged_cents: "500" },
    { nominal_cents: "501", charged_cents: "501" },
  ]);
  assert.equal(extracted.updates.length, 3);
  assert.equal(assembleLegacyProjection(run, extracted.surface).projection.updates.length, 3);
});

test("three-leg extractor rejects Trade, cash, position and fee-expectation drift", () => {
  const trade = threeLegRun();
  trade.records.find((row) => row.kind === "independent_seller_fee_control").legs[1]
    .frame.events[0].event.Trade.qty = 200;
  assert.throws(() => extractLegacyThreeLegFeeCatchupSurface(trade), /gross differs/);
  const cash = threeLegRun();
  cash.records.find((row) => row.kind === "independent_seller_fee_control").legs[0].seller_after.cash += 1;
  assert.throws(() => extractLegacyThreeLegFeeCatchupSurface(cash), /cash delta/);
  const position = threeLegRun();
  position.records.find((row) => row.kind === "independent_seller_fee_control").legs[2]
    .seller_after.positions[600001].recovered_cents -= 1;
  assert.throws(() => extractLegacyThreeLegFeeCatchupSurface(position), /recovered-cost/);
  const expectation = threeLegRun();
  expectation.records.find((row) => row.kind === "fee_expectation").nominal_cumulative_cents[2] = 500;
  assert.throws(() => extractLegacyThreeLegFeeCatchupSurface(expectation), /nominal cumulative/);
});

test("sealed corpus loader rejects modified manifest/run hashes and traversal", async (context) => {
  const tempRoot = process.env.TMPDIR ?? path.resolve(".tmp/process-tmp");
  await mkdir(tempRoot, { recursive: true });
  const root = await mkdtemp(path.join(tempRoot, "corpus-loader-"));
  context.after(() => rm(root, { recursive: true, force: true }));
  await mkdir(path.join(root, "attempt-12"));
  const run = legacyRun();
  const bytes = run.records.map((record) => JSON.stringify(record)).join("\n") + "\n";
  const hash = (value) => createHash("sha256").update(value).digest("hex");
  const manifest = { status: "sealed", corpus_digest: "sealed-test", runs: [{ scenario: "equivalence", seed: 1, file: "equivalence-1.jsonl", sha256: hash(bytes), repeat_sha256: hash(bytes) }] };
  const manifestBytes = JSON.stringify(manifest);
  const index = { schema: 2, kind: "sealed-corpus-index", manifest: "attempt-12/manifest.json", manifest_sha256: hash(manifestBytes), corpus_digest: "sealed-test" };
  await writeFile(path.join(root, "manifest.json"), JSON.stringify(index));
  await writeFile(path.join(root, "attempt-12/manifest.json"), manifestBytes);
  await writeFile(path.join(root, "attempt-12/equivalence-1.jsonl"), bytes);
  assert.equal((await loadSealedCorpusRun(root, "equivalence", 1)).records.length, 3);
  await writeFile(path.join(root, "attempt-12/equivalence-1.jsonl"), bytes + "\n");
  await assert.rejects(loadSealedCorpusRun(root, "equivalence", 1), /run SHA-256/);
  await writeFile(path.join(root, "attempt-12/manifest.json"), "{}");
  await assert.rejects(loadSealedCorpusRun(root, "equivalence", 1), /manifest SHA-256/);
  index.manifest = "../outside";
  await writeFile(path.join(root, "manifest.json"), JSON.stringify(index));
  await assert.rejects(loadSealedCorpusRun(root, "equivalence", 1), /unsafe/);
});
