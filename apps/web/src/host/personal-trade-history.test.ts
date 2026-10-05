import assert from "node:assert/strict";
import test from "node:test";
import { normalizePersonalTradeHistoryRequest, normalizePersonalTradeHistoryPage } from "./personal-trade-history.ts";

const request = { date_from: "2030-01-02", date_to: "2030-01-04", code: null, side: null, before_receipt: null, as_of_receipt: null, page_size: 2 };
const row = { receipt_id: "7", civil_date: "2030-01-03", code: "600001", side: "Buy", price: "1000", quantity_shares: 100, gross: "100000", actual_fees: { commission: "500", stamp_tax: "0", transfer_fee: "1" } };
const page = { request, confirmations: [row], next_cursor: null, as_of_receipt: "10", start_date: "2030-01-02", current_date: "2030-01-05", settled_through: "2030-01-04" };

test("本人日期查询严格required-nullable、日期及完整u64游标，不接受caller选账户", { timeout: 10000 }, () => {
  assert.deepEqual(normalizePersonalTradeHistoryRequest(request), request);
  for (const invalid of [{ ...request, account: 1 }, { ...request, date_from: "2030-02-30" }, { ...request, date_to: "2030-01-01" }, { ...request, page_size: 0 }, { ...request, before_receipt: "01" }, { ...request, as_of_receipt: "18446744073709551616" }]) assert.throws(() => normalizePersonalTradeHistoryRequest(invalid));
  const { code: removed, ...missing } = request;
  assert.equal(removed, null);
  assert.throws(() => normalizePersonalTradeHistoryRequest(missing), /code/);
});

test("本人日期页保留分股事实、原query与scope元数据，拒绝错filters和receiptwindow", { timeout: 10000 }, () => {
  assert.deepEqual(normalizePersonalTradeHistoryPage(page, request), page);
  for (const invalid of [{ ...page, request: { ...request, side: "Sell" } }, { ...page, confirmations: [{ ...row, receipt_id: "10" }] }, { ...page, confirmations: [{ ...row, civil_date: "2030-01-01" }] }, { ...page, next_cursor: "7" }, { ...page, confirmations: [row, row] }, { ...page, settled_through: "2030-01-05" }]) assert.throws(() => normalizePersonalTradeHistoryPage(invalid, request));
});

test("跨开局前及未来的合法范围不能带入本局未发生交割事实，当前日真实成交仍有效", { timeout: 10000 }, () => {
  const wide = { ...request, date_from: "1999-01-01", date_to: "2099-12-31" };
  const scoped = { ...page, request: wide };
  for (const civil_date of ["2030-01-01", "2030-01-06"]) {
    assert.throws(() => normalizePersonalTradeHistoryPage({ ...scoped, confirmations: [{ ...row, civil_date }] }, wide), /本局.*自然日/);
  }
  const current = { ...scoped, confirmations: [{ ...row, civil_date: page.current_date }] };
  assert.deepEqual(normalizePersonalTradeHistoryPage(current, wide), current);
});
