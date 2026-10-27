import assert from "node:assert/strict";
import test from "node:test";
import { configureStore } from "@reduxjs/toolkit";
import { companyReducer } from "../store/company-slice.ts";
import { store as referenceStore } from "../store/store.ts";
import { prepareBaselineUpdate } from "./host-update.ts";
import { ProtocolCoordinator } from "./protocol-coordinator.ts";
import { snapshot } from "./protocol-test-fixtures.ts";
import { CompanyQueryCoordinator } from "./company-query-coordinator.ts";
import { TauriTimelineState } from "./tauri-timeline-state.ts";

const stocks = [{ id: 0, code: "600101" }];
const companies = [{ id: 0, code: "C-600101", name: "当前公司", listed_stock: 0 }];
const host = { capabilities: { deliveryModes: [], targetUiHz: 60, sharedMemory: false, reconnect: false, publicCompanyReports: false, npcDecisionDiagnostics: false } };

test("坏目录 prepare 不发布 Redux/protocol/App，新 baseline 同一次dispatch安装snapshot和company", () => {
  const observed: unknown[] = [];
  const companyStore = configureStore({ reducer: companyReducer });
  const company = new CompanyQueryCoordinator(host, companyStore.dispatch);
  let callbacks = 0;
  const coordinator = new ProtocolCoordinator({
    prepareBaseline(state, baseline, generation) { return company.prepareBaseline({ ...baseline, seq: state.snapshot.seq }, generation); },
    onBaseline() { callbacks++; }, onApplied() {}, onFailure() {},
  }, action => { referenceStore.dispatch(action); observed.push(referenceStore.getState()); });
  coordinator.accept(prepareBaselineUpdate("1", snapshot(0, 0), stocks, companies, { civilDate: null, revision: null }));
  const before = referenceStore.getState();
  assert.equal(before.company.generation, 1);
  assert.equal(before.snapshot.generation, "1");
  assert.equal(before.company.companyCatalog[0].code, "C-600101");
  assert.equal(observed.length, 1);
  for (const invalid of [[], [companies[0], companies[0]], [{ ...companies[0], listed_stock: 99 }]]) {
    assert.throws(() => coordinator.accept(prepareBaselineUpdate("2", snapshot(0, 0), stocks, invalid, { civilDate: null, revision: null })));
    assert.equal(referenceStore.getState(), before);
    assert.equal(callbacks, 1);
  }
  assert.throws(() => coordinator.accept(prepareBaselineUpdate("0", snapshot(0, 0), stocks, companies, { civilDate: null, revision: null })), /旧 generation/);
  assert.equal(referenceStore.getState(), before);
});

test("Tauri 坏目录不覆盖 timeline、generation、query epoch 或 cached baseline", () => {
  const timeline = new TauriTimelineState();
  const initial = { snapshot: snapshot(0, 0), stock_catalog: stocks, company_catalog: companies, timeline_id: "one", generation: "1", civil_date: "2030-01-02" };
  const baseline = timeline.installInitialBaseline(initial);
  const cursor = timeline.captureQueryCursor();
  assert.throws(() => timeline.replaceRestoredBaseline({ ...initial, timeline_id: "two", generation: "2", company_catalog: [] }));
  assert.equal(timeline.currentGeneration(), "1");
  assert.equal(timeline.matchesTimeline("one"), true);
  assert.equal(timeline.baselineForRead(), baseline);
  assert.doesNotThrow(() => timeline.assertQueryCursor(cursor, "1", "stale"));
});
