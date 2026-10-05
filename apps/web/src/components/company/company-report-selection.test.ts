import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import * as React from "react";
import { createServer, type ViteDevServer } from "vite";
import { memoryHook } from "../../app/hook-test-runtime.ts";
import { companyReducer, installCompanyBaseline, recordCompanyPage, recordCompanyQueryFailure, startCompanyQuery, updateCompanyReading } from "../../store/company-slice.ts";
import { publicReportGold } from "./public-report-fixture.ts";

let vite: ViteDevServer;
let views: typeof import("./CompanyPanel.tsx");

before(async () => {
  vite = await createServer({ configFile: false, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  views = await vite.ssrLoadModule("/src/components/company/CompanyPanel.tsx") as typeof views;
});
after(async () => { if (vite) await vite.close(); });

function fixture() {
  const query = { generation: 1, companyId: "C-600101", cursor: null };
  const reports = [publicReportGold(), { ...publicReportGold(), id: "8" }];
  const baseline = companyReducer(undefined, installCompanyBaseline({ generation: 1, civilDate: "2030-04-02", revision: "1", seq: 0 }));
  const ready = companyReducer(baseline, recordCompanyPage({ ...query, reports, nextCursor: null }));
  return { query, reports, ready };
}

function selectionView(initialState: ReturnType<typeof fixture>["ready"]) {
  type Props = Parameters<typeof views.CompanyPanel>[0];
  type Dispatcher = { useEffect: (effect: () => void, dependencies: readonly unknown[]) => void };
  const internals = (React as unknown as { __SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED: { ReactCurrentDispatcher: { current: Dispatcher } } }).__SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED;
  const effects: (() => void)[] = [];
  const previousDependencies: (readonly unknown[])[] = [];
  let currentState = initialState;
  let currentCompanyId = "C-600101";
  function propsFor(companyId: string): Props {
    const generation = currentState.generation;
    return { companyId, companyState: currentState, initialCivilDate: "2030-04-02", onCompanyChange: () => undefined, onQuery: () => undefined, onAdvanceCivilDay: async () => undefined, onReadingChange: (changes) => {
      currentState = companyReducer(currentState, updateCompanyReading({ generation, companyId, changes }));
    } };
  }
  // 仅模拟组件 state 与提交后的 effect；真实 DOM 和浏览器交互仍由 E2E 验证。
  const hook = memoryHook<Props, ReturnType<typeof views.CompanyPanel>>((props) => {
    let index = 0;
    internals.ReactCurrentDispatcher.current = {
      ...internals.ReactCurrentDispatcher.current,
      useEffect(effect, dependencies) {
        const previous = previousDependencies[index];
        if (previous === undefined || previous.length !== dependencies.length || dependencies.some((value, position) => !Object.is(value, previous[position]))) effects.push(effect);
        previousDependencies[index++] = dependencies;
      },
    };
    return views.CompanyPanel(props);
  }, propsFor(currentCompanyId));
  function render(companyId = currentCompanyId) {
    currentCompanyId = companyId;
    const result = hook.render(propsFor(companyId));
    for (const effect of effects.splice(0)) effect();
    return result;
  }
  function disclosure(element: React.ReactElement): React.ReactElement<{ selectedReportId: string; onSelect: (id: string) => void }> | undefined {
    for (const child of React.Children.toArray((element.props as { children?: React.ReactNode }).children)) {
      if (!React.isValidElement(child)) continue;
      if (typeof child.props === "object" && child.props !== null && "selectedReportId" in child.props) return child as ReturnType<typeof disclosure>;
      const found = disclosure(child);
      if (found !== undefined) return found;
    }
    return undefined;
  }
  const first = disclosure(render());
  assert.ok(first);
  first.props.onSelect("8");
  assert.equal(disclosure(render())?.props.selectedReportId, "8");
  return {
    apply: (action: Parameters<typeof companyReducer>[1]) => { currentState = companyReducer(currentState, action); render(); },
    selected: (companyId = currentCompanyId) => disclosure(render(companyId))?.props.selectedReportId,
  };
}

test("公开报告刷新经过 loading 或 error 后保留仍可见的用户选择", () => {
  const { query, reports, ready } = fixture();
  for (const transition of [
    startCompanyQuery(query),
    recordCompanyQueryFailure({ ...query, message: "报告刷新失败" }),
  ]) {
    const view = selectionView(ready);
    view.apply(transition);
    view.apply(recordCompanyPage({ ...query, reports, nextCursor: null }));
    assert.equal(view.selected(), "8");
  }
});

test("公开报告真正 empty 后清除选择，重新出现报告时使用当前默认报告", () => {
  const { query, reports, ready } = fixture();
  const view = selectionView(ready);
  view.apply(recordCompanyPage({ ...query, reports: [], nextCursor: null }));
  view.apply(recordCompanyPage({ ...query, reports, nextCursor: null }));
  assert.equal(view.selected(), "7");
});

test("切换公司时旧选择失效后使用新公司的可见报告", () => {
  const { ready } = fixture();
  const view = selectionView(ready);
  const nextReport = { ...publicReportGold(), company_id: "C-002156", id: "9" };
  nextReport.financials.scope = { Standalone: { entity_id: "C-002156" } };
  view.apply(recordCompanyPage({ generation: 1, companyId: "C-002156", cursor: null, reports: [nextReport], nextCursor: null }));
  assert.equal(view.selected("C-002156"), "9");
  assert.equal(view.selected("C-002156"), "9");
  assert.equal(view.selected("C-600101"), "8");
});
