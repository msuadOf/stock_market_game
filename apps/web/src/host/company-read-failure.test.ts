import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { CompanyReadRejectedError, parseCompanyReadFailure } from "./company-read-failure.ts";

describe("company read failure codec", () => {
  it("保留每个typed变体的既有结构与错误理由", { timeout: 10_000 }, () => {
    const values = [
      { kind: "Unhealthy" },
      { kind: "UnknownOwner", data: { account: "18446744073709551615" } },
      { kind: "UnknownCompany", data: { company: 4294967295 } },
      { kind: "Catalog", data: { fault: { code: 1300, context: [1, 2] } } },
      { kind: "Company", data: { classification: "SystemState", reason: "公司目录状态无效" } },
      { kind: "CorporateState", data: { reason: "权益状态无效" } },
      { kind: "DividendTaxOutstanding", data: { reason: "税额汇总无效" } },
    ];
    assert.deepEqual(values.map(parseCompanyReadFailure), values);
    const cause = { kind: "Company", data: { classification: "BusinessCondition", reason: "当前条件不满足" } };
    const error = new CompanyReadRejectedError(parseCompanyReadFailure(cause), cause);
    assert.equal(error.message, "当前条件不满足");
    assert.equal(error.cause, cause);
  });

  it("显式拒绝缺字段、未知字段、非规范账户与类型错误", { timeout: 10_000 }, () => {
    for (const value of [
      { kind: "Unhealthy", data: null },
      { kind: "UnknownOwner", data: { account: 1 } },
      { kind: "Catalog", data: { fault: { code: 1, context: undefined } } },
      { kind: "Company", data: { classification: "SystemState", reason: "x", extra: true } },
      { kind: "Unknown" },
    ]) assert.throws(() => parseCompanyReadFailure(value));
  });
});
