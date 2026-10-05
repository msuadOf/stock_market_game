import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { parseNpcDecisionDiagnostics, parseNpcDecisionTrace } from "./npc-decision-trace.ts";

const trace = [{
  account: "5",
  tick: "129",
  source_report_ids: ["17"],
  expectation_method: "CashFlow",
  plan_ids: [3],
  plan_changes: ["PlanId(3): Created -> Active/v1"],
  budget_constraints: [],
  order_ids: [7],
  codes: ["600101"],
}];

describe("NPC decision trace boundary", { timeout: 10_000 }, () => {
  it("preserves canonical decimal ticks through the u64 upper bound", () => {
    for (const tick of ["0", "9007199254740993", "18446744073709551615"]) {
      assert.equal(parseNpcDecisionTrace([{ ...trace[0], tick }])[0]?.tick, tick);
    }
    for (const tick of [129, "", "01", "-1", "1.0", "18446744073709551616", "1e3"]) {
      assert.throws(() => parseNpcDecisionTrace([{ ...trace[0], tick }]), /字段无效/);
    }
    assert.throws(() => parseNpcDecisionTrace([{ ...trace[0], plan_changes: [1] }]), /字段无效/);
  });
  it("rejects malformed trace records before they reach a DEV surface", () => {
    assert.throws(
      () => parseNpcDecisionTrace([{ ...trace[0], account: 5 }]),
      /字段无效/,
    );
  });

  it("rejects unsafe integers and unexpected private fields", () => {
    assert.throws(
      () => parseNpcDecisionTrace([{ ...trace[0], account: Number.MAX_SAFE_INTEGER + 1 }]),
      /字段无效/,
    );
    assert.throws(
      () => parseNpcDecisionTrace([{ ...trace[0], private_cash: 1 }]),
      /字段不完整/,
    );
  });

  it("retains the approved causal provenance fields", () => {
    assert.deepEqual(parseNpcDecisionTrace(trace), trace);
  });

  it("accepts unsupported results only without private payload", () => {
    assert.deepEqual(parseNpcDecisionDiagnostics({ kind: "unsupported" }), { kind: "unsupported" });
    assert.throws(
      () => parseNpcDecisionDiagnostics({ kind: "unsupported", records: trace }),
      /不得携带私有数据/,
    );
  });
});
