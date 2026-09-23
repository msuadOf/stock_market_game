import assert from "node:assert/strict";

export function canonical(value) {
  if (Array.isArray(value)) return value.map(canonical);
  if (value !== null && typeof value === "object") return Object.fromEntries(Object.keys(value).sort().map((key) => [key, canonical(value[key])]));
  return value;
}

function facts(frame) {
  const seen = new Set();
  return frame.events.map(({ identity, event }) => {
    const key = JSON.stringify(identity);
    assert(!seen.has(key), `duplicate event identity: ${key}`);
    seen.add(key);
    const variants = Object.entries(event);
    assert.equal(variants.length, 1);
    const [variant, payload] = variants[0];
    assert.equal(identity[0], frame.tick, "event tick identity mismatch");
    assert.equal(identity[1], variant, "event variant identity mismatch");
    const { seq, ...business } = payload;
    assert(Number.isSafeInteger(seq), "seq must be integer");
    return JSON.stringify(canonical({ identity, variant, business }));
  }).sort();
}

export function verifyFrames(frames) {
  assert(frames.length > 0, "empty frame corpus");
  let previous;
  for (const frame of frames) {
    assert(Number.isSafeInteger(frame.tick) && frame.tick > 0, "invalid tick");
    assert(Object.hasOwn(frame, "timeseries_payload"), "missing timeseries");
    assert(Number.isSafeInteger(frame.seq_from) && Number.isSafeInteger(frame.seq_to), "invalid seq range");
    if (previous) {
      assert.equal(frame.tick, previous.tick + 1, "tick gap/reorder");
      assert.equal(frame.seq_from, previous.seq_to + 1, "seq coverage gap");
    }
    assert.equal(frame.seq_to - frame.seq_from + 1, frame.events.length, "seq range cardinality");
    const seqs = frame.events.map(({ event }) => Object.values(event)[0].seq).sort((left, right) => left - right);
    seqs.forEach((seq, index) => assert.equal(seq, frame.seq_from + index, "seq duplicate/gap"));
    facts(frame);
    previous = frame;
  }
}

export function compareFrames(expected, actual) {
  verifyFrames(expected);
  verifyFrames(actual);
  assert.equal(actual.length, expected.length, "missing tick");
  expected.forEach((frame, index) => {
    const other = actual[index];
    assert.deepEqual([other.tick, other.seq_from, other.seq_to], [frame.tick, frame.seq_from, frame.seq_to]);
    assert.deepEqual(canonical(other.timeseries_payload), canonical(frame.timeseries_payload), "timeseries mismatch");
    assert.deepEqual(facts(other), facts(frame), "event identity/payload multiset mismatch");
  });
}

export function feasibility(frames) {
  verifyFrames(frames);
  const minimal = frames.map(({tick,seq_from,seq_to,timeseries_payload,events})=>({tick,seq_from,seq_to,timeseries_payload,events}));
  const reordered = structuredClone(minimal);
  for (const frame of reordered) frame.events.reverse();
  compareFrames(frames, reordered);
  const populated = frames.findIndex((frame) => frame.events.length >= 2);
  assert(populated >= 0, "feasibility requires at least two real events");
  const negatives = {
    missing: (frame) => frame.events.pop(),
    duplicate: (frame) => frame.events.push(structuredClone(frame.events[0])),
    collision: (frame) => { frame.events[1].identity = frame.events[0].identity; },
    payload_swap: (frame) => { [frame.events[0].event, frame.events[1].event] = [frame.events[1].event, frame.events[0].event]; },
    payload_mutation: (frame) => { Object.values(frame.events[0].event)[0].unexpected_business_field = 1; },
    identity_mutation: (frame) => { frame.events[0].identity[2] = "Session:mutated"; },
    timeseries_mutation: (frame) => { frame.timeseries_payload = null; },
  };
  const results = {};
  for (const [name, mutate] of Object.entries(negatives)) {
    const altered = structuredClone(minimal);
    mutate(altered[populated]);
    assert.throws(() => compareFrames(frames, altered), undefined, `negative ${name} was accepted`);
    results[name] = "rejected";
  }
  return { real_event_count: frames.reduce((total, frame) => total + frame.events.length, 0), unique: true, reorder_accepted: true, negatives: results };
}
