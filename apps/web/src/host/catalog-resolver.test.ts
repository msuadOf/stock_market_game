import assert from "node:assert/strict";
import test from "node:test";
import { prepareCatalogResolver } from "./catalog-resolver.ts";

const stocks = [{ id: 0, code: "600101" }];
const companies = [{ id: 7, code: "C-600101", name: "实际公司", listed_stock: 0 }];

test("实际两目录保留原 id，成员 scope 必须属于报告公司", () => {
  const resolver = prepareCatalogResolver(stocks, [...companies, { id: 9, code: "非上市", name: "非上市公司", listed_stock: null }]);
  assert.equal(resolver.stockCode(0), "600101");
  assert.equal(resolver.companyId("C-600101"), 7);
  assert.equal(resolver.companyCode(7), "C-600101");
  assert.equal(resolver.reportScopeCode(7, { Standalone: { entity_id: 7 } }), "C-600101");
  assert.equal(resolver.reportScopeCode(7, { Consolidated: { root_entity_id: 7 } }), "C-600101");
  assert.throws(() => resolver.reportScopeCode(7, { Standalone: { entity_id: 9 } }), /scope/);
  assert.throws(() => resolver.reportScopeCode(7, { Standalone: { entity_id: 100 } }), /scope/);
  assert.throws(() => resolver.stockCode(1), /StockId/);
});

test("目录严格拒绝缺键、多键、重复、非法 u32 与 issuer 覆盖缺口", () => {
  for (const invalid of [undefined, [], [{ ...companies[0], extra: true }], [{ id: 7, code: "C", name: "n" }],
    [companies[0], companies[0]], [companies[0], { ...companies[0], id: 8 }],
    [{ ...companies[0], listed_stock: 1 }], [{ ...companies[0], id: -1 }], [{ ...companies[0], id: 0.5 }],
    [{ ...companies[0], id: 4294967296 }], [{ ...companies[0], code: "  " }]]) {
    assert.throws(() => prepareCatalogResolver(stocks, invalid));
  }
  assert.throws(() => prepareCatalogResolver([...stocks, stocks[0]], companies));
  assert.throws(() => prepareCatalogResolver([{ id: 0, code: "600101", extra: true }], companies));
});

test("合法特殊公司 code 精确保留且不依赖原型", () => {
  for (const code of ["__proto__", "constructor", "公司Ω", "含\u0000代码"]) {
    const resolver = prepareCatalogResolver(stocks, [{ ...companies[0], code }]);
    assert.equal(resolver.companyId(code), 7);
    assert.equal(resolver.companyCode(7), code);
  }
});
