import assert from "node:assert/strict";
import { describe, it } from "node:test";

import {
  artifactReceipt,
  compareCorpusCase,
  verifyConservationSnapshot,
  verifyDeterminismMatrix,
  verifyEvidenceBundle,
  verifyPerturbationGate,
  verifyStressCorpus,
} from "./escrow-verification-contracts.mjs";

const ARTIFACTS = {
  authoritative_state: artifactReceipt("authoritative-state"),
  event_stream: artifactReceipt("event-stream"),
  receipts: artifactReceipt("receipt-stream"),
  save_slot: artifactReceipt("save-slot"),
};
const CONTROL_HASH = "c".repeat(64);

function executionCoverage() {
  const restoreSlot = (slot) => ({
    slot,
    saved: artifactReceipt(`${slot}-save`),
    restored: artifactReceipt(`${slot}-save`),
    uninterrupted_continuation: artifactReceipt(`${slot}-continuation`),
    restored_continuation: artifactReceipt(`${slot}-continuation`),
  });
  return {
    tick_from: 0,
    tick_to: 20,
    auction_finalizations: 1,
    day_end_finalizations: 1,
    stock_codes: ["000001", "600001"],
    multi_leg_order_ids: ["42"],
    restore_slots: [restoreSlot("opening-auction"), restoreSlot("partial-fill")],
  };
}

function observation({ budget = "1", repeat = 0, mode = "canonical", disabled = null, artifacts = ARTIFACTS, order } = {}) {
  return {
    schema: "escrow-determinism-observation-v1",
    scenario: "multi-stock-auction-day-end",
    seed: "7",
    budget,
    repeat,
    mode,
    canonical_merge_disabled: disabled,
    artifacts,
    execution_coverage: executionCoverage(),
    precanonical_order: order ?? {
      account_shards: ["account:1", "account:2"],
      stock_shards: ["stock:000001", "stock:600001"],
      completion_order: ["worker:0", "worker:1"],
    },
  };
}

function determinismMatrix() {
  return ["1", "2", "4", "auto"].flatMap((budget) => [0, 1].map((repeat) => observation({ budget, repeat })));
}

function changedArtifacts(name) {
  return { ...ARTIFACTS, [name]: artifactReceipt(`changed-${name}`) };
}

function perturbationOrder(prefix) {
  return {
    account_shards: [`account:${prefix}:2`, `account:${prefix}:1`],
    stock_shards: [`stock:${prefix}:600001`, `stock:${prefix}:000001`],
    completion_order: [`worker:${prefix}:1`, `worker:${prefix}:0`],
  };
}

function res(cash, shares) {
  return { cash_cents: String(cash), shares: String(shares) };
}

function conservationSnapshot(tick = 12) {
  return {
    schema: "escrow-conservation-snapshot-v1",
    scenario: "multi-stock-auction-day-end",
    seed: "7",
    tick,
    envelopes: [
      {
        key: { account_id: "1", stock_code: "600001", order_id: "41", side: "Buy" },
        origin: "existing",
        basis: { tick_start_live: res(100, 0), p1_live: res(80, 0) },
        receipts: [
          { receipt_index: "10", journal: "PreSeal", kind: "Release", live_before: res(100, 0), spent: res(0, 0), released: res(20, 0), live_after: res(80, 0) },
          { receipt_index: "11", journal: "SealedBatch", kind: "Fill", live_before: res(80, 0), spent: res(50, 0), released: res(10, 0), live_after: res(20, 0) },
        ],
        commit_live: res(20, 0),
      },
      {
        key: { account_id: "1", stock_code: "000001", order_id: "42", side: "Sell" },
        origin: "created",
        basis: { created: res(0, 100) },
        receipts: [
          { receipt_index: "12", journal: "SealedBatch", kind: "Fill", live_before: res(0, 100), spent: res(0, 30), released: res(0, 0), live_after: res(0, 70) },
          { receipt_index: "13", journal: "SealedBatch", kind: "Fill", live_before: res(0, 70), spent: res(0, 30), released: res(0, 0), live_after: res(0, 40) },
        ],
        commit_live: res(0, 40),
      },
    ],
    accounts: [
      {
        account_id: "1",
        cash_cents: "1000",
        positions: [{ stock_code: "000001", qty: "100", t1_locked: "20" }],
        aggregate: { left: res(100, 100), right: res(100, 100) },
      },
    ],
  };
}

function updates() {
  return [{
    kind: "TickFrame",
    tick: 1,
    seq_from: 1,
    seq_to: 2,
    timeseries_payload: { code: "600001", close_cents: "1000" },
    events: [
      {
        comparison_event_key: [1, "OrderAccepted", "Account:1", 0],
        event: { OrderAccepted: { seq: 1, account: "1", code: "600001", id: 9, side: "Buy", price: 1000, remaining_qty: 100 } },
      },
      {
        comparison_event_key: [1, "AuctionTick", "Stock:600001", 0],
        event: { AuctionTick: { seq: 2, tick: 1, phase: "CallAuction", code: "600001", indicative_price: 1000, matched_volume: 0, imbalance: 0 } },
      },
    ],
  }];
}

function corpusControl(corpusClass, surface) {
  const feedback = { strategy_generated_intents: 0, plan_generated_intents: 0, state_dependent_intents: 0 };
  const base = {
    surface,
    seller_order_count: corpusClass === "stress" ? 0 : 1,
    old_sell_reservation_cents: "0",
    fee_prefixes: [],
    feedback,
    acceptance: corpusClass === "stress" ? "not-applicable" : "accepted",
    zero_cash_acceptance: false,
    sealed_exogenous_script_sha256: null,
    rng_cursor: null,
    strategy_state_sha256: null,
    plan_state_sha256: null,
    pending_intents_sha256: null,
    restore_order_sha256: null,
    comparison_points: ["post-commit"],
    surface_evidence: null,
  };
  if (corpusClass === "equivalence") {
    const equivalenceSurface = surface ?? "normal-multi-leg-terminal";
    if (equivalenceSurface !== "normal-multi-leg-terminal") {
      const evidence = {
        "buyer-fees": {
          account_id: "1", stock_code: "600001", side: "Buy", trade_role: "taker-buy",
          gross_cents: "100000", commission_cents: "500", transfer_fee_cents: "1", spent_cash_cents: "100501",
        },
        t1: {
          account_id: "1", stock_code: "600001", side: "Buy", trade_role: "taker-buy",
          qty_before: "100", bought_qty: "100", qty_after: "200", t1_locked_before: "0", t1_locked_after: "100",
        },
        "price-cage": {
          account_id: "1", stock_code: "600001", side: "Buy",
          outside_rejection: "PriceCageExceeded", inside_acceptance: "accepted", inside_order_id: "9",
        },
        "continuous-buy-leg": {
          account_id: "1", stock_code: "600001", side: "Buy", trade_role: "taker-buy",
          order_id: "9", trade_leg_count: 2, stable_order_identity_count: 1,
        },
      }[equivalenceSurface];
      return { ...base, surface: equivalenceSurface, seller_order_count: 0, surface_evidence: evidence };
    }
    return {
      ...base,
      surface: "normal-multi-leg-terminal",
      fee_prefixes: [
        { nominal_cents: "2", charged_cents: "2" },
        { nominal_cents: "5", charged_cents: "5" },
      ],
      zero_cash_acceptance: true,
    };
  }
  if (corpusClass === "divergence-9") {
    const divergenceSurface = surface ?? "acceptance-flip";
    return {
      ...base,
      surface: divergenceSurface,
      old_sell_reservation_cents: "400",
      acceptance: "rejected",
      zero_cash_acceptance: true,
      surface_evidence: divergenceSurface === "acceptance-flip"
        ? { account_id: "1", stock_code: "600001", side: "Sell", trade_role: "taker-sell" }
        : null,
    };
  }
  if (corpusClass === "controlled-live-sell") {
    const controlledSurface = surface ?? "auction-rollover";
    const comparisonPoints = {
      "auction-rollover": ["post-auction-rollover", "post-continuation-tick"],
      "cross-tick-partial-fill": ["post-partial-fill", "post-continuation-tick"],
      "save-restore-live-order": ["pre-save", "post-restore", "post-continuation-tick"],
    }[controlledSurface];
    return {
      ...base,
      surface: controlledSurface,
      fee_prefixes: [{ nominal_cents: "501", charged_cents: "501" }],
      sealed_exogenous_script_sha256: CONTROL_HASH,
      rng_cursor: "17",
      strategy_state_sha256: CONTROL_HASH,
      plan_state_sha256: CONTROL_HASH,
      pending_intents_sha256: CONTROL_HASH,
      restore_order_sha256: CONTROL_HASH,
      comparison_points: comparisonPoints,
    };
  }
  return { ...base, surface: "long-run-multi-stock" };
}

function projection(corpusClass = "equivalence", surface) {
  const result = {
    schema: "escrow-corpus-projection-v1",
    case_id: `${corpusClass}-case`,
    scenario: "multi-stock-auction-day-end",
    seed: "7",
    class: corpusClass,
    updates: updates(),
    state: { reserved_cash: "0", position_qty: "100" },
    seller_fee_control: null,
    corpus_control: corpusControl(corpusClass, surface),
  };
  if (corpusClass === "equivalence" && surface && surface !== "normal-multi-leg-terminal") {
    const evidence = structuredClone(result.corpus_control.surface_evidence);
    const stateField = {
      "buyer-fees": "buyer_fee_control",
      t1: "t1_control",
      "price-cage": "price_cage_control",
      "continuous-buy-leg": "continuous_buy_leg_control",
    }[surface];
    result.state[stateField] = evidence;
    if (surface === "buyer-fees" || surface === "t1") {
      result.updates[0].events.push({
        comparison_event_key: [1, "Trade", "Stock:600001", 0],
        event: { Trade: { seq: 3, code: "600001", price: 1000, qty: 100, maker: "2", taker: "1" } },
      });
      result.updates[0].seq_to = 3;
    } else if (surface === "price-cage") {
      result.updates[0].events[1] = {
        comparison_event_key: [1, "IntentRejected", "Account:1", 0],
        event: { IntentRejected: { seq: 2, account: "1", code: "600001", reason: "PriceCageExceeded" } },
      };
    } else if (surface === "continuous-buy-leg") {
      result.updates[0].events.push(
        {
          comparison_event_key: [1, "Trade", "Stock:600001", 0],
          event: { Trade: { seq: 3, code: "600001", price: 1000, qty: 40, maker: "2", taker: "1" } },
        },
        {
          comparison_event_key: [1, "Trade", "Stock:600001", 1],
          event: { Trade: { seq: 4, code: "600001", price: 1000, qty: 60, maker: "3", taker: "1" } },
        },
      );
      result.updates[0].seq_to = 4;
    }
  }
  return result;
}

function controlledCorpusEntry(surface) {
  const legacy = projection("controlled-live-sell", surface);
  const current = structuredClone(legacy);
  legacy.case_id = current.case_id = `controlled-${surface}`;
  const common = {
    gross_cents: "1200",
    nominal_final_cents: "501",
    charged_final_cents: "501",
    terminal_cash_cents: "699",
    invested_cents: "900",
    recovered_cents: "1200",
    fills: [{ order_id: "9", qty: "1200", price_cents: "1" }],
  };
  legacy.seller_fee_control = structuredClone(common);
  current.seller_fee_control = { ...structuredClone(common), charged_final_cents: "300", terminal_cash_cents: "900" };
  current.corpus_control.fee_prefixes[0].charged_cents = "300";
  const transformations = [
    { divergence: 9, effect: "fee_charged", path: "/seller_fee_control/charged_final_cents" },
    { divergence: 9, effect: "terminal_cash_equation", path: "/seller_fee_control/terminal_cash_cents" },
  ];
  if (surface === "save-restore-live-order") {
    legacy.state.save_representation = { schema: "v1" };
    current.state.save_representation = { schema: "v2" };
    transformations.push({ divergence: 7, effect: "save_representation", path: "/state/save_representation/schema" });
  }
  return { legacy, current, transformations };
}

function completeCorpusEntries() {
  const equivalent = projection("equivalence");
  const buyerSurfaces = ["buyer-fees", "t1", "price-cage", "continuous-buy-leg"].map((surface) => {
    const legacy = projection("equivalence", surface);
    legacy.case_id = `equivalence-${surface}`;
    return { legacy, current: structuredClone(legacy), transformations: [] };
  });
  const acceptanceLegacy = projection("divergence-9", "acceptance-flip");
  const acceptanceCurrent = structuredClone(acceptanceLegacy);
  acceptanceLegacy.case_id = acceptanceCurrent.case_id = "acceptance-flip";
  acceptanceCurrent.corpus_control.acceptance = "accepted";
  acceptanceLegacy.updates[0].events[0] = {
    comparison_event_key: [1, "IntentRejected", "Account:1", 0],
    event: { IntentRejected: { seq: 1, account: "1", code: "600001", reason: "InsufficientCash" } },
  };
  acceptanceCurrent.updates[0].events[0].event.OrderAccepted.side = "Sell";
  acceptanceLegacy.state = { ...acceptanceLegacy.state, reserved_cash: "400", acceptance: "rejected" };
  acceptanceCurrent.state = { ...acceptanceCurrent.state, reserved_cash: "0", acceptance: "accepted" };

  const catchupLegacy = projection("divergence-9", "three-leg-fee-catchup");
  const catchupCurrent = structuredClone(catchupLegacy);
  catchupLegacy.case_id = catchupCurrent.case_id = "three-leg-fee-catchup";
  catchupLegacy.corpus_control.acceptance = catchupCurrent.corpus_control.acceptance = "accepted";
  catchupLegacy.corpus_control.fee_prefixes = [
    { nominal_cents: "5", charged_cents: "5" },
    { nominal_cents: "10", charged_cents: "10" },
    { nominal_cents: "16", charged_cents: "16" },
  ];
  catchupCurrent.corpus_control.fee_prefixes = [
    { nominal_cents: "5", charged_cents: "1" },
    { nominal_cents: "10", charged_cents: "2" },
    { nominal_cents: "16", charged_cents: "9" },
  ];
  catchupLegacy.state = { ...catchupLegacy.state, charged_total_cents: "16", net_delivery_cents: "1184" };
  catchupCurrent.state = { ...catchupCurrent.state, charged_total_cents: "9", net_delivery_cents: "1191" };

  return [
    { legacy: equivalent, current: structuredClone(equivalent), transformations: [] },
    ...buyerSurfaces,
    {
      legacy: acceptanceLegacy,
      current: acceptanceCurrent,
      transformations: [
        { divergence: 9, effect: "sell_reservation", path: "/state/reserved_cash" },
        { divergence: 9, effect: "acceptance_flip", path: "/state/acceptance" },
        { divergence: 9, effect: "acceptance_flip", path: "/updates/*/facts/**" },
      ],
    },
    {
      legacy: catchupLegacy,
      current: catchupCurrent,
      transformations: [
        { divergence: 9, effect: "fee_charged", path: "/state/charged_total_cents" },
        { divergence: 9, effect: "net_delivery", path: "/state/net_delivery_cents" },
      ],
    },
    controlledCorpusEntry("auction-rollover"),
    controlledCorpusEntry("cross-tick-partial-fill"),
    controlledCorpusEntry("save-restore-live-order"),
  ];
}

describe("determinism and perturbation contracts", () => {
  it("requires byte-identical state/events/receipts/save bytes across 1/2/4/auto and repeats", () => {
    assert.deepEqual(verifyDeterminismMatrix(determinismMatrix()), {
      scenarios: 1,
      budgets: ["1", "2", "4", "auto"],
      repeats_per_budget: 2,
      compared_observations: 8,
    });
    const changed = determinismMatrix();
    changed.find((entry) => entry.budget === "4" && entry.repeat === 1).artifacts = changedArtifacts("receipts");
    assert.throws(() => verifyDeterminismMatrix(changed), /byte artifacts mismatch/);
  });

  it("proves all three pre-canonical orders changed and all three disabled-merge controls fail", () => {
    const reference = observation();
    const perturbed = observation({ mode: "perturbed", order: perturbationOrder("p") });
    const controls = [
      observation({ mode: "negative-control", disabled: "account", order: perturbationOrder("a"), artifacts: changedArtifacts("authoritative_state") }),
      observation({ mode: "negative-control", disabled: "stock", order: perturbationOrder("s"), artifacts: changedArtifacts("event_stream") }),
      observation({ mode: "negative-control", disabled: "completion", order: perturbationOrder("c"), artifacts: changedArtifacts("receipts") }),
    ];
    assert.equal(verifyPerturbationGate(reference, [perturbed], controls).negative_controls, 3);

    const emptyControl = structuredClone(controls);
    emptyControl[2].artifacts = structuredClone(ARTIFACTS);
    assert.throws(() => verifyPerturbationGate(reference, [perturbed], emptyControl), /did not expose/);

    const noAccountPerturbation = structuredClone(perturbed);
    noAccountPerturbation.precanonical_order.account_shards = [...reference.precanonical_order.account_shards];
    assert.throws(() => verifyPerturbationGate(reference, [noAccountPerturbation], controls), /did not change account_shards/);
  });
});

describe("per-envelope and per-account conservation", () => {
  it("checks PreSeal and SealedBatch equations separately for cash and shares", () => {
    assert.deepEqual(verifyConservationSnapshot(conservationSnapshot()), {
      scenario: "multi-stock-auction-day-end",
      seed: "7",
      tick: 12,
      envelope_rows: 2,
      account_rows: 1,
      receipt_rows: 4,
    });
  });

  it("rejects cross-resource leakage, duplicate receipts, aggregate drift, negative cash and T+1 overflow", () => {
    const crossLeak = conservationSnapshot();
    crossLeak.envelopes[1].receipts[0].spent = res(1, 59);
    assert.throws(() => verifyConservationSnapshot(crossLeak), /receipt equation|Sell.*cash/);

    const duplicate = conservationSnapshot();
    duplicate.envelopes[1].receipts[0].receipt_index = "11";
    assert.throws(() => verifyConservationSnapshot(duplicate), /duplicate global receipt_index/);

    const aggregate = conservationSnapshot();
    aggregate.accounts[0].aggregate.right.cash_cents = "101";
    assert.throws(() => verifyConservationSnapshot(aggregate), /claimed right aggregate/);

    const negativeCash = conservationSnapshot();
    negativeCash.accounts[0].cash_cents = "-1";
    assert.throws(() => verifyConservationSnapshot(negativeCash), /non-negative decimal string/);

    const t1 = conservationSnapshot();
    t1.accounts[0].positions[0].t1_locked = "101";
    assert.throws(() => verifyConservationSnapshot(t1), /t1_locked exceeds qty/);
  });

  it("rejects a balanced but non-zero Sell cash escrow", () => {
    const sellCash = conservationSnapshot();
    sellCash.envelopes[1].basis.created = res(100, 100);
    sellCash.envelopes[1].receipts[0].live_before = res(100, 100);
    sellCash.envelopes[1].receipts[0].live_after = res(100, 40);
    sellCash.envelopes[1].commit_live = res(100, 40);
    sellCash.accounts[0].aggregate.left.cash_cents = "200";
    sellCash.accounts[0].aggregate.right.cash_cents = "200";
    assert.throws(() => verifyConservationSnapshot(sellCash), /Sell.*cash.*zero/);
  });

  it("rejects a balanced but non-zero Buy shares escrow", () => {
    const buyShares = conservationSnapshot();
    buyShares.envelopes[0].basis.tick_start_live = res(100, 100);
    buyShares.envelopes[0].basis.p1_live = res(80, 100);
    buyShares.envelopes[0].receipts[0].live_before = res(100, 100);
    buyShares.envelopes[0].receipts[0].live_after = res(80, 100);
    buyShares.envelopes[0].receipts[1].live_before = res(80, 100);
    buyShares.envelopes[0].receipts[1].spent = res(50, 50);
    buyShares.envelopes[0].receipts[1].live_after = res(20, 50);
    buyShares.envelopes[0].commit_live = res(20, 50);
    buyShares.accounts[0].aggregate.left.shares = "200";
    buyShares.accounts[0].aggregate.right.shares = "200";
    assert.throws(() => verifyConservationSnapshot(buyShares), /Buy.*shares.*zero/);
  });

  it("rejects Fill labels that do not spend a positive side resource", () => {
    const noOpBuyFill = conservationSnapshot();
    noOpBuyFill.envelopes[0].receipts[1].spent = res(0, 0);
    noOpBuyFill.envelopes[0].receipts[1].released = res(60, 0);
    assert.throws(() => verifyConservationSnapshot(noOpBuyFill), /Buy Fill.*positive.*cash/);

    const noOpSellFill = conservationSnapshot();
    noOpSellFill.envelopes[1].receipts[0].spent = res(0, 0);
    noOpSellFill.envelopes[1].receipts[0].live_after = res(0, 100);
    noOpSellFill.envelopes[1].receipts[1].live_before = res(0, 100);
    noOpSellFill.envelopes[1].receipts[1].live_after = res(0, 70);
    noOpSellFill.envelopes[1].commit_live = res(0, 70);
    assert.throws(() => verifyConservationSnapshot(noOpSellFill), /Sell Fill.*positive.*shares/);
  });
});

describe("structured corpus comparator", () => {
  it("accepts frame-local fact reordering and independent seq renumbering", () => {
    const legacy = projection();
    const current = structuredClone(legacy);
    current.updates[0].events.reverse();
    current.updates[0].seq_from = 101;
    current.updates[0].seq_to = 102;
    current.updates[0].events[0].event.AuctionTick.seq = 101;
    current.updates[0].events[1].event.OrderAccepted.seq = 102;
    assert.deepEqual(compareCorpusCase(legacy, current), {
      case_id: "equivalence-case",
      scenario: "multi-stock-auction-day-end",
      seed: "7",
      ticks: [1],
      class: "equivalence",
      surface: "normal-multi-leg-terminal",
      differences: [],
    });
  });

  it("rejects event deletion, fixed-identity payload swap and unmapped payload mutation", () => {
    const legacy = projection();

    const deletion = structuredClone(legacy);
    deletion.updates[0].events.pop();
    deletion.updates[0].seq_to = deletion.updates[0].seq_from;
    assert.throws(() => compareCorpusCase(legacy, deletion), /unmapped/);

    const swap = structuredClone(legacy);
    [swap.updates[0].events[0].event, swap.updates[0].events[1].event] = [swap.updates[0].events[1].event, swap.updates[0].events[0].event];
    assert.throws(() => compareCorpusCase(legacy, swap), /variant and comparison key disagree/);

    const mutation = structuredClone(legacy);
    mutation.updates[0].events[0].event.OrderAccepted.price = 1001;
    assert.throws(() => compareCorpusCase(legacy, mutation), /unmapped/);
  });

  it("accepts only an explicitly field-mapped #9 difference and rejects cascades in equivalence corpus", () => {
    const legacy = projection("divergence-9");
    const current = structuredClone(legacy);
    current.corpus_control.acceptance = "accepted";
    legacy.updates[0].events[0] = {
      comparison_event_key: [1, "IntentRejected", "Account:1", 0],
      event: { IntentRejected: { seq: 1, account: "1", code: "600001", reason: "InsufficientCash" } },
    };
    current.updates[0].events[0].event.OrderAccepted.side = "Sell";
    current.state.reserved_cash = "0";
    current.state.acceptance = "accepted";
    legacy.state.reserved_cash = "400";
    legacy.state.acceptance = "rejected";
    const result = compareCorpusCase(legacy, current, [
      { divergence: 9, effect: "sell_reservation", path: "/state/reserved_cash" },
      { divergence: 9, effect: "acceptance_flip", path: "/state/acceptance" },
      { divergence: 9, effect: "acceptance_flip", path: "/updates/*/facts/**" },
    ]);
    assert.equal(result.differences[0].divergence, 9);

    const equalLegacy = projection("equivalence");
    const cascaded = structuredClone(equalLegacy);
    cascaded.state.reserved_cash = "400";
    assert.throws(() => compareCorpusCase(equalLegacy, cascaded), /unmapped/);
  });

  it("does not let a #9 fee label conceal a Trade price change", () => {
    const { legacy, current, transformations } = completeCorpusEntries()[6];
    current.updates[0].events[1].event.AuctionTick.indicative_price = 999;
    assert.throws(
      () => compareCorpusCase(legacy, current, [...transformations, { divergence: 9, effect: "fee_charged", path: "/updates/0/facts/*/payload/indicative_price" }]),
      /effect.*path|indicative_price/,
    );
  });

  it("requires acceptance-flip controls to agree with real rejection/acceptance facts", () => {
    const valid = completeCorpusEntries()[5];
    assert.equal(compareCorpusCase(valid.legacy, valid.current, valid.transformations).surface, "acceptance-flip");

    const buyAcceptance = structuredClone(valid);
    buyAcceptance.current.updates[0].events[0].event.OrderAccepted.side = "Buy";
    assert.throws(() => compareCorpusCase(buyAcceptance.legacy, buyAcceptance.current, buyAcceptance.transformations), /Sell|side/);

    const contradictory = structuredClone(valid);
    contradictory.legacy.updates = structuredClone(contradictory.current.updates);
    assert.throws(() => compareCorpusCase(contradictory.legacy, contradictory.current, contradictory.transformations), /legacy evidence.*InsufficientCash/);

    const unrelatedMutation = structuredClone(valid);
    unrelatedMutation.current.updates[0].events[1].event.AuctionTick.indicative_price = 999;
    assert.throws(() => compareCorpusCase(unrelatedMutation.legacy, unrelatedMutation.current, unrelatedMutation.transformations), /unrelated event facts/);
  });

  it("rejects buyer-surface labels that are detached from state and event evidence", () => {
    for (const surface of ["buyer-fees", "t1", "price-cage", "continuous-buy-leg"]) {
      const stateDetached = projection("equivalence", surface);
      const stateField = {
        "buyer-fees": "buyer_fee_control",
        t1: "t1_control",
        "price-cage": "price_cage_control",
        "continuous-buy-leg": "continuous_buy_leg_control",
      }[surface];
      delete stateDetached.state[stateField];
      assert.throws(() => compareCorpusCase(stateDetached, structuredClone(stateDetached)), /state/);

      const eventDetached = projection("equivalence", surface);
      if (surface === "price-cage") eventDetached.updates[0].events[1].event.IntentRejected.reason = "InsufficientCash";
      else {
        for (const fact of eventDetached.updates[0].events) {
          if (fact.event.Trade) fact.event.Trade.taker = "99";
        }
      }
      assert.throws(() => compareCorpusCase(eventDetached, structuredClone(eventDetached)), /Trade|PriceCageExceeded/);
    }
  });

  it("mechanically rejects an inadmissible equivalence corpus and an incomplete legacy fee basis", () => {
    const legacy = projection("equivalence");
    const current = structuredClone(legacy);
    legacy.corpus_control.old_sell_reservation_cents = "400";
    current.corpus_control.old_sell_reservation_cents = "400";
    legacy.corpus_control.feedback.strategy_generated_intents = 1;
    current.corpus_control.feedback.strategy_generated_intents = 1;
    assert.throws(() => compareCorpusCase(legacy, current), /equivalence.*reservation|fee prefix|feedback/);

    const controlledLegacy = projection("controlled-live-sell");
    const controlledCurrent = structuredClone(controlledLegacy);
    controlledLegacy.seller_fee_control = {
      gross_cents: "1200", nominal_final_cents: "501", charged_final_cents: "300",
      terminal_cash_cents: "699", invested_cents: "900", recovered_cents: "1200", fills: [],
    };
    controlledCurrent.seller_fee_control = {
      ...structuredClone(controlledLegacy.seller_fee_control), terminal_cash_cents: "900",
    };
    controlledLegacy.corpus_control.fee_prefixes[0].charged_cents = "300";
    controlledCurrent.corpus_control.fee_prefixes[0].charged_cents = "300";
    assert.throws(
      () => compareCorpusCase(controlledLegacy, controlledCurrent, [{ divergence: 9, effect: "terminal_cash_equation", path: "/seller_fee_control/terminal_cash_cents" }]),
      /legacy.*charged_final.*nominal_final/,
    );
  });

  it("validates event entities and zero-based contiguous comparison ordinals", () => {
    const wrongEntity = projection();
    wrongEntity.updates[0].events[0].comparison_event_key[2] = "Stock:600001";
    assert.throws(() => compareCorpusCase(wrongEntity, structuredClone(wrongEntity)), /entity/);

    const ordinalGap = projection();
    ordinalGap.updates[0].events.push({
      comparison_event_key: [1, "OrderAccepted", "Account:1", 2],
      event: { OrderAccepted: { seq: 3, account: "1", code: "600001", id: 10, side: "Buy", price: 1000, remaining_qty: 100 } },
    });
    ordinalGap.updates[0].seq_to = 3;
    assert.throws(() => compareCorpusCase(ordinalGap, structuredClone(ordinalGap)), /ordinal.*contiguous|ordinal.*zero/);
  });

  it("rejects a payload swap between two facts of the same variant and entity", () => {
    const legacy = projection();
    legacy.updates[0].events.push({
      comparison_event_key: [1, "OrderAccepted", "Account:1", 1],
      event: { OrderAccepted: { seq: 3, account: "1", code: "600001", id: 10, side: "Buy", price: 1001, remaining_qty: 100 } },
    });
    legacy.updates[0].seq_to = 3;
    const current = structuredClone(legacy);
    [current.updates[0].events[0].event, current.updates[0].events[2].event] = [current.updates[0].events[2].event, current.updates[0].events[0].event];
    assert.throws(() => compareCorpusCase(legacy, current), /unmapped/);
  });

  it("enforces the controlled-live-sell terminal cash equation and gross cost-chain equality", () => {
    const legacy = projection("controlled-live-sell");
    const current = structuredClone(legacy);
    const common = {
      gross_cents: "1200",
      nominal_final_cents: "501",
      charged_final_cents: "501",
      terminal_cash_cents: "699",
      invested_cents: "900",
      recovered_cents: "1200",
      fills: [{ order_id: "9", qty: "1200", price_cents: "1" }],
    };
    legacy.seller_fee_control = structuredClone(common);
    current.seller_fee_control = { ...structuredClone(common), charged_final_cents: "300", terminal_cash_cents: "900" };
    current.corpus_control.fee_prefixes[0].charged_cents = "300";
    const transformations = [
      { divergence: 9, effect: "fee_charged", path: "/seller_fee_control/charged_final_cents" },
      { divergence: 9, effect: "terminal_cash_equation", path: "/seller_fee_control/terminal_cash_cents" },
    ];
    assert.equal(compareCorpusCase(legacy, current, transformations).differences.length, 2);
    current.seller_fee_control.recovered_cents = "1199";
    assert.throws(() => compareCorpusCase(legacy, current, transformations), /recovered_cents/);
  });

  it("refuses old/new equivalence language for stress corpus and verifies only new determinism/conservation", () => {
    const stress = projection("stress");
    assert.throws(() => compareCorpusCase(stress, structuredClone(stress)), /new-engine-only/);
    const stressConservation = Array.from({ length: 21 }, (_, tick) => conservationSnapshot(tick));
    const result = verifyStressCorpus(stress, determinismMatrix(), stressConservation);
    assert.equal(result.historical_equivalence_claimed, false);
    assert.equal(result.determinism.compared_observations, 8);
    assert.throws(() => verifyStressCorpus(stress, determinismMatrix(), stressConservation.slice(1)), /every tick|missing.*tick/);
  });
});

describe("complete evidence bundle gate", () => {
  it("accepts only a linked bundle with all corpus surfaces and stress evidence", () => {
    const reference = observation();
    const stress = projection("stress");
    const bundle = {
      schema: "escrow-verification-bundle-v1",
      determinism: determinismMatrix(),
      perturbation: {
        reference,
        observations: [observation({ mode: "perturbed", order: perturbationOrder("p") })],
        negative_controls: [
          observation({ mode: "negative-control", disabled: "account", order: perturbationOrder("a"), artifacts: changedArtifacts("authoritative_state") }),
          observation({ mode: "negative-control", disabled: "stock", order: perturbationOrder("s"), artifacts: changedArtifacts("event_stream") }),
          observation({ mode: "negative-control", disabled: "completion", order: perturbationOrder("c"), artifacts: changedArtifacts("receipts") }),
        ],
      },
      conservation: Array.from({ length: 21 }, (_, tick) => conservationSnapshot(tick)),
      corpus: completeCorpusEntries(),
      stress: [{ current: stress, determinism: determinismMatrix(), conservation: Array.from({ length: 21 }, (_, tick) => conservationSnapshot(tick)) }],
    };
    const result = verifyEvidenceBundle(bundle);
    assert.equal(result.status, "PASS");
    assert.equal(result.corpus.length, 10);
    assert.equal(result.stress[0].historical_equivalence_claimed, false);

    const incompleteRestore = structuredClone(bundle);
    incompleteRestore.determinism[0].execution_coverage.restore_slots.pop();
    assert.throws(() => verifyEvidenceBundle(incompleteRestore), /restore_slots/);

    const fillThenRelease = structuredClone(bundle);
    for (const snapshot of fillThenRelease.conservation) {
      snapshot.envelopes[1].receipts[1].kind = "Release";
      snapshot.envelopes[1].receipts[1].spent = res(0, 0);
      snapshot.envelopes[1].receipts[1].released = res(0, 30);
    }
    assert.throws(() => verifyEvidenceBundle(fillThenRelease), /multi-leg/);

    const noOpMultiLeg = structuredClone(bundle);
    for (const snapshot of noOpMultiLeg.conservation) {
      snapshot.envelopes[1].receipts[0].spent = res(0, 0);
      snapshot.envelopes[1].receipts[0].live_after = res(0, 100);
      snapshot.envelopes[1].receipts[1].live_before = res(0, 100);
      snapshot.envelopes[1].receipts[1].spent = res(0, 0);
      snapshot.envelopes[1].receipts[1].live_after = res(0, 100);
      snapshot.envelopes[1].commit_live = res(0, 100);
    }
    assert.throws(() => verifyEvidenceBundle(noOpMultiLeg), /Sell Fill.*positive.*shares/);

    const missingBuyerFees = structuredClone(bundle);
    missingBuyerFees.corpus = missingBuyerFees.corpus.filter((entry) => entry.legacy.corpus_control.surface !== "buyer-fees");
    assert.throws(() => verifyEvidenceBundle(missingBuyerFees), /equivalence:buyer-fees/);

    const missingTick = structuredClone(bundle);
    missingTick.conservation = missingTick.conservation.filter((snapshot) => snapshot.tick !== 7);
    assert.throws(() => verifyEvidenceBundle(missingTick), /every tick|missing.*tick/);

    const unrelatedMultiLegId = structuredClone(bundle);
    for (const observation of [
      ...unrelatedMultiLegId.determinism,
      unrelatedMultiLegId.perturbation.reference,
      ...unrelatedMultiLegId.perturbation.observations,
      ...unrelatedMultiLegId.perturbation.negative_controls,
    ]) observation.execution_coverage.multi_leg_order_ids = ["999"];
    assert.throws(() => verifyEvidenceBundle(unrelatedMultiLegId), /multi-leg.*order|order.*multi-leg/);
  });

  it("rejects a bundle that omits corpus and stress coverage", () => {
    const reference = observation();
    const bundle = {
      schema: "escrow-verification-bundle-v1",
      determinism: determinismMatrix(),
      perturbation: {
        reference,
        observations: [observation({ mode: "perturbed", order: perturbationOrder("p") })],
        negative_controls: [
          observation({ mode: "negative-control", disabled: "account", order: perturbationOrder("a"), artifacts: changedArtifacts("authoritative_state") }),
          observation({ mode: "negative-control", disabled: "stock", order: perturbationOrder("s"), artifacts: changedArtifacts("event_stream") }),
          observation({ mode: "negative-control", disabled: "completion", order: perturbationOrder("c"), artifacts: changedArtifacts("receipts") }),
        ],
      },
      conservation: Array.from({ length: 21 }, (_, tick) => conservationSnapshot(tick)),
      corpus: [],
      stress: [],
    };
    assert.throws(() => verifyEvidenceBundle(bundle), /corpus.*coverage|corpus.*empty/);
  });
});
