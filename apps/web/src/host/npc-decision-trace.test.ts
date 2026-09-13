import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { parseNpcDecisionDiagnostics, parseNpcDecisionTrace } from "./npc-decision-trace.ts";

const trace = [{
  account: 5,
  tick: 129,
  source_report_ids: ["17"],
  expectation_method: "CashFlow",
  plan_ids: [3],
  budget_constraints: [],
  order_ids: [7],
  codes: ["600101"],
}];

describe("NPC decision trace boundary", () => {
  it("rejects malformed trace records before they reach a DEV surface", () => {
    assert.throws(
      () => parseNpcDecisionTrace([{ ...trace[0], account: "5" }]),
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
