import assert from "node:assert/strict";
import { describe, it } from "node:test";

import {
  artifactReceipt,
  verifyConservationSnapshot,
  verifyDeterminismMatrix,
  verifyPerturbationGate,
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
    tick_from: "0",
    tick_to: "20",
    auction_finalizations: "1",
    day_end_finalizations: "1",
    stock_codes: ["000001", "600001"],
    multi_leg_order_ids: ["42"],
    restore_slots: [restoreSlot("opening-auction"), restoreSlot("partial-fill")],
  };
}

function observation({ budget = "1", repeat = "0", mode = "canonical", disabled = null, artifacts = ARTIFACTS, order } = {}) {
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
  return ["1", "2", "4", "auto"].flatMap((budget) => ["0", "1"].map((repeat) => observation({ budget, repeat })));
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
    tick: String(tick),
    envelopes: [
      {
        key: { account_id: "1", stock_code: "600001", order_id: "41", side: "Buy" },
        origin: "existing",
        basis: { tick_start_live: res(100, 0), p1_live: res(80, 0) },
        receipts: [
          { receipt_index: "10", journal: "PreSeal", source: { kind: "P0Expiry", index: "0" }, transition_ordinal_within_source: "0", kind: "Release", live_before: res(100, 0), spent: res(0, 0), released: res(20, 0), live_after: res(80, 0) },
          { receipt_index: "11", journal: "SealedBatch", source: { kind: "SealedIntent", index: "0" }, transition_ordinal_within_source: "0", kind: "Fill", live_before: res(80, 0), spent: res(50, 0), released: res(10, 0), live_after: res(20, 0) },
        ],
        commit_live: res(20, 0),
      },
      {
        key: { account_id: "1", stock_code: "000001", order_id: "42", side: "Sell" },
        origin: "created",
        basis: { created: res(0, 100) },
        receipts: [
          { receipt_index: "12", journal: "SealedBatch", source: { kind: "SealedIntent", index: "1" }, transition_ordinal_within_source: "0", kind: "Fill", live_before: res(0, 100), spent: res(0, 30), released: res(0, 0), live_after: res(0, 70) },
          { receipt_index: "13", journal: "SealedBatch", source: { kind: "SealedIntent", index: "1" }, transition_ordinal_within_source: "1", kind: "Fill", live_before: res(0, 70), spent: res(0, 30), released: res(0, 0), live_after: res(0, 40) },
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

describe("determinism and perturbation contracts", () => {
  it("requires byte-identical state/events/receipts/save bytes across 1/2/4/auto and repeats", () => {
    assert.deepEqual(verifyDeterminismMatrix(determinismMatrix()), {
      scenarios: 1,
      budgets: ["1", "2", "4", "auto"],
      repeats_per_budget: 2,
      compared_observations: 8,
    });
    const changed = determinismMatrix();
    changed.find((entry) => entry.budget === "4" && entry.repeat === "1").artifacts = changedArtifacts("receipts");
    assert.throws(() => verifyDeterminismMatrix(changed), /byte artifacts mismatch/);

    const numericRepeat = determinismMatrix();
    numericRepeat[0].repeat = 0;
    assert.throws(() => verifyDeterminismMatrix(numericRepeat), /decimal string/);

    const numericByteLength = determinismMatrix();
    numericByteLength[0].artifacts = structuredClone(numericByteLength[0].artifacts);
    numericByteLength[0].artifacts.receipts.byte_length = 10;
    assert.throws(() => verifyDeterminismMatrix(numericByteLength), /decimal string/);
  });

  it("proves all three delivery orders changed and the remaining merge control fails", () => {
    const reference = observation();
    const perturbed = observation({ mode: "perturbed", order: perturbationOrder("p") });
    const controls = [
      observation({ mode: "negative-control", disabled: "completion", order: perturbationOrder("c"), artifacts: changedArtifacts("receipts") }),
    ];
    assert.equal(verifyPerturbationGate(reference, [perturbed], controls).negative_controls, 1);

    const emptyControl = structuredClone(controls);
    emptyControl[0].artifacts = structuredClone(ARTIFACTS);
    assert.throws(() => verifyPerturbationGate(reference, [perturbed], emptyControl), /did not expose/);

    const noAccountPerturbation = structuredClone(perturbed);
    noAccountPerturbation.precanonical_order.account_shards = [...reference.precanonical_order.account_shards];
    assert.throws(() => verifyPerturbationGate(reference, [noAccountPerturbation], controls), /did not change account_shards/);
  });
});

describe("per-envelope and per-account conservation", () => {
  it("accepts actual P3-created Buy rows without inventing an existing P1 basis, and checks quiet ticks", () => {
    const created = conservationSnapshot();
    const buy = created.envelopes[0];
    buy.origin = "created";
    buy.basis = { created: res(80, 0) };
    buy.receipts.shift();
    created.accounts[0].aggregate.left.cash_cents = "80";
    created.accounts[0].aggregate.right.cash_cents = "80";
    assert.equal(verifyConservationSnapshot(created).envelope_rows, 2);
    const quiet = conservationSnapshot();
    quiet.envelopes = [];
    quiet.accounts[0].aggregate = { left: res(0, 0), right: res(0, 0) };
    assert.equal(verifyConservationSnapshot(quiet).receipt_rows, 0);
    quiet.accounts[0].cash_cents = "-1";
    assert.throws(() => verifyConservationSnapshot(quiet), /non-negative decimal/);
  });
  it("checks PreSeal and SealedBatch equations separately for cash and shares", () => {
    assert.deepEqual(verifyConservationSnapshot(conservationSnapshot()), {
      scenario: "multi-stock-auction-day-end",
      seed: "7",
      tick: "12",
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

    const numericTick = conservationSnapshot();
    numericTick.tick = 12;
    assert.throws(() => verifyConservationSnapshot(numericTick), /decimal string/);
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

  it("accepts independent receipt source order and rejects broken local identities", () => {
    const badJournal = conservationSnapshot();
    badJournal.envelopes[0].receipts[0].source.kind = "Auction";
    assert.throws(() => verifyConservationSnapshot(badJournal), /journal\/source pairing/);

    const ordinalGap = conservationSnapshot();
    ordinalGap.envelopes[1].receipts[1].transition_ordinal_within_source = "2";
    assert.throws(() => verifyConservationSnapshot(ordinalGap), /source-local transition ordinal/);

    const reorderedLocalKeys = conservationSnapshot();
    reorderedLocalKeys.envelopes[0].receipts[1].source.index = "2";
    assert.equal(verifyConservationSnapshot(reorderedLocalKeys).receipt_rows, 4);

    const duplicateIdentity = conservationSnapshot();
    duplicateIdentity.envelopes[1].receipts[1].transition_ordinal_within_source = "0";
    assert.throws(() => verifyConservationSnapshot(duplicateIdentity), /duplicate receipt identity/);

    const duplicateIndex = conservationSnapshot();
    duplicateIndex.envelopes[1].receipts[1].receipt_index = "12";
    assert.throws(() => verifyConservationSnapshot(duplicateIndex), /duplicate global receipt_index/);

    const indexGap = conservationSnapshot();
    indexGap.envelopes[1].receipts[1].receipt_index = "14";
    assert.throws(() => verifyConservationSnapshot(indexGap), /global receipt_index has a gap/);
  });
});

describe("EnvelopeConservation transition 边界", () => {
  it("保持超过 i64/u64 的 Resource 精度及 canonical decimal 接受集", () => {
    const huge = 2n ** 80n;
    const snapshot = conservationSnapshot();
    const buy = snapshot.envelopes[0];
    buy.basis.tick_start_live.cash_cents = String(huge);
    buy.basis.p1_live.cash_cents = String(huge - 20n);
    buy.receipts[0].live_before.cash_cents = String(huge);
    buy.receipts[0].live_after.cash_cents = String(huge - 20n);
    buy.receipts[1].live_before.cash_cents = String(huge - 20n);
    buy.receipts[1].live_after.cash_cents = String(huge - 80n);
    buy.commit_live.cash_cents = String(huge - 80n);
    snapshot.accounts[0].aggregate.left.cash_cents = String(huge);
    snapshot.accounts[0].aggregate.right.cash_cents = String(huge);
    assert.equal(verifyConservationSnapshot(snapshot).receipt_rows, 4);
    for (const value of ["-1", "01", "1.0"]) {
      const invalid = structuredClone(snapshot);
      invalid.envelopes[0].basis.tick_start_live.cash_cents = value;
      assert.throws(() => verifyConservationSnapshot(invalid), /non-negative decimal/);
    }
  });

  it("无 receipt 与仅 P0 receipt 都检查 P1 截点", () => {
    for (const onlyP0 of [false, true]) {
      const snapshot = conservationSnapshot();
      snapshot.envelopes = [snapshot.envelopes[0]];
      const buy = snapshot.envelopes[0];
      buy.receipts = onlyP0 ? [buy.receipts[0]] : [];
      buy.basis.p1_live = res(onlyP0 ? 80 : 100, 0);
      buy.commit_live = res(onlyP0 ? 80 : 100, 0);
      snapshot.accounts[0].aggregate = { left: res(100, 0), right: res(100, 0) };
      assert.equal(verifyConservationSnapshot(snapshot).receipt_rows, onlyP0 ? 1 : 0);
      buy.basis.p1_live.cash_cents = "79";
      assert.throws(() => verifyConservationSnapshot(snapshot), /P1 boundary/);
    }
  });

  it("global identity 可跨 envelope 重排，envelope 内 live chain 仍须衔接", () => {
    const independent = conservationSnapshot();
    independent.envelopes.reverse();
    assert.equal(verifyConservationSnapshot(independent).receipt_rows, 4);
    const broken = conservationSnapshot();
    broken.envelopes[1].receipts.reverse();
    assert.throws(() => verifyConservationSnapshot(broken), /receipt chain/);
  });
});
