import assert from "node:assert/strict";
import test from "node:test";
import { normalizePersonalTradeHistoryPage, normalizePersonalTradeHistoryRequest, wirePersonalTradeHistoryRequest } from "./personal-trade-history.ts";

// wire 的 stock 为数值 StockId；002156 ↔ id 1（STOCK_LIST 代码升序，ADR 0041）。
const request = { date_from: "2030-01-02", date_to: "2030-01-04", code: null, side: null, before_receipt: null, as_of_receipt: null, page_size: 2 };
const wireRow = { receipt_id: "7", civil_date: "2030-01-03", stock: 1, side: "Buy", price_cents: "1000", quantity_shares: 100, gross_cents: "100000", actual_fees: { commission: "500", stamp_tax: "0", transfer_fee: "1" } };
const viewRow = { receipt_id: "7", civil_date: "2030-01-03", code: "002156", side: "Buy", price: "1000", quantity_shares: 100, gross: "100000", actual_fees: { commission: "500", stamp_tax: "0", transfer_fee: "1" } };
const page = { request, confirmations: [viewRow], next_cursor: null, as_of_receipt: "10", start_date: "2030-01-02", current_date: "2030-01-05", settled_through: "2030-01-04" };
// wire 回显=数值词表（stock 键）；视图 code 键只出现在归一化结果里。
const wireRequestEcho = { date_from: request.date_from, date_to: request.date_to, stock: null, side: null, before_receipt: null, as_of_receipt: null, page_size: 2 };
const wirePage = { request: wireRequestEcho, confirmations: [wireRow], next_cursor: null, as_of_receipt: "10", start_date: "2030-01-02", current_date: "2030-01-05", settled_through: "2030-01-04" };

test("本人日期查询严格required-nullable、日期及完整u64游标，不接受caller选账户", { timeout: 10000 }, () => {
  assert.deepEqual(normalizePersonalTradeHistoryRequest(request), request);
  for (const invalid of [{ ...request, account: 1 }, { ...request, date_from: "2030-02-30" }, { ...request, date_to: "2030-01-01" }, { ...request, page_size: 0 }, { ...request, before_receipt: "01" }, { ...request, as_of_receipt: "18446744073709551616" }]) assert.throws(() => normalizePersonalTradeHistoryRequest(invalid));
  const { code: removed, ...missing } = request;
  assert.equal(removed, null);
  assert.throws(() => normalizePersonalTradeHistoryRequest(missing), /code/);
});

test("请求侧翻译在 code 筛选上做 StockId 数值化（wire 词表）", { timeout: 10000 }, () => {
  assert.deepEqual(wirePersonalTradeHistoryRequest(request), { date_from: request.date_from, date_to: request.date_to, stock: null, side: null, before_receipt: null, as_of_receipt: null, page_size: 2 });
  const filtered = { ...request, code: "002156" };
  assert.deepEqual(wirePersonalTradeHistoryRequest(filtered).stock, 1);
  assert.throws(() => wirePersonalTradeHistoryRequest({ ...request, code: "600000" }), /未知股票代码/);
});

test("本人日期页保留分股事实、原query与scope元数据，拒绝错filters和receiptwindow", { timeout: 10000 }, () => {
  assert.deepEqual(normalizePersonalTradeHistoryPage(wirePage, request), page);
  for (const invalid of [{ ...wirePage, request: { ...request, side: "Sell" } }, { ...wirePage, confirmations: [{ ...wireRow, receipt_id: "10" }] }, { ...wirePage, confirmations: [{ ...wireRow, civil_date: "2030-01-01" }] }, { ...wirePage, next_cursor: "7" }, { ...wirePage, confirmations: [wireRow, wireRow] }, { ...wirePage, settled_through: "2030-01-05" }]) assert.throws(() => normalizePersonalTradeHistoryPage(invalid, request));
});

test("跨开局前及未来的合法范围不能带入本局未发生交割事实，当前日真实成交仍有效", { timeout: 10000 }, () => {
  const wide = { ...request, date_from: "1999-01-01", date_to: "2099-12-31" };
  // wire 回显是数值词表（stock 键）——视图词表（code 键）只在归一结果中出现。
  const wideWireEcho = { date_from: wide.date_from, date_to: wide.date_to, stock: null, side: null, before_receipt: null, as_of_receipt: null, page_size: wide.page_size };
  const scoped = { ...wirePage, request: wideWireEcho };
  for (const civil_date of ["2030-01-01", "2030-01-06"]) {
    assert.throws(() => normalizePersonalTradeHistoryPage({ ...scoped, confirmations: [{ ...wireRow, civil_date }] }, wide), /本局.*自然日/);
  }
  const current = { ...scoped, confirmations: [{ ...wireRow, civil_date: page.current_date }] };
  assert.deepEqual(normalizePersonalTradeHistoryPage(current, wide), { ...page, request: wide, confirmations: [{ ...viewRow, civil_date: page.current_date }] });
});
