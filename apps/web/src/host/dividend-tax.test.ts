import assert from "node:assert/strict";
import test from "node:test";
import { parseAccountDividendTaxStatusView, parseDividendTaxOutstandingViews } from "./dividend-tax.ts";

test("股息税状态视图接受当前契约形状并保留模式与身份分类", () => {
  const view = parseAccountDividendTaxStatusView({
    mode: "AShareIndividual",
    identity: "Personal",
    stocks: [{ stock: "600101", status: "IndividualPublicMarket" }, { stock: "000812", status: "TreatmentNotConfigured" }],
  });
  assert.equal(view.mode, "AShareIndividual");
  assert.equal(view.identity, "Personal");
  assert.deepEqual(view.stocks, [
    { stock: "600101", status: "IndividualPublicMarket" },
    { stock: "000812", status: "TreatmentNotConfigured" },
  ]);
  assert.deepEqual(parseAccountDividendTaxStatusView({ mode: "Exempt", identity: "NonIndividualPending", stocks: [] }), {
    mode: "Exempt",
    identity: "NonIndividualPending",
    stocks: [],
  });
});

test("股息税状态视图拒绝未知模式、未知身份与多余字段", () => {
  assert.throws(() => parseAccountDividendTaxStatusView({ mode: "NoTax", identity: "Personal", stocks: [] }), /dividend_tax_status\.mode/);
  assert.throws(() => parseAccountDividendTaxStatusView({ mode: "Exempt", identity: "Enterprise", stocks: [] }), /dividend_tax_status\.identity/);
  assert.throws(() => parseAccountDividendTaxStatusView({ mode: "Exempt", identity: "Personal", stocks: [], extra: 1 }), /dividend_tax_status\.extra/);
  assert.throws(() => parseAccountDividendTaxStatusView({ mode: "Exempt", identity: "Personal", stocks: [{ stock: "", status: "Exempt" }] }), /stocks\[0\]\.(status|stock)/);
});

test("股息税未清视图沿用严格到账 parser 并拒绝自相矛盾的余额标志", () => {
  const views = parseDividendTaxOutstandingViews([
    { account: "0", stock: "600101", outstanding: { numerator: "0", denominator: "1" }, needs_funds: false, cause: "Cleared" },
    { account: "0", stock: "000812", outstanding: { numerator: "7", denominator: "1" }, needs_funds: true, cause: "InsufficientAvailableCash" },
  ]);
  assert.equal(views.length, 2);
  assert.equal(views[1]?.outstanding.numerator, "7");
  assert.equal(views[1]?.needs_funds, true);
  assert.throws(() => parseDividendTaxOutstandingViews([
    { account: "0", stock: "600101", outstanding: { numerator: "0", denominator: "1" }, needs_funds: true, cause: "InsufficientAvailableCash" },
  ]), /needs_funds|cause|一致/);
});
