import assert from "node:assert/strict";
import { compareCorpusCase, normalizeUpdates, verifyConservationSnapshot } from "./escrow-verification-contracts.mjs";
import { adaptLegacyStream } from "./escrow-corpus-adapter.mjs";

const absent = (value) => value !== null && typeof value === "object"
  && Object.keys(value).length === 1 && value.absent === true;
const businessPayload = (event) => {
  const [variant, { seq, ...payload }] = Object.entries(event)[0];
  return JSON.stringify([variant, payload]);
};

// Diagnostic full-primary-stream comparison is intentionally below the Task 9
// surface gate. A matching funded stream is not zero-cash acceptance evidence.
export function compareCapturedPrimaryStream(run, current) {
  assert.equal(current.schema, "escrow-current-corpus-run-v1");
  assert.equal(current.scenario, run.scenario);
  assert.equal(current.seed, run.seed);
  assert.equal(current.historical_comparison_performed, false);
  const legacy = adaptLegacyStream(run);
  const left = { updates: normalizeUpdates(legacy.updates, "sealed primary"), state: legacy.state };
  const right = { updates: normalizeUpdates(current.updates, "current primary"), state: current.state };
  assert.equal(current.conservation.length, current.updates.length, "current primary lacks per-tick conservation");
  current.conservation.forEach((snapshot, index) => {
    verifyConservationSnapshot(snapshot);
    assert.equal(snapshot.scenario, run.scenario);
    assert.equal(snapshot.seed, run.seed);
    assert.equal(snapshot.tick, current.updates[index].tick);
  });
  const differences = [];
  const escape = (key) => key.replaceAll("~", "~0").replaceAll("/", "~1");
  function visit(a, b, pointer) {
    if (Object.is(a, b)) return;
    if (a !== null && b !== null && typeof a === "object" && typeof b === "object"
      && Array.isArray(a) === Array.isArray(b)) {
      for (const key of new Set([...Object.keys(a), ...Object.keys(b)])) visit(a[key], b[key], `${pointer}/${escape(key)}`);
      return;
    }
    differences.push({ path: pointer, legacy: a === undefined ? { absent: true } : a,
      current: b === undefined ? { absent: true } : b });
  }
  visit(left, right, "");
  return { status: differences.length === 0 ? "CAPTURE_MATCH" : "UNMAPPED_DIFFERENCES",
    task9_acceptance: false, scenario: run.scenario, seed: run.seed,
    updates: current.updates.length, differences, provenance: legacy.provenance,
    remaining_surface_gate: "Reviewed corpus surface evidence and exact #9 mappings are still required; funded primary capture cannot prove zero-cash acceptance." };
}

// Final corpus acceptance uses a reviewed table of exact old/new leaf values.
// The lower-level comparator supplies class, sequence, identity and cash rules;
// an effect label alone is not evidence that an arbitrary fee value is valid.
export function compareExactCorpusCase(legacy, current, mappings = []) {
  assert(Array.isArray(mappings), "exact mappings must be an array");
  const paths = new Set();
  const transforms = new Map();
  for (const row of mappings) {
    assert.deepEqual(Object.keys(row).sort(), ["current", "divergence", "effect", "legacy", "path"], "exact mapping keys");
    assert(typeof row.path === "string" && row.path.startsWith("/") && !row.path.includes("*")
      && !row.path.includes("..") && !paths.has(row.path), "exact mappings require unique literal JSON pointers");
    paths.add(row.path);
    assert(![row.legacy, row.current].some((value) => value !== null && typeof value === "object" && !absent(value)), "exact mapping cannot replace a subtree");
    if (row.divergence === 7) {
      assert(/^\/state\/save_representation\/(schema|schema_version|simulation_policy_id)$/.test(row.path),
        "#7 maps representation metadata only; orders, balances and restore state remain semantic fields");
    }
    assert.notDeepEqual(row.legacy, row.current, "mapping must describe an observed difference");
    let mappingPath = row.path;
    // Acceptance replaces a typed fact, but every changed leaf is checked below.
    if (row.effect === "acceptance_flip" && /^\/updates\/\d+\/facts\//.test(row.path)) {
      mappingPath = row.path.replace(/\/facts\/.*/, "/facts/**");
    }
    const transformation = { divergence: row.divergence, effect: row.effect, path: mappingPath };
    transforms.set(JSON.stringify(transformation), transformation);
  }
  const result = compareCorpusCase(legacy, current, [...transforms.values()]);
  assert.equal(result.differences.length, mappings.length, "unmapped corpus difference or stale exact mapping");
  for (const difference of result.differences) {
    const row = mappings.find((candidate) => candidate.path === difference.path);
    assert(row, `unmapped corpus difference at ${difference.path}`);
    for (const field of ["legacy", "current", "divergence", "effect"]) {
      const expected = ["legacy", "current"].includes(field) && absent(row[field]) ? undefined : row[field];
      assert.deepEqual(difference[field], expected, `unmapped corpus ${field} value at ${row.path}`);
    }
  }
  return result;
}

// All mutations preserve fixed event identities. Re-sealing the deleted stream
// removes the trivial range failure so that the missing semantic fact is tested.
export function corpusUnmappedDiff(legacy, current, mappings = []) {
  compareExactCorpusCase(legacy, current, mappings);
  const populated = current.updates.findIndex((update) => update.events.length >= 2);
  assert(populated >= 0, "corpus_unmapped_diff needs at least two real facts");
  const reordered = structuredClone(current);
  reordered.updates[populated].events.reverse();
  compareExactCorpusCase(legacy, reordered, mappings);
  const mutations = {
    deleted_event(candidate) {
      candidate.updates[populated].events.pop();
      let cursor = BigInt(candidate.updates[0].seq_from);
      for (const update of candidate.updates) {
        update.seq_from = cursor.toString();
        for (const fact of update.events) Object.values(fact.event)[0].seq = (cursor++).toString();
        update.seq_to = (cursor - 1n).toString();
      }
    },
    fixed_identity_payload_swap(candidate) {
      const facts = candidate.updates[populated].events;
      const pair = facts.flatMap((fact, left) => facts.slice(left + 1).map((other, offset) => [left, left + offset + 1]))
        .find(([left, right]) => Object.keys(facts[left].event)[0] === Object.keys(facts[right].event)[0]
          && businessPayload(facts[left].event) !== businessPayload(facts[right].event));
      const [left, right] = pair ?? [0, 1];
      [facts[left].event, facts[right].event] = [facts[right].event, facts[left].event];
    },
    unmapped_payload_mutation(candidate) {
      Object.values(candidate.updates[populated].events[0].event)[0].unmapped_business_field = "tampered";
    },
  };
  const negatives = {};
  for (const [name, mutate] of Object.entries(mutations)) {
    const candidate = structuredClone(current);
    mutate(candidate);
    assert.throws(() => compareExactCorpusCase(legacy, candidate, mappings), undefined, `negative ${name} was accepted`);
    negatives[name] = "rejected";
  }
  return { case_id: current.case_id, reorder_accepted: true, negatives };
}
