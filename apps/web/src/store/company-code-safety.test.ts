import assert from "node:assert/strict";
import test from "node:test";
import { configureStore } from "@reduxjs/toolkit";
import { companyReducer, installCompanyBaseline, startCompanyQuery, recordCompanyPage, updateCompanyReading, selectCompanyReading, invalidateCompanyForDisclosure } from "./company-slice.ts";
import { publicReportGold } from "../components/company/public-report-fixture.ts";

test("Redux/Immer 对特殊公司 code 的缓存和阅读操作不命中原型", () => {
  const store = configureStore({ reducer: companyReducer });
  store.dispatch(installCompanyBaseline({ generation: 1, civilDate: null, revision: null, seq: 0 }));
  for (const companyId of ["__proto__", "constructor", "公司Ω", "含\u0000代码"]) {
    store.dispatch(startCompanyQuery({ generation: 1, companyId, cursor: null }));
    store.dispatch(recordCompanyPage({ generation: 1, companyId, cursor: null, reports: [publicReportGold()], nextCursor: null }));
    store.dispatch(updateCompanyReading({ generation: 1, companyId, changes: { exactAmountsVisible: true } }));
    assert.equal(Object.hasOwn(store.getState().companies, companyId), true);
    assert.equal(selectCompanyReading(store.getState(), companyId).exactAmountsVisible, true);
    store.dispatch(invalidateCompanyForDisclosure({ generation: 1, companyId, seq: 1 }));
  }
  assert.equal(Object.getPrototypeOf(store.getState().companies), null);
  assert.equal(Object.getPrototypeOf(store.getState().readingByCompany), null);
  store.dispatch(installCompanyBaseline({ generation: 2, civilDate: null, revision: null, seq: 0 }));
  assert.equal(Object.getPrototypeOf(store.getState().companies), null);
  assert.equal(Object.getPrototypeOf(store.getState().readingByCompany), null);
  assert.deepEqual(Object.keys(store.getState().companies), []);
});
