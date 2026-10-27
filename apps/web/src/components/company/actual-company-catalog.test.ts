import assert from "node:assert/strict";
import test from "node:test";
import { actualPublicCompanies } from "./company-catalog.ts";

test("公司选择仅允许 actual 目录存在，名称权威，自定义公司不编造行业", () => {
  const actual = actualPublicCompanies([{ id: 8, code: "C-600101", name: "当前实际名称", listed_stock: 0 }, { id: 1, code: "自定义\u0000公司", name: "独立实体", listed_stock: null }], [{ id: 0, code: "600101" }]);
  assert.deepEqual(actual.map(company => company.id), ["C-600101", "自定义\u0000公司"]);
  assert.equal(actual[0].name, "当前实际名称");
  assert.equal(actual[0].stockCode, "600101");
  assert.equal(actual[1].industry, "");
  assert.equal(actual.some(company => company.id === "C-TEST-BANK"), false);
});
