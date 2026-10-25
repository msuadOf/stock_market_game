import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import React, { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";

let vite: ViteDevServer;
let ParValueInput: typeof import("./ParValueInput.tsx").ParValueInput;
let yuanTextToParCents: typeof import("./ParValueInput.tsx").yuanTextToParCents;

before(async () => {
  vite = await createServer({
    configFile: false,
    appType: "custom",
    server: { middlewareMode: true, ws: false },
    optimizeDeps: { noDiscovery: true },
  });
  const mod = await vite.ssrLoadModule("/src/components/ParValueInput.tsx") as typeof import("./ParValueInput.tsx");
  ParValueInput = mod.ParValueInput;
  yuanTextToParCents = mod.yuanTextToParCents;
});

after(async () => {
  await vite.close();
});

const NOOP = () => {};

test("面值输入展示默认草稿与失焦生效提示", () => {
  const markup = renderToStaticMarkup(createElement(ParValueInput, { value: "100", onChange: NOOP }));
  assert.match(markup, /每股面值/);
  assert.match(markup, /value="1\.00"/);
  assert.match(markup, /失焦或按 Enter 生效/);
  assert.match(markup, /面值 × 总股本/);
});

test("元文本换算保留规范分并拒绝非正/非法输入", () => {
  assert.equal(yuanTextToParCents("1"), "100");
  assert.equal(yuanTextToParCents("1.5"), "150");
  assert.equal(yuanTextToParCents("0.01"), "1");
  assert.equal(yuanTextToParCents(" 2 "), "200");
  assert.equal(yuanTextToParCents("0"), null);
  for (const invalid of ["", "0.001", "1.25.5", "-1", "abc", "1,5", "０.５"]) {
    assert.equal(yuanTextToParCents(invalid), null, `${JSON.stringify(invalid)} 必须被拒绝`);
  }
});


type InputProps = {
  children?: React.ReactNode;
  "aria-label"?: string;
  "aria-invalid"?: boolean;
  value?: string;
  onInput?: (event: { currentTarget: { value: string } }) => void;
  onBlur?: () => void;
  onKeyDown?: (event: { key: string }) => void;
};

function nodes(tree: React.ReactNode): React.ReactElement<InputProps>[] {
  if (Array.isArray(tree)) return tree.flatMap(nodes);
  if (!React.isValidElement<InputProps>(tree)) return [];
  return [tree, ...nodes(tree.props.children)];
}

/** 执行组件的真实事件处理器与 effect；不模拟浏览器 DOM 或原生数字输入的行为。 */
function mountParValue(initial = "100") {
  const slots: { value: unknown; deps?: readonly unknown[] }[] = [];
  const effects: (() => void)[] = [];
  const writes: string[] = [];
  let cursor = 0;
  let value = initial;
  let tree: React.ReactNode;
  const internal = (React as unknown as {
    __SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED: { ReactCurrentDispatcher: { current: unknown } };
  }).__SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED;
  const dispatcher = {
    useState(initialState: unknown) {
      const index = cursor++;
      if (slots[index] === undefined) slots[index] = { value: initialState };
      return [slots[index].value, (next: unknown) => { slots[index].value = next; }];
    },
    useEffect(effect: () => void, deps: readonly unknown[]) {
      const index = cursor++;
      const previous = slots[index]?.deps;
      if (previous === undefined || deps.some((item, position) => !Object.is(item, previous[position]))) {
        slots[index] = { value: undefined, deps };
        effects.push(effect);
      }
    },
  };
  function render() {
    const previous = internal.ReactCurrentDispatcher.current;
    cursor = 0;
    internal.ReactCurrentDispatcher.current = dispatcher;
    try {
      tree = ParValueInput({ value, onChange(next) { writes.push(next); value = next; } });
    } finally {
      internal.ReactCurrentDispatcher.current = previous;
    }
    while (effects.length > 0) effects.shift()!();
  }
  function input() {
    const node = nodes(tree).find((node) => node.props["aria-label"] === "每股面值（元）");
    assert.ok(node);
    return node;
  }
  function settle() { render(); render(); }
  settle();
  return {
    writes, input,
    value: () => value,
    markup: () => renderToStaticMarkup(tree),
    enter(raw: string) { input().props.onInput!({ currentTarget: { value: raw } }); settle(); },
    blur() { input().props.onBlur!(); settle(); },
    key(key: string) { input().props.onKeyDown!({ key }); settle(); },
    replace(next: string) { value = next; settle(); },
  };
}

test("面值逐次编辑不提交，失焦或 Enter 才写入精确分金额", { timeout: 10000 }, () => {
  const ui = mountParValue();
  ui.enter("1");
  ui.enter("15.23");
  assert.deepEqual(ui.writes, []);
  assert.equal(ui.value(), "100");
  assert.match(ui.markup(), /尚未生效/);
  ui.key("Escape");
  assert.deepEqual(ui.writes, []);
  ui.blur();
  assert.deepEqual(ui.writes, ["1523"]);
  ui.enter("90071992547409.93");
  ui.key("Enter");
  assert.deepEqual(ui.writes, ["1523", "9007199254740993"]);
  assert.equal(ui.input().props.value, "90071992547409.93");
  ui.blur();
  assert.equal(ui.writes.length, 2, "Enter 后失焦不重复提交");
});

test("面值非法编辑在失焦和 Enter 后均保留原文并持续显错", { timeout: 10000 }, () => {
  const ui = mountParValue();
  for (const raw of ["", "0", "0.00", "-1", "0.001", "1e2", "０.５", "92233720368547758.08"]) {
    ui.enter(raw);
    assert.equal(ui.input().props["aria-invalid"], true, `${raw} 应明确标错`);
    for (const submit of [ui.blur, () => ui.key("Enter")]) {
      submit();
      assert.equal(ui.input().props.value, raw, "不得回填旧值隐藏错误");
      assert.equal(ui.input().props["aria-invalid"], true);
      assert.match(ui.markup(), /role="alert".*当前面值输入无效/);
      assert.match(ui.markup(), /已设面值仍为 1.00 元/);
      assert.equal(ui.value(), "100");
      assert.deepEqual(ui.writes, []);
    }
  }
  ui.enter("0.01");
  ui.blur();
  assert.deepEqual(ui.writes, ["1"]);
  assert.doesNotMatch(ui.markup(), /role="alert"/);
});

test("面值上限精确提交，外部值变化同步输入且不反向提交", { timeout: 10000 }, () => {
  const ui = mountParValue();
  ui.enter("92233720368547758.07");
  ui.blur();
  assert.deepEqual(ui.writes, ["9223372036854775807"]);
  ui.enter("非法文本");
  ui.replace("123");
  assert.equal(ui.input().props.value, "1.23");
  assert.doesNotMatch(ui.markup(), /role="alert"/);
  assert.deepEqual(ui.writes, ["9223372036854775807"]);
});


test("面值超出金额上限时立即显错并阻止失焦提交", { timeout: 10000 }, () => {
  const ui = mountParValue();
  ui.enter("92233720368547758.08");
  assert.equal(ui.input().props["aria-invalid"], true);
  ui.blur();
  assert.equal(ui.input().props.value, "92233720368547758.08");
  assert.deepEqual(ui.writes, []);
  assert.equal(ui.value(), "100");
});
