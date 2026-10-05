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

function selectionView(initialState: ReturnType<typeof fixture>["ready"], onAvailabilityQuery?: (query: Parameters<Parameters<typeof views.CompanyPanel>[0]["onAvailabilityQuery"]>[0]) => Promise<Awaited<ReturnType<Parameters<typeof views.CompanyPanel>[0]["onAvailabilityQuery"]>>>) {
  type Props = Parameters<typeof views.CompanyPanel>[0];
  type Dispatcher = { useEffect: (effect: () => void, dependencies: readonly unknown[]) => void };
  const internals = (React as unknown as { __SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED: { ReactCurrentDispatcher: { current: Dispatcher } } }).__SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED;
  const effects: (() => void)[] = [];
  const previousDependencies: (readonly unknown[])[] = [];
  let currentState = initialState;
  let currentCompanyId = "C-600101";
  let currentTimelineGeneration: string | null = "1";
  const availabilityQueries: unknown[] = [];
  const availabilityCompletions: Promise<void>[] = [];
  function propsFor(companyId: string): Props {
    const generation = currentState.generation;
    return { companyId, companyState: currentState, initialCivilDate: "2030-04-02", timelineGeneration: currentTimelineGeneration, onCompanyChange: () => undefined, onQuery: () => undefined, onAvailabilityQuery: (query) => { availabilityQueries.push(query); const result = onAvailabilityQuery === undefined ? Promise.resolve({ status: "Unavailable", reason: "NotYetPublished" } as const) : onAvailabilityQuery(query); availabilityCompletions.push(result.then(() => undefined, () => undefined)); return result; }, onAdvanceCivilDay: async () => undefined, onReadingChange: (changes) => {
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
  function renderWithoutEffects(companyId = currentCompanyId, timelineGeneration = currentTimelineGeneration) {
    currentCompanyId = companyId;
    currentTimelineGeneration = timelineGeneration;
    return hook.render(propsFor(companyId));
  }
  function flushEffects() {
    for (const effect of effects.splice(0)) effect();
  }
  function render(companyId = currentCompanyId, timelineGeneration = currentTimelineGeneration) {
    const result = renderWithoutEffects(companyId, timelineGeneration);
    flushEffects();
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
  function elements(element: React.ReactElement): React.ReactElement[] {
    return [element, ...React.Children.toArray((element.props as { children?: React.ReactNode }).children).flatMap((child) => React.isValidElement(child) ? elements(child) : [])];
  }
  function textContent(element: React.ReactElement): string {
    return React.Children.toArray((element.props as { children?: React.ReactNode }).children).map((child) => React.isValidElement(child) ? textContent(child) : String(child)).join("");
  }
  function startAvailabilityQuery(): Promise<void> {
    const controls = elements(render());
    const period = controls.find((element) => element.props["aria-label"] === "报告期末");
    const kind = controls.find((element) => element.props["aria-label"] === "报告类型");
    const scope = controls.find((element) => element.props["aria-label"] === "报告 scope");
    assert.ok(period && kind && scope);
    (period.props.onChange as (event: { currentTarget: { value: string } }) => void)({ currentTarget: { value: "2030-03-31" } });
    (kind.props.onChange as (event: { currentTarget: { value: string } }) => void)({ currentTarget: { value: "Quarter" } });
    (scope.props.onChange as (event: { currentTarget: { value: string } }) => void)({ currentTarget: { value: "Standalone" } });
    const button = elements(render()).find((element) => element.type === "button" && element.props.children === "查询该报告是否已公开");
    assert.ok(button);
    (button.props.onClick as () => void)();
    const completion = availabilityCompletions.at(-1);
    assert.ok(completion);
    return completion;
  }
  const first = disclosure(render());
  assert.ok(first);
  first.props.onSelect("8");
  assert.equal(disclosure(render())?.props.selectedReportId, "8");
  return {
    apply: (action: Parameters<typeof companyReducer>[1]) => { currentState = companyReducer(currentState, action); render(); },
    selected: (companyId = currentCompanyId) => disclosure(render(companyId))?.props.selectedReportId,
    render,
    renderWithoutEffects,
    flushEffects,
    elements,
    textContent,
    startAvailabilityQuery,
    availabilityQueries,
  };
}

test("公司面板按钮调用宿主可用性查询并发送必填期间、类型与 scope", async () => {
  const view = selectionView(fixture().ready);
  let tree = view.render();
  const controls = view.elements(tree);
  const period = controls.find((element) => element.props["aria-label"] === "报告期末");
  const kind = controls.find((element) => element.props["aria-label"] === "报告类型");
  const scope = controls.find((element) => element.props["aria-label"] === "报告 scope");
  assert.ok(period && kind && scope);
  (period.props.onChange as (event: { currentTarget: { value: string } }) => void)({ currentTarget: { value: "2030-03-31" } });
  (kind.props.onChange as (event: { currentTarget: { value: string } }) => void)({ currentTarget: { value: "Quarter" } });
  (scope.props.onChange as (event: { currentTarget: { value: string } }) => void)({ currentTarget: { value: "Standalone" } });
  tree = view.render();
  const button = view.elements(tree).find((element) => element.type === "button" && element.props.children === "查询该报告是否已公开");
  assert.ok(button);
  await (button.props.onClick as () => Promise<void>)();
  assert.deepEqual(view.availabilityQueries, [{ company_id: "C-600101", period_end: "2030-03-31", kind: "Quarter", scope: { Standalone: { entity_id: "C-600101" } } }]);
});

test("时间线切换后旧可用性成功和失败响应都不会覆盖当前面板", async () => {
  for (const rejectLate of [false, true]) {
    let resolveAvailability!: (result: Awaited<ReturnType<Parameters<typeof views.CompanyPanel>[0]["onAvailabilityQuery"]>>) => void;
    let rejectAvailability!: (error: Error) => void;
    const pending = new Promise<Awaited<ReturnType<Parameters<typeof views.CompanyPanel>[0]["onAvailabilityQuery"]>>>((resolve, reject) => {
      resolveAvailability = resolve;
      rejectAvailability = reject;
    });
    const view = selectionView(fixture().ready, () => pending);
    const request = view.startAvailabilityQuery();
    assert.equal(view.availabilityQueries.length, 1);
    const beforeResponse = view.renderWithoutEffects("C-600101", "2");
    const beforeText = view.textContent(beforeResponse);
    assert.doesNotMatch(beforeText, /该报告已公开|旧时间线查询失败/);
    if (rejectLate) rejectAvailability(new Error("旧时间线查询失败"));
    else resolveAvailability({ status: "Available", report: publicReportGold() });
    await request;
    await pending.catch(() => undefined);
    await Promise.resolve();
    const rendered = view.textContent(view.renderWithoutEffects());
    assert.doesNotMatch(rendered, /该报告已公开|旧时间线查询失败/);
    view.flushEffects();
  }
});

test("新时间线的 effect 执行前不显示旧可用性结果或加载状态", async () => {
  const view = selectionView(fixture().ready, async () => ({ status: "Available", report: publicReportGold() }));
  await view.startAvailabilityQuery();
  view.render();
  const oldTimeline = view.textContent(view.renderWithoutEffects());
  assert.match(oldTimeline, /该报告已公开/);
  const newTimeline = view.renderWithoutEffects("C-600101", "2");
  const text = view.textContent(newTimeline);
  const button = view.elements(newTimeline).find((element) => element.type === "button" && element.props.children === "查询该报告是否已公开");
  assert.ok(button);
  assert.doesNotMatch(text, /该报告已公开|正在查询/);
  assert.equal(button.props.disabled, true);
  assert.equal(view.elements(newTimeline).find((element) => element.props["aria-label"] === "报告期末")?.props.value, "");
  assert.equal(view.elements(newTimeline).find((element) => element.props["aria-label"] === "报告类型")?.props.value, "");
  assert.equal(view.elements(newTimeline).find((element) => element.props["aria-label"] === "报告 scope")?.props.value, "");
  view.flushEffects();
});

test("同一时间线切换公司时 effect 前隔离旧响应、报告与表单", async () => {
  for (const rejectLate of [false, true]) {
    let resolveAvailability!: (result: Awaited<ReturnType<Parameters<typeof views.CompanyPanel>[0]["onAvailabilityQuery"]>>) => void;
    let rejectAvailability!: (error: Error) => void;
    const pending = new Promise<Awaited<ReturnType<Parameters<typeof views.CompanyPanel>[0]["onAvailabilityQuery"]>>>((resolve, reject) => {
      resolveAvailability = resolve;
      rejectAvailability = reject;
    });
    const view = selectionView(fixture().ready, () => pending);
    const request = view.startAvailabilityQuery();
    const nextCompany = view.renderWithoutEffects("C-002156", "1");
    const text = view.textContent(nextCompany);
    assert.doesNotMatch(text, /该报告已公开|正在查询|旧公司查询失败/);
    const controls = view.elements(nextCompany);
    const queryButton = controls.find((element) => element.type === "button" && element.props.children === "查询该报告是否已公开");
    assert.ok(queryButton);
    assert.equal(controls.find((element) => element.props["aria-label"] === "报告期末")?.props.value, "");
    assert.equal(controls.find((element) => element.props["aria-label"] === "报告类型")?.props.value, "");
    assert.equal(controls.find((element) => element.props["aria-label"] === "报告 scope")?.props.value, "");
    if (rejectLate) rejectAvailability(new Error("旧公司查询失败"));
    else resolveAvailability({ status: "Available", report: publicReportGold() });
    await request;
    await pending.catch(() => undefined);
    assert.doesNotMatch(view.textContent(view.renderWithoutEffects()), /该报告已公开|旧公司查询失败/);
    view.flushEffects();
  }
  const completed = selectionView(fixture().ready, async () => ({ status: "Available", report: publicReportGold() }));
  await completed.startAvailabilityQuery();
  completed.render();
  assert.match(completed.textContent(completed.renderWithoutEffects()), /该报告已公开/);
  const switched = completed.renderWithoutEffects("C-002156", "1");
  assert.doesNotMatch(completed.textContent(switched), /该报告已公开/);
  completed.flushEffects();
});

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
