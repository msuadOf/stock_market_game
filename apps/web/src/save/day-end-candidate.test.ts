import assert from "node:assert/strict";
import test from "node:test";
import { currentSaveFixture } from "./current-save-fixture.ts";
import { validateDayEndCandidate } from "./day-end-candidate.ts";

function candidate() {
  const fixture = currentSaveFixture();
  const snapshot = fixture.snapshot as Record<string, unknown>;
  snapshot.seq = 1;
  const clock = fixture.civil_clock as Record<string, unknown>;
  clock.current_date = "2030-01-02";
  clock.settled_through = "2030-01-01";
  return fixture;
}

test("day-end candidate matches both completed civil date and committed sequence", () => {
  const reference = { seq: 1, settledDate: "2030-01-01" };
  assert.deepEqual(validateDayEndCandidate(candidate(), reference), candidate());
  assert.throws(() => validateDayEndCandidate(candidate(), { ...reference, seq: 2 }), /日终候选不属于/);
  assert.throws(() => validateDayEndCandidate(candidate(), { ...reference, settledDate: "2030-01-02" }), /日终候选不属于/);
  assert.throws(() => validateDayEndCandidate(currentSaveFixture(), reference), /日终候选不属于/);
});
