import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";
import type { ReportFrequencyDraft } from "./ReportFrequencyInput.tsx";

let vite: ViteDevServer;
let ReportFrequencyInput: typeof import("./ReportFrequencyInput.tsx").ReportFrequencyInput;
before(async () => {
  vite = await createServer({ configFile: false, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  ({ ReportFrequencyInput } = await vite.ssrLoadModule("/src/components/ReportFrequencyInput.tsx"));
});
after(async () => { await vite.close(); });

type Props = { children?: React.ReactNode; "aria-label"?: string; value?: string; onChange?: (event: { currentTarget: { value: string }; target: { value: string } }) => void };
function nodes(tree: React.ReactNode): React.ReactElement<Props>[] {
  if (Array.isArray(tree)) return tree.flatMap(nodes);
  if (!React.isValidElement<Props>(tree)) return [];
  return [tree, ...nodes(tree.props.children)];
}

/** 只运行真实组件与回调，手工提交 effect；不模拟 DOM 或浏览器事件。 */
function mount(initial: ReportFrequencyDraft = "Quarterly") {
  let value = initial;
  const writes: ReportFrequencyDraft[] = [];
  const slots: { value?: unknown; deps?: readonly unknown[] }[] = [];
  let cursor = 0;
  const effects: (() => void)[] = [];
  let tree: React.ReactNode;
  const internal = (React as unknown as { __SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED: { ReactCurrentDispatcher: { current: unknown } } }).__SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED;
  const dispatcher = {
    useState(initialState: unknown) {
      const index = cursor++;
      if (slots[index] === undefined) slots[index] = { value: typeof initialState === "function" ? initialState() : initialState };
      return [slots[index].value, (next: unknown) => { slots[index].value = typeof next === "function" ? next(slots[index].value) : next; }];
    },
    useRef(initialState: unknown) {
      const index = cursor++;
      if (slots[index] === undefined) slots[index] = { value: { current: initialState } };
      return slots[index].value;
    },
    useEffect(effect: () => void, deps: readonly unknown[]) {
      const index = cursor++;
      const previous = slots[index]?.deps;
      if (previous === undefined || previous.length !== deps.length || deps.some((entry, position) => !Object.is(entry, previous[position]))) {
        slots[index] = { deps }; effects.push(effect);
      }
    },
  };
  function expand(node: React.ReactNode): React.ReactNode {
    if (Array.isArray(node)) return node.map(expand);
    if (!React.isValidElement<Props>(node)) return node;
    if (typeof node.type === "function") return expand((node.type as (props: Props) => React.ReactNode)(node.props));
    return React.cloneElement(node, undefined, expand(node.props.children));
  }
  function render() {
    const previous = internal.ReactCurrentDispatcher.current;
    cursor = 0; internal.ReactCurrentDispatcher.current = dispatcher;
    try { tree = expand(ReportFrequencyInput({ value, onChange: (next) => { value = next; writes.push(next); } })); }
    finally { internal.ReactCurrentDispatcher.current = previous; }
    while (effects.length > 0) effects.shift()!();
  }
  function field(label: string) {
    const node = nodes(tree).find((entry) => entry.props["aria-label"] === label);
    assert.ok(node, `缺少 ${label}`); return node;
  }
  render(); render();
  return {
    writes, value: () => value, field,
    markup: () => renderToStaticMarkup(tree),
    enter(label: string, raw: string) {
      const node = field(label); assert.ok(node.props.onChange);
      node.props.onChange({ currentTarget: { value: raw }, target: { value: raw } }); render(); render();
    },
    replace(next: ReportFrequencyDraft) { value = next; render(); render(); },
  };
}
const preset = (name: "FirstDayEvening" | "TenthDayEvening"): ReportFrequencyDraft => ({ Monthly: { schedule: { Preset: { preset: name, delay: "None" } } } });

test("Monthly 未明确排期仅回写不可创建草稿，Quarterly 与法定披露边界可见", { timeout: 10000 }, () => {
  const ui = mount();
  assert.equal(ui.field("财报公开频率").props.value, "Quarterly");
  ui.enter("财报公开频率", "Monthly");
  assert.equal(ui.value(), "Monthly");
  assert.equal(ui.field("月报排期").props.value, "");
  assert.match(ui.markup(), /请明确选择月报排期后再创建新局/);
  assert.match(ui.markup(), /额外游戏报告.*不替代法定年报、中报和季度报告/);
  assert.match(ui.markup(), /税务核算仍按年度/);
  ui.enter("月报排期", "Custom");
  ui.enter("月报次月日期", "1");
  assert.equal(ui.value(), "Monthly", "缺失时间仍不可创建");
  ui.enter("财报公开频率", "Quarterly");
  assert.equal(ui.value(), "Quarterly");
  assert.doesNotMatch(ui.markup(), /aria-label="月报排期"/);
});

test("两个 Preset、None 和 Uniform 1..31 真实回写引擎契约", { timeout: 10000 }, () => {
  const ui = mount("Monthly");
  for (const name of ["FirstDayEvening", "TenthDayEvening"] as const) {
    ui.enter("月报排期", name); assert.deepEqual(ui.value(), preset(name));
    for (const maximum of [1, 31]) {
      ui.enter("月报随机延迟", String(maximum));
      assert.deepEqual(ui.value(), { Monthly: { schedule: { Preset: { preset: name, delay: { Uniform: { max_days: maximum } } } } } });
    }
    ui.enter("月报随机延迟", "0"); assert.deepEqual(ui.value(), preset(name));
  }
});

test("Custom 的两端日与秒数保持精确，非法输入不截断且显式报错", { timeout: 10000 }, () => {
  const ui = mount("Monthly");
  ui.enter("月报排期", "Custom"); ui.enter("月报次月日期", "1"); ui.enter("月报公开时间", "00:00:00");
  assert.deepEqual(ui.value(), { Monthly: { schedule: { Custom: { day: 1, second_of_day: 0, delay: "None" } } } });
  ui.enter("月报次月日期", "28"); ui.enter("月报公开时间", "23:59:59");
  assert.deepEqual(ui.value(), { Monthly: { schedule: { Custom: { day: 28, second_of_day: 86399, delay: "None" } } } });
  for (const raw of ["0", "29", "1.5", "1e1", "-1", "文字"]) {
    ui.enter("月报次月日期", raw); assert.equal(ui.value(), "Monthly");
    assert.equal(ui.field("月报次月日期").props.value, raw); assert.match(ui.markup(), /role="alert"/);
  }
  ui.enter("月报次月日期", "1");
  for (const raw of ["24:00:00", "23:60:00", "23:59:60", "12:00:01.1", "文字"]) {
    ui.enter("月报公开时间", raw); assert.equal(ui.value(), "Monthly");
    assert.equal(ui.field("月报公开时间").props.value, raw); assert.match(ui.markup(), /role="alert"/);
  }
  ui.enter("月报公开时间", "12:34");
  assert.deepEqual(ui.value(), { Monthly: { schedule: { Custom: { day: 1, second_of_day: 45240, delay: "None" } } } });
  assert.doesNotMatch(ui.markup(), /role="alert"/);
});

test("延迟空值与非整数字符串不被 Number 强转为合法上限", { timeout: 10000 }, () => {
  const ui = mount(preset("FirstDayEvening"));
  for (const raw of ["", " ", "-1", "32", "1.5", "0x1", "1e1", "NaN"]) {
    ui.enter("月报随机延迟", raw); assert.equal(ui.value(), "Monthly", `${raw} 应保持不可创建`);
    assert.match(ui.markup(), /role="alert"/);
  }
  ui.enter("月报随机延迟", "31");
  assert.deepEqual(ui.value(), { Monthly: { schedule: { Preset: { preset: "FirstDayEvening", delay: { Uniform: { max_days: 31 } } } } } });
});

test("外部 Monthly 未选排期清空旧排期，其他外部 value 同步且不发回调", { timeout: 10000 }, () => {
  const ui = mount(preset("FirstDayEvening"));
  ui.replace("Monthly");
  assert.equal(ui.field("月报排期").props.value, "");
  assert.match(ui.markup(), /请明确选择月报排期/);
  ui.replace({ Monthly: { schedule: { Custom: { day: 28, second_of_day: 86399, delay: { Uniform: { max_days: 31 } } } } } });
  assert.equal(ui.field("月报排期").props.value, "Custom");
  assert.equal(ui.field("月报次月日期").props.value, "28");
  assert.equal(ui.field("月报公开时间").props.value, "23:59:59");
  assert.equal(ui.field("月报随机延迟").props.value, "31");
  ui.replace(preset("TenthDayEvening")); assert.equal(ui.field("月报排期").props.value, "TenthDayEvening");
  assert.deepEqual(ui.writes, []);
});

test("模式来回切换不恢复未确认的旧排期，未知选择显式拒绝", { timeout: 10000 }, () => {
  const ui = mount(preset("TenthDayEvening"));
  ui.enter("财报公开频率", "Quarterly"); ui.enter("财报公开频率", "Monthly");
  assert.equal(ui.value(), "Monthly"); assert.equal(ui.field("月报排期").props.value, "");
  ui.enter("月报排期", "UnknownPreset"); assert.equal(ui.value(), "Monthly"); assert.match(ui.markup(), /role="alert"/);
  assert.throws(() => ui.enter("财报公开频率", "Unknown"), /无效财报频率/);
});
