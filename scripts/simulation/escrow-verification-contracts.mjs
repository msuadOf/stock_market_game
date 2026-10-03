#!/usr/bin/env node
import { createHash } from "node:crypto";

const OBSERVATION_SCHEMA = "escrow-determinism-observation-v1";
const CONSERVATION_SCHEMA = "escrow-conservation-snapshot-v1";
const BUDGETS = ["1", "2", "4", "auto"];
const ARTIFACT_NAMES = ["authoritative_state", "event_stream", "receipts", "save_slot"];
const RECEIPT_SOURCE_KINDS = new Set(["P0Expiry", "SealedIntent", "Auction", "DayEnd"]);
const SIGNED_DECIMAL_FIELDS = new Set([
  "price", "indicative_price", "clearing_price", "last_price", "last_close", "best_bid", "best_ask",
  "time", "open", "high", "low", "close", "price_cents", "reserved_cash", "reserved_cash_cents",
  "old_sell_reservation_cents", "gross_cents", "nominal_final_cents", "charged_final_cents",
  "charged_total_cents", "net_delivery_cents", "terminal_cash_cents", "invested_cents",
  "recovered_cents", "commission_cents", "transfer_fee_cents", "spent_cash_cents",
  "nominal_cents", "charged_cents",
]);
const UNSIGNED_DECIMAL_FIELDS = new Set([
  "seq", "tick", "qty", "maker", "taker", "matched_volume", "volume", "trade_count",
  "cumulative_volume", "account", "order_id", "remaining_qty", "publication_id",
  "second_of_day", "limit", "ticks_per_day", "snapshot_tick", "snapshot_seq", "intraday_ticks",
  "phase_rank", "local_event_index", "imbalance", "turnover_cents",
]);

function fail(message) {
  throw new Error(message);
}

function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function requireRecord(value, label) {
  if (!isRecord(value)) fail(`${label} must be an object`);
  return value;
}

function exactKeys(value, expected, label) {
  const actual = Object.keys(requireRecord(value, label)).sort();
  const wanted = [...expected].sort();
  if (JSON.stringify(actual) !== JSON.stringify(wanted)) fail(`${label} keys mismatch`);
}

function jsonEqual(left, right) {
  return JSON.stringify(left) === JSON.stringify(right);
}

function requireJsonEqual(left, right, label) {
  if (!jsonEqual(left, right)) fail(`${label} mismatch`);
}

export function artifactReceipt(bytes) {
  if (!(typeof bytes === "string" || Buffer.isBuffer(bytes) || bytes instanceof Uint8Array)) fail("artifact bytes must be a string or byte array");
  const buffer = Buffer.from(bytes);
  return { byte_length: String(buffer.length), sha256: createHash("sha256").update(buffer).digest("hex") };
}

function validateArtifactReceipt(receipt, label) {
  exactKeys(receipt, ["byte_length", "sha256"], label);
  decimal(receipt.byte_length, `${label}.byte_length`);
  if (typeof receipt.sha256 !== "string" || !/^[a-f0-9]{64}$/.test(receipt.sha256)) fail(`${label}.sha256 is invalid`);
}

function validateOrder(order, label) {
  exactKeys(order, ["account_shards", "stock_shards", "completion_order"], label);
  for (const field of ["account_shards", "stock_shards", "completion_order"]) {
    const values = order[field];
    if (!Array.isArray(values) || values.length < 2 || !values.every((value) => typeof value === "string" && value.length > 0)) {
      fail(`${label}.${field} must contain at least two non-empty shard identities`);
    }
    if (new Set(values).size !== values.length) fail(`${label}.${field} contains duplicate identities`);
  }
}

function observationIdentity(observation) {
  return `${observation.scenario}\u0000${observation.seed}`;
}

function validateExecutionCoverage(coverage, label) {
  exactKeys(coverage, ["tick_from", "tick_to", "auction_finalizations", "day_end_finalizations", "stock_codes", "multi_leg_order_ids", "restore_slots"], label);
  const tickFrom = decimal(coverage.tick_from, `${label}.tick_from`);
  const tickTo = decimal(coverage.tick_to, `${label}.tick_to`);
  if (tickTo < tickFrom) {
    fail(`${label} tick range is invalid`);
  }
  for (const field of ["auction_finalizations", "day_end_finalizations"]) {
    if (decimal(coverage[field], `${label}.${field}`) <= 0n) fail(`${label}.${field} must be positive`);
  }
  if (!Array.isArray(coverage.stock_codes) || coverage.stock_codes.length < 2
    || !coverage.stock_codes.every((code) => typeof code === "string" && /^[0-9]{6}$/.test(code))
    || new Set(coverage.stock_codes).size !== coverage.stock_codes.length) {
    fail(`${label}.stock_codes must identify at least two distinct stocks`);
  }
  if (!Array.isArray(coverage.multi_leg_order_ids) || coverage.multi_leg_order_ids.length === 0
    || !coverage.multi_leg_order_ids.every((orderId) => typeof orderId === "string" && /^(0|[1-9][0-9]*)$/.test(orderId))) {
    fail(`${label}.multi_leg_order_ids must identify an observed multi-leg order`);
  }
  if (!Array.isArray(coverage.restore_slots) || coverage.restore_slots.length !== 2) fail(`${label}.restore_slots must contain exactly two restore-continuation checks`);
  const slots = new Set();
  coverage.restore_slots.forEach((slot, index) => {
    const slotLabel = `${label}.restore_slots[${index}]`;
    exactKeys(slot, ["slot", "saved", "restored", "uninterrupted_continuation", "restored_continuation"], slotLabel);
    if (typeof slot.slot !== "string" || slot.slot.length === 0 || slots.has(slot.slot)) fail(`${slotLabel}.slot is invalid or duplicate`);
    slots.add(slot.slot);
    for (const field of ["saved", "restored", "uninterrupted_continuation", "restored_continuation"]) validateArtifactReceipt(slot[field], `${slotLabel}.${field}`);
    requireJsonEqual(slot.saved, slot.restored, `${slotLabel} saved/restored bytes`);
    requireJsonEqual(slot.uninterrupted_continuation, slot.restored_continuation, `${slotLabel} continuation bytes`);
  });
}

export function validateObservation(observation, label = "runtime observation") {
  exactKeys(observation, ["schema", "scenario", "seed", "budget", "repeat", "mode", "canonical_merge_disabled", "artifacts", "precanonical_order", "execution_coverage"], label);
  if (observation.schema !== OBSERVATION_SCHEMA) fail(`${label}.schema is unsupported`);
  if (typeof observation.scenario !== "string" || observation.scenario.length === 0) fail(`${label}.scenario is missing`);
  decimal(observation.seed, `${label}.seed`);
  if (!BUDGETS.includes(observation.budget)) fail(`${label}.budget is invalid`);
  decimal(observation.repeat, `${label}.repeat`);
  if (!new Set(["canonical", "perturbed", "negative-control"]).has(observation.mode)) fail(`${label}.mode is invalid`);
  const disabled = observation.canonical_merge_disabled;
  if (observation.mode === "negative-control") {
    if (disabled !== "completion") fail(`${label}.canonical_merge_disabled is invalid`);
  } else if (disabled !== null) {
    fail(`${label}.canonical_merge_disabled must be null outside a negative control`);
  }
  exactKeys(observation.artifacts, ARTIFACT_NAMES, `${label}.artifacts`);
  for (const name of ARTIFACT_NAMES) validateArtifactReceipt(observation.artifacts[name], `${label}.artifacts.${name}`);
  validateOrder(observation.precanonical_order, `${label}.precanonical_order`);
  validateExecutionCoverage(observation.execution_coverage, `${label}.execution_coverage`);
}

function compareArtifacts(reference, candidate, label) {
  requireJsonEqual(candidate.artifacts, reference.artifacts, `${label} byte artifacts`);
  requireJsonEqual(candidate.execution_coverage, reference.execution_coverage, `${label} execution coverage`);
}

export function verifyDeterminismMatrix(observations) {
  if (!Array.isArray(observations) || observations.length === 0) fail("determinism observations must be non-empty");
  const groups = new Map();
  observations.forEach((observation, index) => {
    validateObservation(observation, `determinism observation ${index}`);
    if (observation.mode !== "canonical") fail("determinism matrix accepts canonical observations only");
    const key = observationIdentity(observation);
    const group = groups.get(key) ?? [];
    group.push(observation);
    groups.set(key, group);
  });
  for (const [key, group] of groups) {
    const slots = new Map();
    for (const observation of group) {
      const slot = `${observation.budget}/${observation.repeat}`;
      if (slots.has(slot)) fail(`duplicate determinism observation ${key}/${slot}`);
      slots.set(slot, observation);
    }
    for (const budget of BUDGETS) {
      for (const repeat of ["0", "1"]) {
        if (!slots.has(`${budget}/${repeat}`)) fail(`missing determinism observation ${key}/${budget}/${repeat}`);
      }
    }
    if (slots.size !== BUDGETS.length * 2) fail(`unexpected determinism observation outside the exact budget/repeat matrix for ${key}`);
    const reference = slots.get("1/0");
    for (const [slot, candidate] of slots) compareArtifacts(reference, candidate, `determinism ${key}/${slot}`);
  }
  return { scenarios: groups.size, budgets: [...BUDGETS], repeats_per_budget: 2, compared_observations: observations.length };
}

export function verifyPerturbationGate(reference, perturbations, negativeControls) {
  validateObservation(reference, "perturbation reference");
  if (reference.mode !== "canonical") fail("perturbation reference must be canonical");
  if (!Array.isArray(perturbations) || perturbations.length === 0) fail("perturbation gate requires at least one real perturbed observation");
  for (const [index, candidate] of perturbations.entries()) {
    validateObservation(candidate, `perturbation ${index}`);
    if (candidate.mode !== "perturbed") fail(`perturbation ${index} must be marked perturbed`);
    if (observationIdentity(candidate) !== observationIdentity(reference) || candidate.budget !== reference.budget) fail(`perturbation ${index} identity or budget mismatch`);
    for (const field of ["account_shards", "stock_shards", "completion_order"]) {
      if (jsonEqual(candidate.precanonical_order[field], reference.precanonical_order[field])) fail(`perturbation ${index} did not change ${field}`);
    }
    compareArtifacts(reference, candidate, `perturbation ${index}`);
  }
  if (!Array.isArray(negativeControls) || negativeControls.length !== 1) fail("perturbation gate requires exactly one negative control");
  const observedDisabled = new Set();
  for (const [index, candidate] of negativeControls.entries()) {
    validateObservation(candidate, `perturbation negative control ${index}`);
    if (candidate.mode !== "negative-control") fail(`negative control ${index} mode mismatch`);
    if (observationIdentity(candidate) !== observationIdentity(reference) || candidate.budget !== reference.budget) fail(`negative control ${index} identity or budget mismatch`);
    if (observedDisabled.has(candidate.canonical_merge_disabled)) fail(`duplicate negative control ${candidate.canonical_merge_disabled}`);
    observedDisabled.add(candidate.canonical_merge_disabled);
    if (jsonEqual(candidate.artifacts, reference.artifacts)) fail(`negative control ${candidate.canonical_merge_disabled} did not expose a canonicalization failure`);
  }
  requireJsonEqual([...observedDisabled], ["completion"], "perturbation disabled-merge coverage");
  return { perturbations: perturbations.length, negative_controls: negativeControls.length, dimensions: ["account", "stock", "completion"] };
}

function decimal(value, label) {
  if (typeof value !== "string" || !/^(0|[1-9][0-9]*)$/.test(value)) fail(`${label} must be a non-negative decimal string`);
  return BigInt(value);
}

function signedDecimal(value, label) {
  if (typeof value !== "string" || !/^(0|-?[1-9][0-9]*)$/.test(value)) fail(`${label} must be a canonical signed decimal string`);
  const parsed = BigInt(value);
  if (parsed < -(1n << 63n) || parsed > (1n << 63n) - 1n) fail(`${label} is outside the signed i64 range`);
  return parsed;
}

function unsignedDecimal(value, label) {
  const parsed = decimal(value, label);
  if (parsed > (1n << 64n) - 1n) fail(`${label} is outside the unsigned u64 range`);
  return parsed;
}

function validateProjectedIntegers(value, label, field = null) {
  if (value === null || typeof value === "boolean" || typeof value === "string") {
    if (value !== null && SIGNED_DECIMAL_FIELDS.has(field)) signedDecimal(value, label);
    if (value !== null && UNSIGNED_DECIMAL_FIELDS.has(field)) unsignedDecimal(value, label);
    return;
  }
  if (typeof value === "number") {
    if (Number.isFinite(value) && !Number.isInteger(value)) return;
    fail(`${label} contains an integral or non-finite JSON number; evidence integers must use decimal strings`);
  }
  if (Array.isArray(value)) {
    if (field === "bids" || field === "asks") {
      value.forEach((entry, index) => {
        if (!Array.isArray(entry) || entry.length !== 2) fail(`${label}[${index}] must be a price/quantity pair`);
        signedDecimal(entry[0], `${label}[${index}][0]`);
        decimal(entry[1], `${label}[${index}][1]`);
      });
      return;
    }
    value.forEach((entry, index) => validateProjectedIntegers(entry, `${label}[${index}]`, field));
    return;
  }
  for (const [key, entry] of Object.entries(requireRecord(value, label))) {
    validateProjectedIntegers(entry, `${label}.${key}`, key);
  }
}

function compareBigInt(left, right) {
  return left < right ? -1 : left > right ? 1 : 0;
}

class Resource {
  constructor(cash = 0n, shares = 0n) {
    this.cash = cash;
    this.shares = shares;
    Object.freeze(this);
  }

  static fromJson(value, label) {
    exactKeys(value, ["cash_cents", "shares"], label);
    return new Resource(decimal(value.cash_cents, `${label}.cash_cents`), decimal(value.shares, `${label}.shares`));
  }

  toJson() {
    return { cash_cents: String(this.cash), shares: String(this.shares) };
  }

  add(other) {
    return new Resource(this.cash + other.cash, this.shares + other.shares);
  }

  equals(other) {
    return this.cash === other.cash && this.shares === other.shares;
  }

  assertEquals(other, label) {
    if (!this.equals(other)) fail(`${label} mismatch: cash ${this.cash}/${other.cash}, shares ${this.shares}/${other.shares}`);
  }
}

function envelopeKey(key, label) {
  exactKeys(key, ["account_id", "stock_code", "order_id", "side"], label);
  decimal(key.account_id, `${label}.account_id`);
  if (typeof key.stock_code !== "string" || !/^[0-9]{6}$/.test(key.stock_code)) fail(`${label}.stock_code is invalid`);
  if (typeof key.order_id !== "string" || !/^(0|[1-9][0-9]*)$/.test(key.order_id)) fail(`${label}.order_id is invalid`);
  if (!new Set(["Buy", "Sell"]).has(key.side)) fail(`${label}.side is invalid`);
  return JSON.stringify(key);
}

function accumulateAccount(map, account, side, value) {
  const row = map.get(account) ?? { left: new Resource(), right: new Resource() };
  row[side] = row[side].add(value);
  map.set(account, row);
}

class EnvelopeConservation {
  constructor(row, rowIndex) {
    this.row = row;
    this.rowIndex = rowIndex;
    if (!new Set(["existing", "created"]).has(row.origin)) fail(`envelope row ${rowIndex}.origin is invalid`);
    this.commitLive = Resource.fromJson(row.commit_live, `envelope row ${rowIndex}.commit_live`);
    if (!Array.isArray(row.receipts)) fail(`envelope row ${rowIndex}.receipts must be an array`);
    if (row.origin === "existing") {
      exactKeys(row.basis, ["tick_start_live", "p1_live"], `envelope row ${rowIndex}.basis`);
      this.live = Resource.fromJson(row.basis.tick_start_live, `envelope row ${rowIndex}.basis.tick_start_live`);
      this.allocationLive = Resource.fromJson(row.basis.p1_live, `envelope row ${rowIndex}.basis.p1_live`);
      this.left = this.live;
    } else {
      exactKeys(row.basis, ["created"], `envelope row ${rowIndex}.basis`);
      this.live = Resource.fromJson(row.basis.created, `envelope row ${rowIndex}.basis.created`);
      this.left = this.live;
    }
    if (row.key.side === "Sell" && this.live.cash !== 0n) fail(`envelope row ${rowIndex} Sell basis cash escrow must be zero`);
    if (row.key.side === "Sell" && this.commitLive.cash !== 0n) fail(`envelope row ${rowIndex} Sell commit cash escrow must be zero`);
    if (row.key.side === "Buy" && this.live.shares !== 0n) fail(`envelope row ${rowIndex} Buy basis shares escrow must be zero`);
    if (row.key.side === "Buy" && row.origin === "existing" && this.allocationLive.shares !== 0n) fail(`envelope row ${rowIndex} Buy P1 basis shares escrow must be zero`);
    if (row.key.side === "Buy" && this.commitLive.shares !== 0n) fail(`envelope row ${rowIndex} Buy commit shares escrow must be zero`);
    this.expiryReleased = new Resource();
    this.sealedSpent = new Resource();
    this.sealedReleased = new Resource();
    this.reachedSealed = false;
  }

  assertP1Boundary() {
    if (this.row.origin === "existing") this.live.assertEquals(this.allocationLive, `envelope row ${this.rowIndex} P1 boundary`);
  }

  applyReceipt(receipt, receiptIndex) {
    const row = this.row;
    const rowIndex = this.rowIndex;
    if (row.origin === "created" && receipt.journal === "PreSeal") fail(`created envelope row ${rowIndex} has a P0 contribution`);
    if (this.reachedSealed && receipt.journal === "PreSeal") fail(`envelope row ${rowIndex} returns to PreSeal after SealedBatch`);
    if (receipt.journal === "SealedBatch" && !this.reachedSealed) {
      this.assertP1Boundary();
      this.reachedSealed = true;
    }
    const before = Resource.fromJson(receipt.live_before, `envelope row ${rowIndex} receipt ${receiptIndex}.live_before`);
    const spent = Resource.fromJson(receipt.spent, `envelope row ${rowIndex} receipt ${receiptIndex}.spent`);
    const released = Resource.fromJson(receipt.released, `envelope row ${rowIndex} receipt ${receiptIndex}.released`);
    const after = Resource.fromJson(receipt.live_after, `envelope row ${rowIndex} receipt ${receiptIndex}.live_after`);
    if (receipt.kind !== "Fill") spent.assertEquals(new Resource(), `envelope row ${rowIndex} non-Fill spent ${receiptIndex}`);
    if (receipt.kind === "Fill" && row.key.side === "Buy" && spent.cash === 0n) {
      fail(`envelope row ${rowIndex} receipt ${receiptIndex} Buy Fill must spend positive cash`);
    }
    if (receipt.kind === "Fill" && row.key.side === "Sell" && spent.shares === 0n) {
      fail(`envelope row ${rowIndex} receipt ${receiptIndex} Sell Fill must spend positive shares`);
    }
    if (row.key.side === "Sell" && (before.cash !== 0n || spent.cash !== 0n || released.cash !== 0n || after.cash !== 0n)) {
      fail(`envelope row ${rowIndex} Sell receipt ${receiptIndex} cash escrow/spent/released/live must all be zero`);
    }
    if (row.key.side === "Buy" && (before.shares !== 0n || spent.shares !== 0n || released.shares !== 0n || after.shares !== 0n)) {
      fail(`envelope row ${rowIndex} Buy receipt ${receiptIndex} shares escrow/spent/released/live must all be zero`);
    }
    before.assertEquals(this.live, `envelope row ${rowIndex} receipt chain ${receiptIndex}`);
    before.assertEquals(spent.add(released).add(after), `envelope row ${rowIndex} receipt equation ${receiptIndex}`);
    if (receipt.journal === "PreSeal") {
      spent.assertEquals(new Resource(), `envelope row ${rowIndex} PreSeal spent ${receiptIndex}`);
      this.expiryReleased = this.expiryReleased.add(released);
    } else {
      this.sealedSpent = this.sealedSpent.add(spent);
      this.sealedReleased = this.sealedReleased.add(released);
    }
    this.live = after;
  }

  assertCommitAndConservation() {
    const row = this.row;
    const rowIndex = this.rowIndex;
    if (!this.reachedSealed) this.assertP1Boundary();
    this.live.assertEquals(this.commitLive, `envelope row ${rowIndex} commit live`);
    const sealedTotal = this.sealedSpent.add(this.sealedReleased).add(this.commitLive);
    if (row.origin === "existing") {
      this.left.assertEquals(this.expiryReleased.add(this.allocationLive), `envelope row ${rowIndex} preseal conservation`);
      this.allocationLive.assertEquals(sealedTotal, `envelope row ${rowIndex} sealed conservation`);
    } else {
      this.left.assertEquals(sealedTotal, `envelope row ${rowIndex} created conservation`);
    }
    const right = this.expiryReleased.add(this.sealedSpent).add(this.sealedReleased.add(this.commitLive));
    this.left.assertEquals(right, `envelope row ${rowIndex} combined conservation`);
    return { left: this.left, right };
  }
}

export function verifyConservationSnapshot(snapshot) {
  exactKeys(snapshot, ["schema", "scenario", "seed", "tick", "envelopes", "accounts"], "conservation snapshot");
  if (snapshot.schema !== CONSERVATION_SCHEMA) fail("conservation snapshot schema is unsupported");
  if (typeof snapshot.scenario !== "string" || snapshot.scenario.length === 0) fail("conservation snapshot scenario is missing");
  decimal(snapshot.seed, "conservation snapshot seed");
  decimal(snapshot.tick, "conservation snapshot tick");
  if (!Array.isArray(snapshot.envelopes)) fail("conservation snapshot envelope rows must be an array");
  if (!Array.isArray(snapshot.accounts) || snapshot.accounts.length === 0) fail("conservation snapshot needs account aggregates");
  const keys = new Set();
  const receiptIndices = [];
  const receiptIdentities = [];
  const aggregates = new Map();

  for (const [rowIndex, row] of snapshot.envelopes.entries()) {
    exactKeys(row, ["key", "origin", "basis", "receipts", "commit_live"], `envelope row ${rowIndex}`);
    const serializedKey = envelopeKey(row.key, `envelope row ${rowIndex}.key`);
    if (keys.has(serializedKey)) fail(`duplicate envelope key at row ${rowIndex}`);
    keys.add(serializedKey);
    const envelope = new EnvelopeConservation(row, rowIndex);
    for (const [receiptIndex, receipt] of row.receipts.entries()) {
      const receiptLabel = `envelope row ${rowIndex} receipt ${receiptIndex}`;
      exactKeys(receipt, ["receipt_index", "journal", "source", "transition_ordinal_within_source", "kind", "live_before", "spent", "released", "live_after"], receiptLabel);
      const globalIndex = decimal(receipt.receipt_index, `envelope row ${rowIndex} receipt ${receiptIndex}.receipt_index`);
      receiptIndices.push(globalIndex);
      if (!new Set(["PreSeal", "SealedBatch"]).has(receipt.journal)) fail(`envelope row ${rowIndex} receipt ${receiptIndex}.journal is invalid`);
      if (!new Set(["Fill", "Release", "Reject", "Rollover"]).has(receipt.kind)) fail(`envelope row ${rowIndex} receipt ${receiptIndex}.kind is invalid`);
      if (receipt.journal === "PreSeal" && receipt.kind !== "Release") fail(`envelope row ${rowIndex} receipt ${receiptIndex} PreSeal must be a Release`);
      exactKeys(receipt.source, ["kind", "index"], `${receiptLabel}.source`);
      if (!RECEIPT_SOURCE_KINDS.has(receipt.source.kind)) fail(`${receiptLabel}.source.kind is invalid`);
      decimal(receipt.source.index, `${receiptLabel}.source.index`);
      const ordinal = decimal(receipt.transition_ordinal_within_source, `${receiptLabel}.transition_ordinal_within_source`);
      const expectedJournal = receipt.source.kind === "P0Expiry" ? "PreSeal" : "SealedBatch";
      if (receipt.journal !== expectedJournal) fail(`${receiptLabel} journal/source pairing is invalid`);
      const ordinalScope = `${receipt.journal}\u0000${receipt.source.kind}\u0000${receipt.source.index}\u0000${serializedKey}`;
      receiptIdentities.push({
        index: globalIndex,
        ordinalScope,
        ordinal,
      });
      envelope.applyReceipt(receipt, receiptIndex);
    }
    const { left, right } = envelope.assertCommitAndConservation();
    accumulateAccount(aggregates, row.key.account_id, "left", left);
    accumulateAccount(aggregates, row.key.account_id, "right", right);
  }

  const sortedIndices = [...receiptIndices].sort((left, right) => left < right ? -1 : left > right ? 1 : 0);
  if (new Set(sortedIndices.map(String)).size !== sortedIndices.length) fail("duplicate global receipt_index");
  for (let index = 1; index < sortedIndices.length; index += 1) {
    if (sortedIndices[index] !== sortedIndices[index - 1] + 1n) fail("global receipt_index has a gap");
  }
  receiptIdentities.sort((left, right) => compareBigInt(left.index, right.index));
  const receiptOrdinals = new Map();
  const seenIdentities = new Set();
  for (const identity of receiptIdentities) {
    const localIdentity = `${identity.ordinalScope}\u0000${identity.ordinal}`;
    if (seenIdentities.has(localIdentity)) fail("duplicate receipt identity");
    seenIdentities.add(localIdentity);
    const expectedOrdinal = receiptOrdinals.get(identity.ordinalScope) ?? 0n;
    if (identity.ordinal !== expectedOrdinal) fail("source-local transition ordinal must be zero-based and contiguous");
    receiptOrdinals.set(identity.ordinalScope, expectedOrdinal + 1n);
  }

  const accountIds = new Set();
  for (const [index, account] of snapshot.accounts.entries()) {
    exactKeys(account, ["account_id", "cash_cents", "positions", "aggregate"], `conservation account ${index}`);
    if (typeof account.account_id !== "string" || account.account_id.length === 0 || accountIds.has(account.account_id)) fail(`conservation account ${index}.account_id is invalid or duplicate`);
    accountIds.add(account.account_id);
    decimal(account.cash_cents, `conservation account ${index}.cash_cents`);
    if (!Array.isArray(account.positions)) fail(`conservation account ${index}.positions must be an array`);
    const positionCodes = new Set();
    for (const [positionIndex, position] of account.positions.entries()) {
      exactKeys(position, ["stock_code", "qty", "t1_locked"], `conservation account ${index} position ${positionIndex}`);
      if (typeof position.stock_code !== "string" || !/^[0-9]{6}$/.test(position.stock_code) || positionCodes.has(position.stock_code)) fail(`conservation account ${index} position ${positionIndex} stock code is invalid or duplicate`);
      positionCodes.add(position.stock_code);
      const qty = decimal(position.qty, `conservation account ${index} position ${positionIndex}.qty`);
      const locked = decimal(position.t1_locked, `conservation account ${index} position ${positionIndex}.t1_locked`);
      if (locked > qty) fail(`conservation account ${index} position ${positionIndex} t1_locked exceeds qty`);
    }
    exactKeys(account.aggregate, ["left", "right"], `conservation account ${index}.aggregate`);
    const claimed = {
      left: Resource.fromJson(account.aggregate.left, `conservation account ${index}.aggregate.left`),
      right: Resource.fromJson(account.aggregate.right, `conservation account ${index}.aggregate.right`),
    };
    const computed = aggregates.get(account.account_id) ?? { left: new Resource(), right: new Resource() };
    claimed.left.assertEquals(computed.left, `conservation account ${index} claimed left aggregate`);
    claimed.right.assertEquals(computed.right, `conservation account ${index} claimed right aggregate`);
    claimed.left.assertEquals(claimed.right, `conservation account ${index} aggregate conservation`);
  }
  for (const accountId of aggregates.keys()) if (!accountIds.has(accountId)) fail(`missing account aggregate for envelope owner ${accountId}`);
  return { scenario: snapshot.scenario, seed: snapshot.seed, tick: snapshot.tick, envelope_rows: snapshot.envelopes.length, account_rows: snapshot.accounts.length, receipt_rows: receiptIndices.length };
}
