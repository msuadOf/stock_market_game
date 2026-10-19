import assert from "node:assert/strict";
import test from "node:test";
import { currentSaveFixture, withSettledDay } from "./current-save-fixture.ts";
import { validateDayEndCandidate } from "./day-end-candidate.ts";

function candidate() {
  const fixture = withSettledDay(currentSaveFixture(), "2030-01-01", "2030-01-02");
  const snapshot = fixture.snapshot as Record<string, unknown>;
  snapshot.seq = 1;
  return fixture;
}

test("day-end candidate matches both completed civil date and committed sequence", () => {
  const reference = { seq: 1, settledDate: "2030-01-01" };
  assert.deepEqual(validateDayEndCandidate(candidate(), reference), candidate());
  assert.throws(() => validateDayEndCandidate(candidate(), { ...reference, seq: 2 }), /日终候选不属于/);
  assert.throws(() => validateDayEndCandidate(candidate(), { ...reference, settledDate: "2030-01-02" }), /日终候选不属于/);
  assert.throws(() => validateDayEndCandidate(currentSaveFixture(), reference), /日终候选不属于/);
});
