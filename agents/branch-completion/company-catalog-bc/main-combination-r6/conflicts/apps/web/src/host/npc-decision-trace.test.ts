import { defaultCatalogFixture } from "./catalog-test-fixture.ts";
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { parseNpcDecisionDiagnostics, parseNpcDecisionTrace } from "./npc-decision-trace.ts";

const trace = [{
  account: "5",
  tick: "129",
  source_report_ids: ["17"],
  expectation_method: "CashFlow",
  plan_ids: [{ owner: "5", sequence: "3" }],
  plan_changes: ["PlanId(3): Created -> Active/v1"],
  budget_constraints: [],
  order_ids: [7],
  // wire 的 codes 为数值 StockId 数组（摄入层译回代码串）。
  codes: [3],
}];

describe("NPC decision trace boundary", { timeout: 10_000 }, () => {
  it("preserves canonical decimal ticks through the u64 upper bound", () => {
    for (const tick of ["0", "9007199254740993", "18446744073709551615"]) {
      assert.equal(parseNpcDecisionTrace([{ ...trace[0], tick }], defaultCatalogFixture)[0]?.tick, tick);
    }
    for (const tick of [129, "", "01", "-1", "1.0", "18446744073709551616", "1e3"]) {
      assert.throws(() => parseNpcDecisionTrace([{ ...trace[0], tick }], defaultCatalogFixture), /字段无效/);
    }
    assert.throws(() => parseNpcDecisionTrace([{ ...trace[0], plan_changes: [1] }], defaultCatalogFixture), /字段无效/);
  });
  it("rejects malformed trace records before they reach a DEV surface", () => {
    assert.throws(
<<<<<<< HEAD
      () => parseNpcDecisionTrace([{ ...trace[0], account: "5" }], defaultCatalogFixture),
=======
      () => parseNpcDecisionTrace([{ ...trace[0], account: 5 }]),
>>>>>>> 9e7c3da2c41f89930d186e3061c11792fb60780a
      /字段无效/,
    );
  });

  it("rejects unsafe integers and unexpected private fields", () => {
    assert.throws(
      () => parseNpcDecisionTrace([{ ...trace[0], account: Number.MAX_SAFE_INTEGER + 1 }], defaultCatalogFixture),
      /字段无效/,
    );
    assert.throws(
      () => parseNpcDecisionTrace([{ ...trace[0], private_cash: 1 }], defaultCatalogFixture),
      /字段不完整/,
    );
  });

  it("retains the approved causal provenance fields", () => {
    // wire codes 数值 → 摄入译回代码串：往返后与公共形态一致。
    assert.deepEqual(parseNpcDecisionTrace(trace, defaultCatalogFixture), trace.map((row) => ({ ...row, codes: ["600101"] })));
  });

  it("accepts unsupported results only without private payload", () => {
    assert.deepEqual(parseNpcDecisionDiagnostics({ kind: "unsupported" }, defaultCatalogFixture), { kind: "unsupported" });
    assert.throws(
      () => parseNpcDecisionDiagnostics({ kind: "unsupported", records: trace }, defaultCatalogFixture),
      /不得携带私有数据/,
    );
  });
});


describe("NPC 决策 PlanId 边界", { timeout: 10_000 }, () => {
  it("无损保留完整 owner 和 sequence，及同 owner 的多项计划", () => {
    const row = {
      ...trace[0], account: "18446744073709551615",
      plan_ids: [
        { owner: "18446744073709551615", sequence: "9007199254740993" },
        { owner: "18446744073709551615", sequence: "18446744073709551615" },
      ],
    };
    assert.deepEqual(parseNpcDecisionTrace([row])[0]?.plan_ids, row.plan_ids);
  });
  it("拒绝旧标量身份、缺字段、未知字段、非规范及越界 u64", () => {
    for (const id of [3, "3", {}, { owner: "5" }, { sequence: "3" }, { owner: "5", sequence: "3", extra: true }]) {
      assert.throws(() => parseNpcDecisionTrace([{ ...trace[0], plan_ids: [id] }]), /plan_ids\[0\]/);
    }
    for (const value of [5, "", "01", "-0", "+1", "-1", "1.0", "1e3", " 1", "1\n", "18446744073709551616"]) {
      for (const field of ["owner", "sequence"]) {
        assert.throws(() => parseNpcDecisionTrace([{ ...trace[0], plan_ids: [{ owner: "5", sequence: "3", [field]: value }] }]), /plan_ids\[0\]/);
      }
      assert.throws(() => parseNpcDecisionTrace([{ ...trace[0], account: value }]), /字段无效/);
    }
    assert.throws(() => parseNpcDecisionTrace([{ ...trace[0], plan_ids: {} }]), /plan_ids/);
  });
  it("拒绝重复的完整身份和与诊断 account 不符的 owner", () => {
    assert.throws(() => parseNpcDecisionTrace([{ ...trace[0], plan_ids: [...trace[0]!.plan_ids, ...trace[0]!.plan_ids] }]), /重复.*PlanId/);
    assert.throws(() => parseNpcDecisionTrace([{ ...trace[0], plan_ids: [{ owner: "6", sequence: "3" }] }]), /owner.*account/);
  });
});
