import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import React, { createElement, type ReactElement, type ReactNode } from "react";
import { parseReportFrequency } from "../save/schema/report-frequency.ts";
import type { ReportFrequencyDraft } from "./ReportFrequencyInput.tsx";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";

let vite: ViteDevServer;
let ReportFrequencyInput: typeof import("./ReportFrequencyInput.tsx").ReportFrequencyInput;
before(async () => {
  vite = await createServer({ configFile: false, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  ({ ReportFrequencyInput } = await vite.ssrLoadModule("/src/components/ReportFrequencyInput.tsx"));
});
after(async () => { await vite?.close(); });

test("月报选项不替法定报告且尚未选择schedule时显示必选提示", { timeout: 10000 }, () => {
  const html = renderToStaticMarkup(createElement(ReportFrequencyInput, { value: "Monthly", onChange() {} }));
  assert.match(html, /请明确选择月报排期/);
  assert.match(html, /次月1日18:00/);
  assert.match(html, /次月10日18:00/);
  assert.match(html, /自定义/);
  assert.match(html, /不替代.*法定/);
});

test("恢复自定义月报设置完整呈现日期时间与随机延迟而不改为默认预设", { timeout: 10000 }, () => {
  const html = renderToStaticMarkup(createElement(ReportFrequencyInput, { value: { Monthly: { schedule: { Custom: { day: 12, second_of_day: 54001, delay: { Uniform: { max_days: 2 } } } } } }, onChange() {} }));
  assert.match(html, /value="12"/);
  assert.match(html, /value="15:00:01"/);
  assert.match(html, /随机晚0至2天/);
  assert.match(html, /1–28/);
});

test("Quarterly默认不安装月报排期，恢复预设必须保留实际所选和可选延迟", { timeout: 10000 }, () => {
  const quarterly = renderToStaticMarkup(createElement(ReportFrequencyInput, { value: "Quarterly", onChange() {} }));
  assert.doesNotMatch(quarterly, /aria-label="月报排期"/);
  const preset = renderToStaticMarkup(createElement(ReportFrequencyInput, { value: { Monthly: { schedule: { Preset: { preset: "FirstDayEvening", delay: { Uniform: { max_days: 1 } } } } } }, onChange() {} }));
  assert.match(preset, /value="FirstDayEvening" selected/);
  assert.match(preset, /value="1" selected[^>]*>随机晚0至1天/);
});

type InputProps = { readonly children?: ReactNode; readonly value?: string; readonly "aria-label"?: string; readonly onChange?: (event: { currentTarget: { value: string } }) => void };
function elements(node: ReactNode): ReactElement<InputProps>[] {
  if (Array.isArray(node)) return node.flatMap(elements);
  if (!React.isValidElement<InputProps>(node)) return [];
  return [node, ...elements(node.props.children)];
}

function mountedSchedule(initial: ReportFrequencyDraft) {
  const slots: { value: unknown; dependencies?: readonly unknown[] }[] = [];
  let cursor = 0;
  let value = initial;
  const pending: (() => void)[] = [];
  const internals = (React as unknown as { __SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED: { ReactCurrentDispatcher: { current: unknown } } }).__SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED;
  const dispatcher = {
    useState(initialValue: unknown) {
      const index = cursor++;
      if (slots[index] === undefined) slots[index] = { value: typeof initialValue === "function" ? initialValue() : initialValue };
      return [slots[index].value, (next: unknown) => { slots[index].value = next; }];
    },
    useEffect(effect: () => void, dependencies: readonly unknown[]) {
      const index = cursor++;
      const previous = slots[index]?.dependencies;
      if (previous === undefined || dependencies.some((item, position) => !Object.is(item, previous[position]))) {
        slots[index] = { value: undefined, dependencies };
        pending.push(effect);
      }
    },
  };
  let tree: ReactNode;
  function render() {
    const outer = ReportFrequencyInput({ value, onChange: (next) => { value = next; } });
    const component = elements(outer).find((element) => typeof element.type === "function");
    assert.ok(component, "Monthly实际控件已挂载");
    cursor = 0;
    const previous = internals.ReactCurrentDispatcher.current;
    internals.ReactCurrentDispatcher.current = dispatcher;
    try { tree = (component.type as (props: unknown) => ReactNode)(component.props); }
    finally { internals.ReactCurrentDispatcher.current = previous; }
    while (pending.length > 0) pending.shift()!();
    return tree;
  }
  render();
  return {
    value: () => value,
    replace: (next: ReportFrequencyDraft) => { value = next; render(); render(); },
    change(label: string, next: string) {
      const input = elements(tree).find((element) => element.props["aria-label"] === label);
      assert.ok(input, label);
      input.props.onChange!({ currentTarget: { value: next } });
      render(); render();
    },
    field(label: string) { return elements(tree).find((element) => element.props["aria-label"] === label)?.props.value; },
  };
}

test("真实月报控件选择预设及编辑custom只发送完整排期，无效草稿拒绝创建", { timeout: 10000 }, () => {
  const ui = mountedSchedule("Monthly");
  assert.throws(() => parseReportFrequency(ui.value()), /请明确选择/);
  ui.change("月报排期", "FirstDayEvening");
  assert.deepEqual(ui.value(), { Monthly: { schedule: { Preset: { preset: "FirstDayEvening", delay: "None" } } } });
  ui.change("月报随机延迟", "2");
  assert.deepEqual(ui.value(), { Monthly: { schedule: { Preset: { preset: "FirstDayEvening", delay: { Uniform: { max_days: 2 } } } } } });
  ui.change("月报排期", "Custom");
  assert.throws(() => parseReportFrequency(ui.value()), /请明确选择/);
  ui.change("月报次月日期", "12");
  ui.change("月报公开时间", "15:00:01");
  assert.deepEqual(ui.value(), { Monthly: { schedule: { Custom: { day: 12, second_of_day: 54001, delay: { Uniform: { max_days: 2 } } } } } });
  ui.change("月报次月日期", "29");
  assert.equal(ui.value(), "Monthly");
  assert.throws(() => parseReportFrequency(ui.value()), /请明确选择/);
  ui.change("月报次月日期", "28");
  assert.doesNotThrow(() => parseReportFrequency(ui.value()));
});

test("同一挂载月报控件换档回填自定义时刻与预设，不残留前局draft", { timeout: 10000 }, () => {
  const ui = mountedSchedule({ Monthly: { schedule: { Custom: { day: 12, second_of_day: 54001, delay: { Uniform: { max_days: 2 } } } } } });
  ui.replace({ Monthly: { schedule: { Custom: { day: 5, second_of_day: 1, delay: "None" } } } });
  assert.equal(ui.field("月报次月日期"), "5");
  assert.equal(ui.field("月报公开时间"), "00:00:01");
  assert.equal(ui.field("月报随机延迟"), "0");
  ui.replace({ Monthly: { schedule: { Preset: { preset: "TenthDayEvening", delay: { Uniform: { max_days: 1 } } } } } });
  assert.equal(ui.field("月报排期"), "TenthDayEvening");
  assert.equal(ui.field("月报随机延迟"), "1");
});
