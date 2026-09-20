#!/usr/bin/env node
import { createHash } from "node:crypto";
import fsp from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const OBSERVATION_SCHEMA = "escrow-determinism-observation-v1";
const CONSERVATION_SCHEMA = "escrow-conservation-snapshot-v1";
const CORPUS_SCHEMA = "escrow-corpus-projection-v1";
const BUNDLE_SCHEMA = "escrow-verification-bundle-v1";
const BUDGETS = ["1", "2", "4", "auto"];
const ARTIFACT_NAMES = ["authoritative_state", "event_stream", "receipts", "save_slot"];
const CORPUS_CLASSES = new Set(["equivalence", "divergence-9", "controlled-live-sell", "stress"]);
const EFFECTS = new Map([
  [9, new Set(["sell_reservation", "acceptance_flip", "fee_charged", "net_delivery", "fee_event_payload", "terminal_cash_equation"])],
  [7, new Set(["save_representation"])],
]);
const STOCK_EVENT_VARIANTS = new Set(["Trade", "AuctionTick", "AuctionCompleted", "PriceTick"]);
const ACCOUNT_EVENT_VARIANTS = new Set(["IntentRejected", "SettlementError", "OrderCanceled", "OrderAccepted"]);
const SESSION_EVENT_VARIANTS = new Set(["DayBoundary", "CivilDateAdvanced", "CompanyDisclosurePublished", "ResourceLimit"]);
const RECEIPT_SOURCE_RANK = new Map([["P0Expiry", 0], ["SealedIntent", 0], ["Auction", 1], ["DayEnd", 2]]);
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
  "cumulative_volume", "account", "id", "order_id", "remaining_qty", "publication_id",
  "second_of_day", "limit", "ticks_per_day", "snapshot_tick", "snapshot_seq", "intraday_ticks",
  "phase_rank", "local_event_index", "imbalance", "turnover_cents",
]);
const CONTROL_HASH_FIELDS = ["sealed_exogenous_script_sha256", "strategy_state_sha256", "plan_state_sha256", "pending_intents_sha256", "restore_order_sha256"];
const EFFECT_PATH_PATTERNS = new Map([
  ["sell_reservation", [/(?:^|\/)(?:reserved_cash|reserved_cash_cents|cash_escrow_cents|sell_reservation_cents)$/]],
  ["acceptance_flip", [/^\/state\/(?:.*\/)?(?:acceptance|accepted|status|rejection_reason)$/, /^\/updates\/(?:\*|\d+)\/facts\/\*\*$/]],
  ["fee_charged", [/(?:^|\/)(?:charged(?:_[a-z]+)*_cents|fee(?:_[a-z]+)*_cents|commission_charged_cents|stamp_duty_charged_cents|transfer_fee_charged_cents)$/]],
  ["net_delivery", [/(?:^|\/)(?:deliver_cash_cents|net_delivery_cents|net_cash_cents)$/]],
  ["fee_event_payload", [/^\/updates\/(?:\*|\d+)\/facts\/(?:\*|\d+)\/payload\/(?:fee|charged|commission|stamp_duty|transfer_fee|deliver_cash|net_cash)(?:_[a-z]+)*_cents$/]],
  ["terminal_cash_equation", [/^\/seller_fee_control\/terminal_cash_cents$/]],
  ["save_representation", [/^\/state\/(?:save_representation|save_schema|save_slot)(?:\/|$)/]],
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

function canonical(value) {
  if (Array.isArray(value)) return value.map(canonical);
  if (isRecord(value)) return Object.fromEntries(Object.keys(value).sort().map((key) => [key, canonical(value[key])]));
  return value;
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

function validateObservation(observation, label) {
  exactKeys(observation, ["schema", "scenario", "seed", "budget", "repeat", "mode", "canonical_merge_disabled", "artifacts", "precanonical_order", "execution_coverage"], label);
  if (observation.schema !== OBSERVATION_SCHEMA) fail(`${label}.schema is unsupported`);
  if (typeof observation.scenario !== "string" || observation.scenario.length === 0) fail(`${label}.scenario is missing`);
  decimal(observation.seed, `${label}.seed`);
  if (!BUDGETS.includes(observation.budget)) fail(`${label}.budget is invalid`);
  decimal(observation.repeat, `${label}.repeat`);
  if (!new Set(["canonical", "perturbed", "negative-control"]).has(observation.mode)) fail(`${label}.mode is invalid`);
  const disabled = observation.canonical_merge_disabled;
  if (observation.mode === "negative-control") {
    if (!new Set(["account", "stock", "completion"]).has(disabled)) fail(`${label}.canonical_merge_disabled is invalid`);
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
  if (!Array.isArray(negativeControls) || negativeControls.length !== 3) fail("perturbation gate requires exactly three negative controls");
  const observedDisabled = new Set();
  for (const [index, candidate] of negativeControls.entries()) {
    validateObservation(candidate, `perturbation negative control ${index}`);
    if (candidate.mode !== "negative-control") fail(`negative control ${index} mode mismatch`);
    if (observationIdentity(candidate) !== observationIdentity(reference) || candidate.budget !== reference.budget) fail(`negative control ${index} identity or budget mismatch`);
    if (observedDisabled.has(candidate.canonical_merge_disabled)) fail(`duplicate negative control ${candidate.canonical_merge_disabled}`);
    observedDisabled.add(candidate.canonical_merge_disabled);
    if (jsonEqual(candidate.artifacts, reference.artifacts)) fail(`negative control ${candidate.canonical_merge_disabled} did not expose a canonicalization failure`);
  }
  requireJsonEqual([...observedDisabled].sort(), ["account", "completion", "stock"], "perturbation disabled-merge coverage");
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
  if (typeof value === "number") fail(`${label} contains a JSON number; evidence integers must use decimal strings`);
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

function compareText(left, right) {
  return left < right ? -1 : left > right ? 1 : 0;
}

function compareReceiptLocalKey(left, right) {
  const scalarComparisons = [
    [left.journalRank, right.journalRank, compareBigInt],
    [left.sourceRank, right.sourceRank, compareBigInt],
    [left.sourceIndex, right.sourceIndex, compareBigInt],
    [left.account, right.account, compareBigInt],
    [left.stock, right.stock, compareText],
    [left.order, right.order, compareBigInt],
    [left.sideRank, right.sideRank, compareBigInt],
    [left.ordinal, right.ordinal, compareBigInt],
  ];
  for (const [a, b, compare] of scalarComparisons) {
    const ordering = compare(a, b);
    if (ordering !== 0) return ordering;
  }
  return 0;
}

function resource(value, label) {
  exactKeys(value, ["cash_cents", "shares"], label);
  return { cash: decimal(value.cash_cents, `${label}.cash_cents`), shares: decimal(value.shares, `${label}.shares`) };
}

function add(left, right) {
  return { cash: left.cash + right.cash, shares: left.shares + right.shares };
}

function equalResource(left, right) {
  return left.cash === right.cash && left.shares === right.shares;
}

function requireResourceEqual(left, right, label) {
  if (!equalResource(left, right)) fail(`${label} mismatch: cash ${left.cash}/${right.cash}, shares ${left.shares}/${right.shares}`);
}

function envelopeKey(key, label) {
  exactKeys(key, ["account_id", "stock_code", "order_id", "side"], label);
  decimal(key.account_id, `${label}.account_id`);
  if (typeof key.stock_code !== "string" || !/^[0-9]{6}$/.test(key.stock_code)) fail(`${label}.stock_code is invalid`);
  if (typeof key.order_id !== "string" || !/^(0|[1-9][0-9]*)$/.test(key.order_id)) fail(`${label}.order_id is invalid`);
  if (!new Set(["Buy", "Sell"]).has(key.side)) fail(`${label}.side is invalid`);
  return JSON.stringify(key);
}

function emptyResource() {
  return { cash: 0n, shares: 0n };
}

function accumulateAccount(map, account, side, value) {
  const row = map.get(account) ?? { left: emptyResource(), right: emptyResource() };
  row[side] = add(row[side], value);
  map.set(account, row);
}

export function verifyConservationSnapshot(snapshot) {
  exactKeys(snapshot, ["schema", "scenario", "seed", "tick", "envelopes", "accounts"], "conservation snapshot");
  if (snapshot.schema !== CONSERVATION_SCHEMA) fail("conservation snapshot schema is unsupported");
  if (typeof snapshot.scenario !== "string" || snapshot.scenario.length === 0) fail("conservation snapshot scenario is missing");
  decimal(snapshot.seed, "conservation snapshot seed");
  decimal(snapshot.tick, "conservation snapshot tick");
  if (!Array.isArray(snapshot.envelopes) || snapshot.envelopes.length === 0) fail("conservation snapshot needs at least one envelope");
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
    if (!new Set(["existing", "created"]).has(row.origin)) fail(`envelope row ${rowIndex}.origin is invalid`);
    const commitLive = resource(row.commit_live, `envelope row ${rowIndex}.commit_live`);
    if (!Array.isArray(row.receipts)) fail(`envelope row ${rowIndex}.receipts must be an array`);
    let live;
    let left;
    let p1Live;
    if (row.origin === "existing") {
      exactKeys(row.basis, ["tick_start_live", "p1_live"], `envelope row ${rowIndex}.basis`);
      live = resource(row.basis.tick_start_live, `envelope row ${rowIndex}.basis.tick_start_live`);
      p1Live = resource(row.basis.p1_live, `envelope row ${rowIndex}.basis.p1_live`);
      left = live;
    } else {
      exactKeys(row.basis, ["created"], `envelope row ${rowIndex}.basis`);
      live = resource(row.basis.created, `envelope row ${rowIndex}.basis.created`);
      left = live;
    }
    if (row.key.side === "Sell" && live.cash !== 0n) fail(`envelope row ${rowIndex} Sell basis cash escrow must be zero`);
    if (row.key.side === "Sell" && commitLive.cash !== 0n) fail(`envelope row ${rowIndex} Sell commit cash escrow must be zero`);
    if (row.key.side === "Buy" && live.shares !== 0n) fail(`envelope row ${rowIndex} Buy basis shares escrow must be zero`);
    if (row.key.side === "Buy" && p1Live?.shares !== 0n) fail(`envelope row ${rowIndex} Buy P1 basis shares escrow must be zero`);
    if (row.key.side === "Buy" && commitLive.shares !== 0n) fail(`envelope row ${rowIndex} Buy commit shares escrow must be zero`);
    let p0Released = emptyResource();
    let sealedSpent = emptyResource();
    let sealedReleased = emptyResource();
    let reachedSealed = false;
    for (const [receiptIndex, receipt] of row.receipts.entries()) {
      const receiptLabel = `envelope row ${rowIndex} receipt ${receiptIndex}`;
      exactKeys(receipt, ["receipt_index", "journal", "source", "transition_ordinal_within_source", "kind", "live_before", "spent", "released", "live_after"], receiptLabel);
      const globalIndex = decimal(receipt.receipt_index, `envelope row ${rowIndex} receipt ${receiptIndex}.receipt_index`);
      receiptIndices.push(globalIndex);
      if (!new Set(["PreSeal", "SealedBatch"]).has(receipt.journal)) fail(`envelope row ${rowIndex} receipt ${receiptIndex}.journal is invalid`);
      if (!new Set(["Fill", "Release", "Reject", "Rollover"]).has(receipt.kind)) fail(`envelope row ${rowIndex} receipt ${receiptIndex}.kind is invalid`);
      if (receipt.journal === "PreSeal" && receipt.kind !== "Release") fail(`envelope row ${rowIndex} receipt ${receiptIndex} PreSeal must be a Release`);
      exactKeys(receipt.source, ["kind", "index"], `${receiptLabel}.source`);
      if (!RECEIPT_SOURCE_RANK.has(receipt.source.kind)) fail(`${receiptLabel}.source.kind is invalid`);
      const sourceIndex = decimal(receipt.source.index, `${receiptLabel}.source.index`);
      const ordinal = decimal(receipt.transition_ordinal_within_source, `${receiptLabel}.transition_ordinal_within_source`);
      const expectedJournal = receipt.source.kind === "P0Expiry" ? "PreSeal" : "SealedBatch";
      if (receipt.journal !== expectedJournal) fail(`${receiptLabel} journal/source pairing is invalid`);
      const ordinalScope = `${receipt.journal}\u0000${receipt.source.kind}\u0000${receipt.source.index}\u0000${serializedKey}`;
      receiptIdentities.push({
        index: globalIndex,
        ordinalScope,
        local: {
          journalRank: receipt.journal === "PreSeal" ? 0n : 1n,
          sourceRank: BigInt(RECEIPT_SOURCE_RANK.get(receipt.source.kind)),
          sourceIndex,
          account: BigInt(row.key.account_id),
          stock: row.key.stock_code,
          order: BigInt(row.key.order_id),
          sideRank: row.key.side === "Buy" ? 0n : 1n,
          ordinal,
        },
      });
      if (row.origin === "created" && receipt.journal === "PreSeal") fail(`created envelope row ${rowIndex} has a P0 contribution`);
      if (reachedSealed && receipt.journal === "PreSeal") fail(`envelope row ${rowIndex} returns to PreSeal after SealedBatch`);
      if (receipt.journal === "SealedBatch" && !reachedSealed) {
        if (row.origin === "existing") requireResourceEqual(live, p1Live, `envelope row ${rowIndex} P1 boundary`);
        reachedSealed = true;
      }
      const before = resource(receipt.live_before, `envelope row ${rowIndex} receipt ${receiptIndex}.live_before`);
      const spent = resource(receipt.spent, `envelope row ${rowIndex} receipt ${receiptIndex}.spent`);
      const released = resource(receipt.released, `envelope row ${rowIndex} receipt ${receiptIndex}.released`);
      const after = resource(receipt.live_after, `envelope row ${rowIndex} receipt ${receiptIndex}.live_after`);
      if (receipt.kind !== "Fill") requireResourceEqual(spent, emptyResource(), `envelope row ${rowIndex} non-Fill spent ${receiptIndex}`);
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
      requireResourceEqual(before, live, `envelope row ${rowIndex} receipt chain ${receiptIndex}`);
      requireResourceEqual(before, add(add(spent, released), after), `envelope row ${rowIndex} receipt equation ${receiptIndex}`);
      if (receipt.journal === "PreSeal") {
        requireResourceEqual(spent, emptyResource(), `envelope row ${rowIndex} PreSeal spent ${receiptIndex}`);
        p0Released = add(p0Released, released);
      } else {
        sealedSpent = add(sealedSpent, spent);
        sealedReleased = add(sealedReleased, released);
      }
      live = after;
    }
    if (row.origin === "existing" && !reachedSealed) requireResourceEqual(live, p1Live, `envelope row ${rowIndex} P1 boundary`);
    requireResourceEqual(live, commitLive, `envelope row ${rowIndex} commit live`);
    const sealedTotal = add(add(sealedSpent, sealedReleased), commitLive);
    if (row.origin === "existing") {
      requireResourceEqual(left, add(p0Released, p1Live), `envelope row ${rowIndex} preseal conservation`);
      requireResourceEqual(p1Live, sealedTotal, `envelope row ${rowIndex} sealed conservation`);
    } else {
      requireResourceEqual(left, sealedTotal, `envelope row ${rowIndex} created conservation`);
    }
    const right = add(add(p0Released, sealedSpent), add(sealedReleased, commitLive));
    requireResourceEqual(left, right, `envelope row ${rowIndex} combined conservation`);
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
  for (const identity of receiptIdentities) {
    const expectedOrdinal = receiptOrdinals.get(identity.ordinalScope) ?? 0n;
    if (identity.local.ordinal !== expectedOrdinal) fail("source-local transition ordinal must be zero-based and contiguous");
    receiptOrdinals.set(identity.ordinalScope, expectedOrdinal + 1n);
  }
  for (let index = 1; index < receiptIdentities.length; index += 1) {
    if (compareReceiptLocalKey(receiptIdentities[index - 1].local, receiptIdentities[index].local) >= 0) {
      fail("global receipt_index order disagrees with canonical ReceiptLocalKey order");
    }
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
      left: resource(account.aggregate.left, `conservation account ${index}.aggregate.left`),
      right: resource(account.aggregate.right, `conservation account ${index}.aggregate.right`),
    };
    const computed = aggregates.get(account.account_id) ?? { left: emptyResource(), right: emptyResource() };
    requireResourceEqual(claimed.left, computed.left, `conservation account ${index} claimed left aggregate`);
    requireResourceEqual(claimed.right, computed.right, `conservation account ${index} claimed right aggregate`);
    requireResourceEqual(claimed.left, claimed.right, `conservation account ${index} aggregate conservation`);
  }
  for (const accountId of aggregates.keys()) if (!accountIds.has(accountId)) fail(`missing account aggregate for envelope owner ${accountId}`);
  return { scenario: snapshot.scenario, seed: snapshot.seed, tick: snapshot.tick, envelope_rows: snapshot.envelopes.length, account_rows: snapshot.accounts.length, receipt_rows: receiptIndices.length };
}

function eventFact(fact, tick, label) {
  exactKeys(fact, ["comparison_event_key", "event"], label);
  if (!Array.isArray(fact.comparison_event_key) || fact.comparison_event_key.length !== 4) fail(`${label}.comparison_event_key is malformed`);
  const [eventTickText, taggedVariant, entity, ordinalText] = fact.comparison_event_key;
  if (![eventTickText, taggedVariant, entity, ordinalText].every((part) => typeof part === "string")) {
    fail(`${label}.comparison_event_key is invalid`);
  }
  const eventTick = decimal(eventTickText, `${label}.comparison_event_key[0]`);
  const ordinal = decimal(ordinalText, `${label}.comparison_event_key[3]`);
  if (eventTick !== tick) fail(`${label}.comparison_event_key tick disagrees with its update`);
  const match = /^(0|[1-9][0-9]*):([A-Za-z][A-Za-z0-9]*)$/.exec(taggedVariant);
  if (!match) fail(`${label}.comparison_event_key phase-tagged variant is invalid`);
  const phaseRank = BigInt(match[1]);
  const variant = match[2];
  const variants = Object.entries(requireRecord(fact.event, `${label}.event`));
  if (variants.length !== 1 || variants[0][0] !== variant) fail(`${label} event variant and comparison key disagree`);
  const payload = requireRecord(variants[0][1], `${label}.${variant}`);
  const seq = decimal(payload.seq, `${label}.${variant}.seq`);
  validateProjectedIntegers(payload, `${label}.${variant}`);
  let expectedEntity;
  let expectedPhase;
  let expectedSource;
  if (STOCK_EVENT_VARIANTS.has(variant)) {
    if (typeof payload.code !== "string" || !/^[0-9]{6}$/.test(payload.code)) fail(`${label}.${variant}.code cannot derive a Stock entity`);
    expectedEntity = `Stock:${payload.code}`;
    expectedPhase = 4n;
    expectedSource = variant === "Trade" ? "Sealed" : "PriceTick";
  } else if (ACCOUNT_EVENT_VARIANTS.has(variant)) {
    decimal(payload.account, `${label}.${variant}.account`);
    expectedEntity = `Account:${payload.account}`;
    expectedPhase = 4n;
    expectedSource = "Sealed";
    if (new Set(["OrderAccepted", "OrderCanceled"]).has(variant)) {
      const orderId = payload.id ?? payload.order_id;
      decimal(orderId, `${label}.${variant}.order_id`);
    }
  } else if (SESSION_EVENT_VARIANTS.has(variant)) {
    expectedEntity = "Session";
    expectedPhase = variant === "DayBoundary" ? 5n : 6n;
    expectedSource = variant === "DayBoundary" ? "DayEnd" : "Session";
  } else {
    fail(`${label}.${variant} has no exhaustive comparison entity mapping`);
  }
  if (phaseRank !== expectedPhase) fail(`${label}.${variant} phase ${phaseRank} does not equal derived ${expectedPhase}`);
  if (entity !== expectedEntity) fail(`${label}.${variant} entity ${entity} does not equal derived ${expectedEntity}`);
  const { seq: _seq, ...business } = payload;
  return { key: fact.comparison_event_key, tick: eventTick, phaseRank, source: expectedSource, entity, ordinal, variant, payload: canonical(business), seq };
}

function validateEventOrdinals(facts, label) {
  const scopes = new Map();
  for (const fact of facts) {
    const scope = `${fact.tick}\u0000${fact.phaseRank}\u0000${fact.entity}\u0000${fact.source}`;
    const ordinals = scopes.get(scope) ?? [];
    ordinals.push(fact.ordinal);
    scopes.set(scope, ordinals);
  }
  for (const [scope, ordinals] of scopes) {
    ordinals.sort(compareBigInt);
    ordinals.forEach((ordinal, index) => {
      if (ordinal !== BigInt(index)) fail(`${label} comparison ordinal scope ${scope} must be zero-based and contiguous`);
    });
  }
}

function normalizeUpdates(updates, label) {
  if (!Array.isArray(updates) || updates.length === 0) fail(`${label} must contain updates`);
  let previous;
  const allFacts = [];
  const normalized = updates.map((update, index) => {
    const updateLabel = `${label} update ${index}`;
    if (!isRecord(update) || !new Set(["TickFrame", "CivilUpdate"]).has(update.kind)) fail(`${updateLabel}.kind is invalid`);
    const payloadField = update.kind === "TickFrame" ? "timeseries_payload" : "civil_payload";
    exactKeys(update, ["kind", "tick", "seq_from", "seq_to", payloadField, "events"], updateLabel);
    const tick = decimal(update.tick, `${updateLabel}.tick`);
    const seqFrom = decimal(update.seq_from, `${updateLabel}.seq_from`);
    const seqTo = decimal(update.seq_to, `${updateLabel}.seq_to`);
    if (seqTo < seqFrom || !Array.isArray(update.events) || update.events.length === 0 || seqTo - seqFrom + 1n !== BigInt(update.events.length)) fail(`${updateLabel} seq range cardinality mismatch`);
    if (previous) {
      if (seqFrom !== previous.seqTo + 1n) fail(`${updateLabel} seq coverage gap`);
      const expectedTick = update.kind === "CivilUpdate" ? previous.tick : previous.tick + 1n;
      if (tick !== expectedTick) fail(`${updateLabel} tick gap or boundary mismatch`);
    }
    validateProjectedIntegers(update[payloadField], `${updateLabel}.${payloadField}`);
    const facts = update.events.map((fact, factIndex) => eventFact(fact, tick, `${updateLabel} fact ${factIndex}`));
    const identities = facts.map((fact) => JSON.stringify(fact.key));
    if (new Set(identities).size !== identities.length) fail(`${updateLabel} has duplicate comparison event identity`);
    allFacts.push(...facts);
    const seqs = facts.map((fact) => fact.seq).sort(compareBigInt);
    seqs.forEach((seq, seqIndex) => {
      if (seq !== seqFrom + BigInt(seqIndex)) fail(`${updateLabel} seq duplicate or gap`);
    });
    previous = { tick, seqTo };
    return {
      kind: update.kind,
      tick: update.tick,
      payload: canonical(update[payloadField]),
      facts: facts.map(({ key, variant, payload }) => ({ key, variant, payload })).sort((left, right) => JSON.stringify(left.key).localeCompare(JSON.stringify(right.key))),
    };
  });
  validateEventOrdinals(allFacts, label);
  return normalized;
}

function pointerEscape(value) {
  return value.replaceAll("~", "~0").replaceAll("/", "~1");
}

function diffJson(left, right, pointer = "") {
  if (Object.is(left, right)) return [];
  if (Array.isArray(left) && Array.isArray(right)) {
    const differences = [];
    const length = Math.max(left.length, right.length);
    for (let index = 0; index < length; index += 1) differences.push(...diffJson(left[index], right[index], `${pointer}/${index}`));
    return differences;
  }
  if (isRecord(left) && isRecord(right)) {
    const differences = [];
    const keys = [...new Set([...Object.keys(left), ...Object.keys(right)])].sort();
    for (const key of keys) differences.push(...diffJson(left[key], right[key], `${pointer}/${pointerEscape(key)}`));
    return differences;
  }
  return [{ path: pointer || "/", legacy: left, current: right }];
}

function pathMatches(pattern, actual) {
  const expectedParts = pattern.split("/");
  const actualParts = actual.split("/");
  if (expectedParts.at(-1) === "**") {
    if (actualParts.length < expectedParts.length - 1) return false;
    expectedParts.pop();
  } else if (expectedParts.length !== actualParts.length) return false;
  return expectedParts.every((part, index) => part === "*" || part === actualParts[index]);
}

function pathAllowedForEffect(effect, path) {
  return EFFECT_PATH_PATTERNS.get(effect)?.some((pattern) => pattern.test(path)) ?? false;
}

function validateTransformations(transformations, corpusClass) {
  if (!Array.isArray(transformations)) fail("corpus transformations must be an array");
  const allowedDivergences = corpusClass === "divergence-9" ? new Set([9]) : corpusClass === "controlled-live-sell" ? new Set([7, 9]) : new Set();
  return transformations.map((transformation, index) => {
    exactKeys(transformation, ["divergence", "effect", "path"], `corpus transformation ${index}`);
    if (!allowedDivergences.has(transformation.divergence)) fail(`corpus transformation ${index} divergence is not allowed for class ${corpusClass}`);
    if (!EFFECTS.get(transformation.divergence)?.has(transformation.effect)) fail(`corpus transformation ${index} effect is unsupported`);
    if (typeof transformation.path !== "string" || !transformation.path.startsWith("/") || transformation.path.includes("..")) fail(`corpus transformation ${index} path is unsafe`);
    if (!pathAllowedForEffect(transformation.effect, transformation.path)) fail(`corpus transformation ${index} effect ${transformation.effect} is not valid for path ${transformation.path}`);
    return transformation;
  });
}

function validateOptionalHash(value, label, required = false) {
  if (value === null && !required) return;
  if (typeof value !== "string" || !/^[a-f0-9]{64}$/.test(value)) fail(`${label} must be ${required ? "a" : "null or a"} sha256 digest`);
}

function validateSurfaceSubject(subject, expectedSide, label) {
  if (typeof subject.account_id !== "string" || subject.account_id.length === 0) fail(`${label}.account_id is invalid`);
  if (typeof subject.stock_code !== "string" || !/^[0-9]{6}$/.test(subject.stock_code)) fail(`${label}.stock_code is invalid`);
  if (subject.side !== expectedSide) fail(`${label}.side must be ${expectedSide}`);
  const roles = new Set([`maker-${expectedSide.toLowerCase()}`, `taker-${expectedSide.toLowerCase()}`]);
  if (!roles.has(subject.trade_role)) fail(`${label}.trade_role must identify the ${expectedSide} maker or taker leg`);
}

function validateCorpusControl(control, corpusClass, label) {
  exactKeys(control, [
    "surface", "seller_order_count", "old_sell_reservation_cents", "fee_prefixes", "feedback",
    "acceptance", "zero_cash_acceptance", "sealed_exogenous_script_sha256", "rng_cursor",
    "strategy_state_sha256", "plan_state_sha256", "pending_intents_sha256", "restore_order_sha256",
    "comparison_points", "surface_evidence",
  ], label);
  const surfaces = {
    equivalence: new Set(["normal-multi-leg-terminal", "buyer-fees", "t1", "price-cage", "continuous-buy-leg"]),
    "divergence-9": new Set(["acceptance-flip", "three-leg-fee-catchup"]),
    "controlled-live-sell": new Set(["auction-rollover", "cross-tick-partial-fill", "save-restore-live-order"]),
    stress: new Set(["long-run-multi-stock"]),
  };
  if (!surfaces[corpusClass].has(control.surface)) fail(`${label}.surface is invalid for ${corpusClass}`);
  const sellerOrderCount = decimal(control.seller_order_count, `${label}.seller_order_count`);
  decimal(control.old_sell_reservation_cents, `${label}.old_sell_reservation_cents`);
  if (!Array.isArray(control.fee_prefixes)) fail(`${label}.fee_prefixes must be an array`);
  control.fee_prefixes.forEach((prefix, index) => {
    exactKeys(prefix, ["nominal_cents", "charged_cents"], `${label}.fee_prefixes[${index}]`);
    const nominal = decimal(prefix.nominal_cents, `${label}.fee_prefixes[${index}].nominal_cents`);
    const charged = decimal(prefix.charged_cents, `${label}.fee_prefixes[${index}].charged_cents`);
    if (charged > nominal) fail(`${label}.fee_prefixes[${index}] charged exceeds nominal`);
    if (index > 0) {
      const previous = control.fee_prefixes[index - 1];
      if (nominal < BigInt(previous.nominal_cents) || charged < BigInt(previous.charged_cents)) fail(`${label}.fee_prefixes must be cumulative and monotonic`);
    }
  });
  exactKeys(control.feedback, ["strategy_generated_intents", "plan_generated_intents", "state_dependent_intents"], `${label}.feedback`);
  const feedbackValues = Object.entries(control.feedback).map(([field, value]) => decimal(value, `${label}.feedback.${field}`));
  if (!new Set(["accepted", "rejected", "not-applicable"]).has(control.acceptance)) fail(`${label}.acceptance is invalid`);
  if (typeof control.zero_cash_acceptance !== "boolean") fail(`${label}.zero_cash_acceptance must be boolean`);
  validateOptionalHash(control.sealed_exogenous_script_sha256, `${label}.sealed_exogenous_script_sha256`, corpusClass === "controlled-live-sell");
  if (control.rng_cursor !== null && !(typeof control.rng_cursor === "string" && /^(0|[1-9][0-9]*)$/.test(control.rng_cursor))) fail(`${label}.rng_cursor is invalid`);
  for (const field of CONTROL_HASH_FIELDS.slice(1)) validateOptionalHash(control[field], `${label}.${field}`, corpusClass === "controlled-live-sell");
  if (!Array.isArray(control.comparison_points) || control.comparison_points.length === 0
    || !control.comparison_points.every((point) => typeof point === "string" && point.length > 0)
    || new Set(control.comparison_points).size !== control.comparison_points.length) {
    fail(`${label}.comparison_points must be a non-empty unique list`);
  }
  if (control.surface_evidence !== null) requireRecord(control.surface_evidence, `${label}.surface_evidence`);
  const feedbackCount = feedbackValues.reduce((total, value) => total + value, 0n);
  if (corpusClass !== "stress" && feedbackCount !== 0n) fail(`${label} ${corpusClass} feedback must be disabled`);
  if (new Set(["divergence-9", "controlled-live-sell"]).has(corpusClass) && sellerOrderCount !== 1n) fail(`${label} ${corpusClass} must contain exactly one seller order`);
  if (corpusClass === "equivalence") {
    if (control.old_sell_reservation_cents !== "0") fail(`${label} equivalence old sell reservation must be zero`);
    if (control.acceptance !== "accepted") fail(`${label} equivalence must be accepted on both sides`);
    if (control.sealed_exogenous_script_sha256 !== null) fail(`${label} equivalence must not declare a feedback continuation script`);
    if (control.surface === "normal-multi-leg-terminal") {
      if (sellerOrderCount !== 1n || control.fee_prefixes.length < 2 || control.fee_prefixes.some((prefix) => prefix.charged_cents !== prefix.nominal_cents)) fail(`${label} equivalence seller fee prefixes must all have charged == nominal and include multiple legs`);
      if (!control.zero_cash_acceptance || control.surface_evidence !== null) fail(`${label} equivalence seller surface must prove zero-cash acceptance without unrelated evidence`);
    } else {
      if (sellerOrderCount !== 0n || control.fee_prefixes.length !== 0 || control.zero_cash_acceptance) fail(`${label} equivalence ${control.surface} must not masquerade as a seller-fee surface`);
      const evidence = requireRecord(control.surface_evidence, `${label}.${control.surface} evidence`);
      if (control.surface === "buyer-fees") {
        exactKeys(evidence, ["account_id", "stock_code", "side", "trade_role", "gross_cents", "commission_cents", "transfer_fee_cents", "spent_cash_cents"], `${label}.buyer-fees evidence`);
        validateSurfaceSubject(evidence, "Buy", `${label}.buyer-fees evidence`);
        const gross = decimal(evidence.gross_cents, `${label}.buyer-fees gross`);
        const commission = decimal(evidence.commission_cents, `${label}.buyer-fees commission`);
        const transfer = decimal(evidence.transfer_fee_cents, `${label}.buyer-fees transfer`);
        const spent = decimal(evidence.spent_cash_cents, `${label}.buyer-fees spent`);
        if (spent !== gross + commission + transfer) fail(`${label}.buyer-fees spent must equal gross + commission + transfer`);
      } else if (control.surface === "t1") {
        exactKeys(evidence, ["account_id", "stock_code", "side", "trade_role", "qty_before", "bought_qty", "qty_after", "t1_locked_before", "t1_locked_after"], `${label}.t1 evidence`);
        validateSurfaceSubject(evidence, "Buy", `${label}.t1 evidence`);
        const qtyBefore = decimal(evidence.qty_before, `${label}.t1 qty_before`);
        const bought = decimal(evidence.bought_qty, `${label}.t1 bought_qty`);
        const qtyAfter = decimal(evidence.qty_after, `${label}.t1 qty_after`);
        const lockedBefore = decimal(evidence.t1_locked_before, `${label}.t1 locked_before`);
        const lockedAfter = decimal(evidence.t1_locked_after, `${label}.t1 locked_after`);
        if (bought === 0n || qtyAfter !== qtyBefore + bought || lockedAfter !== lockedBefore + bought || lockedAfter > qtyAfter) fail(`${label}.t1 evidence does not prove bought shares became locked`);
      } else if (control.surface === "price-cage") {
        exactKeys(evidence, ["account_id", "stock_code", "side", "outside_rejection", "inside_acceptance", "inside_order_id"], `${label}.price-cage evidence`);
        if (typeof evidence.account_id !== "string" || evidence.account_id.length === 0
          || typeof evidence.stock_code !== "string" || !/^[0-9]{6}$/.test(evidence.stock_code)
          || evidence.side !== "Buy" || evidence.outside_rejection !== "PriceCageExceeded" || evidence.inside_acceptance !== "accepted"
          || !/^(0|[1-9][0-9]*)$/.test(evidence.inside_order_id)) {
          fail(`${label}.price-cage evidence must prove an outside Buy rejection and inside Buy acceptance`);
        }
      } else if (control.surface === "continuous-buy-leg") {
        exactKeys(evidence, ["account_id", "stock_code", "side", "trade_role", "order_id", "trade_leg_count", "stable_order_identity_count"], `${label}.continuous-buy-leg evidence`);
        validateSurfaceSubject(evidence, "Buy", `${label}.continuous-buy-leg evidence`);
        if (!/^(0|[1-9][0-9]*)$/.test(evidence.order_id)
          || decimal(evidence.trade_leg_count, `${label}.continuous-buy-leg trade_leg_count`) <= 0n
          || decimal(evidence.stable_order_identity_count, `${label}.continuous-buy-leg stable_order_identity_count`) <= 0n) fail(`${label}.continuous-buy-leg evidence must prove a real Buy execution leg and order identity`);
      }
    }
  }
  if (corpusClass === "divergence-9" && control.surface === "acceptance-flip") {
    const subject = requireRecord(control.surface_evidence, `${label}.acceptance-flip subject`);
    exactKeys(subject, ["account_id", "stock_code", "side", "trade_role"], `${label}.acceptance-flip subject`);
    validateSurfaceSubject(subject, "Sell", `${label}.acceptance-flip subject`);
  }
  if (corpusClass === "divergence-9" && control.surface === "three-leg-fee-catchup" && control.surface_evidence !== null) {
    fail(`${label}.three-leg-fee-catchup must not declare acceptance subject evidence`);
  }
  if (corpusClass === "controlled-live-sell" && (control.acceptance !== "accepted" || control.fee_prefixes.length === 0)) fail(`${label} controlled-live-sell must be accepted and carry cumulative fee prefixes`);
  if (corpusClass === "controlled-live-sell") {
    const requiredPoints = {
      "auction-rollover": ["post-auction-rollover", "post-continuation-tick"],
      "cross-tick-partial-fill": ["post-partial-fill", "post-continuation-tick"],
      "save-restore-live-order": ["pre-save", "post-restore", "post-continuation-tick"],
    }[control.surface];
    requireJsonEqual(control.comparison_points, requiredPoints, `${label}.${control.surface} comparison checkpoints`);
  }
  return canonical(control);
}

function validateCorpusControlPair(legacy, current, corpusClass) {
  if (corpusClass === "equivalence") requireJsonEqual(current, legacy, "equivalence corpus mechanical controls");
  if (new Set(["divergence-9", "controlled-live-sell"]).has(corpusClass)) {
    for (const field of ["surface", "seller_order_count", "old_sell_reservation_cents", "feedback", "zero_cash_acceptance", "sealed_exogenous_script_sha256", "rng_cursor", "strategy_state_sha256", "plan_state_sha256", "pending_intents_sha256", "restore_order_sha256", "comparison_points", "surface_evidence"]) {
      requireJsonEqual(current[field], legacy[field], `${corpusClass} control ${field}`);
    }
    if (current.fee_prefixes.length !== legacy.fee_prefixes.length) fail(`${corpusClass} fee prefix count mismatch`);
    for (let index = 0; index < legacy.fee_prefixes.length; index += 1) {
      requireJsonEqual(current.fee_prefixes[index].nominal_cents, legacy.fee_prefixes[index].nominal_cents, `${corpusClass} nominal fee prefix ${index}`);
    }
  }
  if (corpusClass === "divergence-9") {
    if (legacy.surface !== current.surface) fail("divergence-9 control surface mismatch");
    if (legacy.surface === "acceptance-flip") {
      if (BigInt(legacy.old_sell_reservation_cents) === 0n || legacy.acceptance !== "rejected" || current.acceptance !== "accepted") fail("divergence-9 acceptance-flip must prove positive old reservation and old reject/new accept");
    } else {
      if (legacy.fee_prefixes.length < 3) fail("divergence-9 fee catchup must contain the same three-or-more prefixes");
      if (legacy.fee_prefixes.some((prefix) => prefix.charged_cents !== prefix.nominal_cents)) fail("divergence-9 legacy fee prefixes must charge nominal in full");
      if (!current.fee_prefixes.some((prefix) => prefix.charged_cents !== prefix.nominal_cents)) fail("divergence-9 current fee prefixes must expose a shortfall/catchup");
    }
  }
  if (corpusClass === "controlled-live-sell") {
    requireJsonEqual(current.acceptance, legacy.acceptance, "controlled-live-sell acceptance");
  }
}

function rejectionReasonIsInsufficientCash(reason) {
  return reason === "InsufficientCash" || (isRecord(reason) && Object.hasOwn(reason, "InsufficientCash"));
}

function rejectionReasonIsPriceCageExceeded(reason) {
  return reason === "PriceCageExceeded" || (isRecord(reason) && Object.hasOwn(reason, "PriceCageExceeded"));
}

function tradeFactMatchesSubject(fact, subject) {
  if (fact.variant !== "Trade" || fact.payload.code !== subject.stock_code) return false;
  const role = subject.trade_role.startsWith("maker-") ? "maker" : "taker";
  return String(fact.payload[role]) === subject.account_id;
}

function eventOrderId(payload) {
  const id = payload.id ?? payload.order_id;
  if (typeof id === "string" && /^(0|[1-9][0-9]*)$/.test(id)) return id;
  return null;
}

function eventInteger(value, label) {
  return decimal(value, label);
}

function validateAcceptanceFlipEvidence(legacyUpdates, currentUpdates, subject) {
  const legacyFacts = legacyUpdates.flatMap((update) => update.facts);
  const currentFacts = currentUpdates.flatMap((update) => update.facts);
  const rejections = legacyFacts.filter((fact) => fact.variant === "IntentRejected" && rejectionReasonIsInsufficientCash(fact.payload.reason));
  if (rejections.length !== 1) fail("acceptance-flip legacy evidence must contain exactly one InsufficientCash IntentRejected fact");
  const rejection = rejections[0];
  const account = String(rejection.payload.account);
  const code = rejection.payload.code;
  if (account !== subject.account_id || code !== subject.stock_code || subject.side !== "Sell") {
    fail("acceptance-flip legacy evidence must match its explicit Sell subject");
  }
  if (currentFacts.some((fact) => fact.variant === "IntentRejected" && String(fact.payload.account) === account
    && fact.payload.code === code && rejectionReasonIsInsufficientCash(fact.payload.reason))) {
    fail("acceptance-flip current evidence still contains the old InsufficientCash rejection");
  }
  const accepted = currentFacts.some((fact) => fact.variant === "OrderAccepted" && String(fact.payload.account) === account
    && fact.payload.code === code && fact.payload.side === "Sell");
  const traded = currentFacts.some((fact) => tradeFactMatchesSubject(fact, subject));
  if (!accepted && !traded) fail("acceptance-flip current evidence must contain a related Sell OrderAccepted or explicitly-role-bound Sell Trade fact");
  if (legacyUpdates.length !== currentUpdates.length) fail("acceptance-flip update count changed outside its isolated event facts");
  for (let index = 0; index < legacyUpdates.length; index += 1) {
    const legacyRemainder = legacyUpdates[index].facts.filter((fact) => !(fact.variant === "IntentRejected"
      && String(fact.payload.account) === account && fact.payload.code === code && rejectionReasonIsInsufficientCash(fact.payload.reason)));
    const currentRemainder = currentUpdates[index].facts.filter((fact) => !((fact.variant === "OrderAccepted"
      && String(fact.payload.account) === account && fact.payload.code === code && fact.payload.side === "Sell")
      || tradeFactMatchesSubject(fact, subject)));
    requireJsonEqual(currentRemainder, legacyRemainder, `acceptance-flip unrelated event facts in update ${index}`);
  }
}

function validateProjectionSurfaceEvidence(projection, label) {
  if (projection.class !== "equivalence" || projection.corpus_control.surface === "normal-multi-leg-terminal") return;
  const surface = projection.corpus_control.surface;
  const evidence = projection.corpus_control.surface_evidence;
  const facts = projection.updates.flatMap((update) => update.facts);
  const stateField = {
    "buyer-fees": "buyer_fee_control",
    t1: "t1_control",
    "price-cage": "price_cage_control",
    "continuous-buy-leg": "continuous_buy_leg_control",
  }[surface];
  if (!Object.hasOwn(projection.state, stateField)) fail(`${label}.${surface} state is missing ${stateField}`);
  requireJsonEqual(projection.state[stateField], evidence, `${label}.${surface} state/surface evidence`);

  if (surface === "buyer-fees") {
    const trades = facts.filter((fact) => tradeFactMatchesSubject(fact, evidence));
    if (trades.length === 0) fail(`${label}.buyer-fees surface evidence has no related Buy Trade`);
    const gross = trades.reduce((total, fact, index) => total
      + eventInteger(fact.payload.price, `${label}.buyer-fees Trade ${index} price`)
        * eventInteger(fact.payload.qty, `${label}.buyer-fees Trade ${index} qty`), 0n);
    if (gross !== BigInt(evidence.gross_cents)) fail(`${label}.buyer-fees Trade gross does not match surface evidence`);
  } else if (surface === "t1") {
    const trades = facts.filter((fact) => tradeFactMatchesSubject(fact, evidence));
    if (trades.length === 0) fail(`${label}.t1 surface evidence has no related Buy Trade`);
    const bought = trades.reduce((total, fact, index) => total + eventInteger(fact.payload.qty, `${label}.t1 Trade ${index} qty`), 0n);
    if (bought !== BigInt(evidence.bought_qty)) fail(`${label}.t1 related Buy Trade quantity does not match surface evidence`);
  } else if (surface === "price-cage") {
    const rejected = facts.filter((fact) => fact.variant === "IntentRejected"
      && String(fact.payload.account) === evidence.account_id && fact.payload.code === evidence.stock_code
      && rejectionReasonIsPriceCageExceeded(fact.payload.reason));
    const accepted = facts.filter((fact) => fact.variant === "OrderAccepted"
      && String(fact.payload.account) === evidence.account_id && fact.payload.code === evidence.stock_code
      && fact.payload.side === "Buy" && eventOrderId(fact.payload) === evidence.inside_order_id);
    if (rejected.length !== 1 || accepted.length !== 1) fail(`${label}.price-cage surface evidence must bind one PriceCageExceeded rejection and one inside Buy OrderAccepted`);
  } else if (surface === "continuous-buy-leg") {
    const accepted = facts.filter((fact) => fact.variant === "OrderAccepted"
      && String(fact.payload.account) === evidence.account_id && fact.payload.code === evidence.stock_code
      && fact.payload.side === "Buy" && eventOrderId(fact.payload) === evidence.order_id);
    const stableIds = new Set(accepted.map((fact) => eventOrderId(fact.payload)));
    const trades = facts.filter((fact) => tradeFactMatchesSubject(fact, evidence));
    if (accepted.length === 0 || BigInt(stableIds.size) !== BigInt(evidence.stable_order_identity_count)
      || BigInt(trades.length) !== BigInt(evidence.trade_leg_count)) {
      fail(`${label}.continuous-buy-leg surface evidence must bind its Buy OrderAccepted identity and related Trade legs`);
    }
  }
}

function normalizeProjection(projection, label) {
  exactKeys(projection, ["schema", "case_id", "scenario", "seed", "class", "updates", "state", "seller_fee_control", "corpus_control"], label);
  if (projection.schema !== CORPUS_SCHEMA) fail(`${label}.schema is unsupported`);
  if (typeof projection.case_id !== "string" || projection.case_id.length === 0) fail(`${label}.case_id is missing`);
  if (typeof projection.scenario !== "string" || projection.scenario.length === 0) fail(`${label}.scenario is missing`);
  decimal(projection.seed, `${label}.seed`);
  if (!CORPUS_CLASSES.has(projection.class)) fail(`${label}.class is invalid`);
  requireRecord(projection.state, `${label}.state`);
  if (projection.seller_fee_control !== null) requireRecord(projection.seller_fee_control, `${label}.seller_fee_control`);
  validateProjectedIntegers(projection.state, `${label}.state`);
  validateProjectedIntegers(projection.seller_fee_control, `${label}.seller_fee_control`);
  if (projection.class === "controlled-live-sell" && projection.seller_fee_control === null) fail(`${label} controlled-live-sell requires seller_fee_control`);
  if (projection.class !== "controlled-live-sell" && projection.seller_fee_control !== null) fail(`${label} ${projection.class} seller_fee_control must be null`);
  const normalized = {
    case_id: projection.case_id,
    scenario: projection.scenario,
    seed: projection.seed,
    class: projection.class,
    updates: normalizeUpdates(projection.updates, `${label}.updates`),
    state: canonical(projection.state),
    seller_fee_control: canonical(projection.seller_fee_control),
    corpus_control: validateCorpusControl(projection.corpus_control, projection.class, `${label}.corpus_control`),
  };
  validateProjectionSurfaceEvidence(normalized, label);
  return normalized;
}

function verifySellerFeeControl(legacy, current) {
  const fields = ["gross_cents", "nominal_final_cents", "charged_final_cents", "terminal_cash_cents", "invested_cents", "recovered_cents", "fills"];
  exactKeys(legacy, fields, "legacy seller fee control");
  exactKeys(current, fields, "current seller fee control");
  for (const field of ["gross_cents", "nominal_final_cents", "charged_final_cents", "terminal_cash_cents", "invested_cents", "recovered_cents"]) {
    decimal(legacy[field], `legacy seller fee control.${field}`);
    decimal(current[field], `current seller fee control.${field}`);
  }
  requireJsonEqual(current.fills, legacy.fills, "controlled seller fills/FIFO/order identity");
  for (const field of ["gross_cents", "nominal_final_cents", "invested_cents", "recovered_cents"]) requireJsonEqual(current[field], legacy[field], `controlled seller ${field}`);
  const nominal = BigInt(current.nominal_final_cents);
  const charged = BigInt(current.charged_final_cents);
  if (BigInt(legacy.charged_final_cents) !== BigInt(legacy.nominal_final_cents)) fail("controlled seller legacy charged_final must equal nominal_final");
  if (charged > nominal) fail("controlled seller charged_final exceeds nominal_final");
  const balanceDifference = BigInt(current.terminal_cash_cents) - BigInt(legacy.terminal_cash_cents);
  const expectedDifference = nominal - charged;
  if (balanceDifference !== expectedDifference) fail("controlled seller terminal cash difference does not equal the uncollected final fee");
}

function validateMappedDifference(difference, transformation) {
  const numericEffects = new Set(["sell_reservation", "fee_charged", "net_delivery", "fee_event_payload", "terminal_cash_equation"]);
  if (numericEffects.has(transformation.effect)) {
    decimal(difference.legacy, `mapped ${transformation.effect} legacy value at ${difference.path}`);
    decimal(difference.current, `mapped ${transformation.effect} current value at ${difference.path}`);
  }
  if (transformation.effect === "sell_reservation" && difference.current !== "0") fail(`mapped sell_reservation current value at ${difference.path} must be zero`);
  if (transformation.effect === "acceptance_flip") {
    if (difference.path.startsWith("/updates/") && difference.path.includes("/facts/")) return;
    const acceptedPairs = [["rejected", "accepted"], [false, true], ["Rejected", "Accepted"]];
    if (!acceptedPairs.some(([legacy, current]) => Object.is(difference.legacy, legacy) && Object.is(difference.current, current))) {
      fail(`mapped acceptance_flip at ${difference.path} must be old reject/new accept`);
    }
  }
}

export function compareCorpusCase(legacy, current, transformations = []) {
  const oldProjection = normalizeProjection(legacy, "legacy corpus projection");
  const newProjection = normalizeProjection(current, "current corpus projection");
  if (oldProjection.class === "stress" || newProjection.class === "stress") fail("stress corpus is new-engine-only and cannot be used for old/new equivalence");
  if (oldProjection.case_id !== newProjection.case_id || oldProjection.scenario !== newProjection.scenario
    || oldProjection.seed !== newProjection.seed || oldProjection.class !== newProjection.class) fail("corpus case identity or class mismatch");
  const allowed = validateTransformations(transformations, oldProjection.class);
  validateCorpusControlPair(oldProjection.corpus_control, newProjection.corpus_control, oldProjection.class);
  if (oldProjection.class === "divergence-9" && oldProjection.corpus_control.surface === "acceptance-flip") {
    validateAcceptanceFlipEvidence(oldProjection.updates, newProjection.updates, oldProjection.corpus_control.surface_evidence);
  }
  if (oldProjection.class === "controlled-live-sell") {
    if (oldProjection.seller_fee_control === null || newProjection.seller_fee_control === null) fail("controlled-live-sell corpus requires the seller fee control block");
    verifySellerFeeControl(oldProjection.seller_fee_control, newProjection.seller_fee_control);
    const oldFinalPrefix = oldProjection.corpus_control.fee_prefixes.at(-1);
    const newFinalPrefix = newProjection.corpus_control.fee_prefixes.at(-1);
    requireJsonEqual(oldFinalPrefix.nominal_cents, oldProjection.seller_fee_control.nominal_final_cents, "controlled seller legacy final nominal prefix");
    requireJsonEqual(oldFinalPrefix.charged_cents, oldProjection.seller_fee_control.charged_final_cents, "controlled seller legacy final charged prefix");
    requireJsonEqual(newFinalPrefix.nominal_cents, newProjection.seller_fee_control.nominal_final_cents, "controlled seller current final nominal prefix");
    requireJsonEqual(newFinalPrefix.charged_cents, newProjection.seller_fee_control.charged_final_cents, "controlled seller current final charged prefix");
  }
  const { corpus_control: _oldControl, ...oldBusinessProjection } = oldProjection;
  const { corpus_control: _newControl, ...newBusinessProjection } = newProjection;
  const differences = diffJson(oldBusinessProjection, newBusinessProjection);
  const mapped = differences.map((difference) => {
    const matches = allowed.filter((transformation) => pathMatches(transformation.path, difference.path));
    if (matches.length !== 1) fail(`unmapped or ambiguously mapped corpus difference at ${difference.path}`);
    validateMappedDifference(difference, matches[0]);
    return { ...difference, divergence: matches[0].divergence, effect: matches[0].effect };
  });
  for (const transformation of allowed) {
    if (!mapped.some((difference) => difference.divergence === transformation.divergence && difference.effect === transformation.effect && pathMatches(transformation.path, difference.path))) {
      fail(`declared corpus transformation did not match an observed difference: ${transformation.path}`);
    }
  }
  const mappedEffects = new Set(mapped.map((difference) => difference.effect));
  if (oldProjection.class === "divergence-9" && oldProjection.corpus_control.surface === "acceptance-flip"
    && (!mappedEffects.has("sell_reservation") || !mappedEffects.has("acceptance_flip"))) {
    fail("divergence-9 acceptance-flip must map both sell_reservation and acceptance_flip effects");
  }
  if (oldProjection.class === "divergence-9" && oldProjection.corpus_control.surface === "three-leg-fee-catchup"
    && (!mappedEffects.has("fee_charged") || !mappedEffects.has("net_delivery"))) {
    fail("divergence-9 three-leg-fee-catchup must map both fee_charged and net_delivery effects");
  }
  if (oldProjection.class === "controlled-live-sell" && oldProjection.corpus_control.surface === "save-restore-live-order"
    && !mappedEffects.has("save_representation")) fail("controlled save-restore surface must map its #7 save representation difference");
  return { case_id: oldProjection.case_id, scenario: oldProjection.scenario, seed: oldProjection.seed, ticks: oldProjection.updates.map((update) => update.tick), class: oldProjection.class, surface: oldProjection.corpus_control.surface, differences: mapped };
}

export function verifyStressCorpus(current, determinismObservations, conservationSnapshots) {
  const projection = normalizeProjection(current, "stress corpus projection");
  if (projection.class !== "stress") fail("stress corpus projection must declare class=stress");
  const determinism = verifyDeterminismMatrix(determinismObservations);
  const expectedIdentity = `${projection.scenario}\u0000${projection.seed}`;
  for (const observation of determinismObservations) if (observationIdentity(observation) !== expectedIdentity) fail("stress determinism identity does not match its projection");
  const coverage = determinismObservations[0].execution_coverage;
  const tickFrom = BigInt(coverage.tick_from);
  const tickTo = BigInt(coverage.tick_to);
  for (const update of projection.updates) if (BigInt(update.tick) < tickFrom || BigInt(update.tick) > tickTo) fail("stress projection tick is outside determinism coverage");
  if (!Array.isArray(conservationSnapshots) || conservationSnapshots.length === 0) fail("stress corpus requires conservation snapshots");
  for (const snapshot of conservationSnapshots) {
    if (`${snapshot.scenario}\u0000${snapshot.seed}` !== expectedIdentity) fail("stress conservation identity does not match its projection");
    if (BigInt(snapshot.tick) < tickFrom || BigInt(snapshot.tick) > tickTo) fail("stress conservation tick is outside determinism coverage");
  }
  const conservation = conservationSnapshots.map(verifyConservationSnapshot);
  requireConservationCoverage(conservationSnapshots, new Map([[expectedIdentity, coverage]]), "stress conservation coverage");
  return { case_id: projection.case_id, scenario: projection.scenario, seed: projection.seed, surface: projection.corpus_control.surface, historical_equivalence_claimed: false, determinism, conservation };
}

function requireConservationCoverage(snapshots, coverageByIdentity, label) {
  const snapshotsByIdentity = new Map();
  for (const snapshot of snapshots) {
    const identity = `${snapshot.scenario}\u0000${snapshot.seed}`;
    const group = snapshotsByIdentity.get(identity) ?? [];
    group.push(snapshot);
    snapshotsByIdentity.set(identity, group);
  }
  for (const identity of snapshotsByIdentity.keys()) {
    if (!coverageByIdentity.has(identity)) fail(`${label} identity ${identity} has no execution coverage`);
  }
  for (const [identity, coverage] of coverageByIdentity) {
    const group = snapshotsByIdentity.get(identity) ?? [];
    const tickFrom = BigInt(coverage.tick_from);
    const tickTo = BigInt(coverage.tick_to);
    const expectedCount = tickTo - tickFrom + 1n;
    if (BigInt(group.length) !== expectedCount) fail(`${label} must contain exactly one snapshot for every tick ${coverage.tick_from}..${coverage.tick_to} of ${identity}`);
    const ticks = new Set();
    const stockCodes = new Set();
    const observedMultiLegOrderIds = new Set();
    for (const snapshot of group) {
      if (ticks.has(snapshot.tick)) fail(`${label} contains duplicate tick ${snapshot.tick} for ${identity}`);
      ticks.add(snapshot.tick);
      if (BigInt(snapshot.tick) < tickFrom || BigInt(snapshot.tick) > tickTo) fail(`${label} tick ${snapshot.tick} is outside ${coverage.tick_from}..${coverage.tick_to} for ${identity}`);
      for (const envelope of snapshot.envelopes) {
        stockCodes.add(envelope.key.stock_code);
        if (envelope.receipts.filter((receipt) => receipt.journal === "SealedBatch" && receipt.kind === "Fill").length >= 2) {
          observedMultiLegOrderIds.add(envelope.key.order_id);
        }
      }
    }
    for (let tick = tickFrom; tick <= tickTo; tick += 1n) {
      if (!ticks.has(tick.toString())) fail(`${label} is missing tick ${tick} for ${identity}`);
    }
    if (stockCodes.size < 2) fail(`${label} for ${identity} must contain at least two stocks`);
    const claimed = [...coverage.multi_leg_order_ids].sort();
    const observed = [...observedMultiLegOrderIds].sort();
    if (!jsonEqual(claimed, observed)) fail(`${label} multi-leg order ids for ${identity} do not match observed Fill envelopes`);
  }
}

export function verifyEvidenceBundle(bundle) {
  exactKeys(bundle, ["schema", "determinism", "perturbation", "conservation", "corpus", "stress"], "verification bundle");
  if (bundle.schema !== BUNDLE_SCHEMA) fail("verification bundle schema is unsupported");
  const determinism = verifyDeterminismMatrix(bundle.determinism);
  exactKeys(bundle.perturbation, ["reference", "observations", "negative_controls"], "verification bundle perturbation");
  const perturbation = verifyPerturbationGate(bundle.perturbation.reference, bundle.perturbation.observations, bundle.perturbation.negative_controls);
  const determinismIdentities = new Set(bundle.determinism.map(observationIdentity));
  const coverageByIdentity = new Map(bundle.determinism.map((observation) => [observationIdentity(observation), observation.execution_coverage]));
  if (!determinismIdentities.has(observationIdentity(bundle.perturbation.reference))) fail("verification bundle perturbation identity is absent from determinism coverage");
  if (!Array.isArray(bundle.conservation) || bundle.conservation.length === 0) fail("verification bundle conservation list is empty");
  const conservation = bundle.conservation.map(verifyConservationSnapshot);
  const conservationIdentities = new Set(bundle.conservation.map((snapshot) => `${snapshot.scenario}\u0000${snapshot.seed}`));
  for (const identity of determinismIdentities) if (!conservationIdentities.has(identity)) fail(`verification bundle conservation is missing determinism identity ${identity}`);
  for (const identity of conservationIdentities) if (!determinismIdentities.has(identity)) fail(`verification bundle conservation identity ${identity} has no determinism matrix`);
  for (const snapshot of bundle.conservation) {
    const coverage = coverageByIdentity.get(`${snapshot.scenario}\u0000${snapshot.seed}`);
    if (BigInt(snapshot.tick) < BigInt(coverage.tick_from) || BigInt(snapshot.tick) > BigInt(coverage.tick_to)) fail(`verification bundle conservation tick ${snapshot.tick} is outside determinism coverage`);
  }
  requireConservationCoverage(bundle.conservation, coverageByIdentity, "verification bundle conservation coverage");
  if (!Array.isArray(bundle.corpus) || bundle.corpus.length === 0) fail("verification bundle corpus coverage is empty");
  const corpus = bundle.corpus.map((entry, index) => {
    exactKeys(entry, ["legacy", "current", "transformations"], `verification bundle corpus ${index}`);
    return compareCorpusCase(entry.legacy, entry.current, entry.transformations);
  });
  for (const entry of corpus) {
    const coverage = coverageByIdentity.get(`${entry.scenario}\u0000${entry.seed}`);
    if (!coverage) fail(`verification bundle corpus identity ${entry.scenario}/${entry.seed} has no determinism matrix`);
    if (entry.ticks.some((tick) => tick < coverage.tick_from || tick > coverage.tick_to)) fail(`verification bundle corpus ${entry.case_id} tick is outside determinism coverage`);
  }
  const requiredCorpusSurfaces = new Set([
    "equivalence:normal-multi-leg-terminal",
    "equivalence:buyer-fees",
    "equivalence:t1",
    "equivalence:price-cage",
    "equivalence:continuous-buy-leg",
    "divergence-9:acceptance-flip",
    "divergence-9:three-leg-fee-catchup",
    "controlled-live-sell:auction-rollover",
    "controlled-live-sell:cross-tick-partial-fill",
    "controlled-live-sell:save-restore-live-order",
  ]);
  for (const entry of corpus) requiredCorpusSurfaces.delete(`${entry.class}:${entry.surface}`);
  if (requiredCorpusSurfaces.size > 0) fail(`verification bundle corpus coverage is missing ${[...requiredCorpusSurfaces].join(", ")}`);
  if (!Array.isArray(bundle.stress) || bundle.stress.length === 0) fail("verification bundle stress coverage is empty");
  const stress = bundle.stress.map((entry, index) => {
    exactKeys(entry, ["current", "determinism", "conservation"], `verification bundle stress ${index}`);
    return verifyStressCorpus(entry.current, entry.determinism, entry.conservation);
  });
  return { status: "PASS", determinism, perturbation, conservation, corpus, stress };
}

export async function main(argv) {
  if (argv.length !== 1 || argv[0].startsWith("-")) fail("usage: node scripts/simulation/escrow-verification-contracts.mjs <bundle.json>");
  const filePath = path.resolve(argv[0]);
  const stat = await fsp.lstat(filePath);
  if (!stat.isFile() || stat.isSymbolicLink()) fail("verification bundle must be a regular file");
  const bundle = JSON.parse(await fsp.readFile(filePath, "utf8"));
  console.log(JSON.stringify(verifyEvidenceBundle(bundle)));
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch((error) => {
    console.error(`escrow verification failed: ${error.message}`);
    process.exitCode = 1;
  });
}
