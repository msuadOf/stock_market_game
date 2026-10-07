import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import React, { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";

let vite: ViteDevServer;
let TaxModeInput: typeof import("./TaxModeInput.tsx").TaxModeInput;
let MAX_FLAT_WITHHOLDING_BP: typeof import("./TaxModeInput.tsx").MAX_FLAT_WITHHOLDING_BP;

before(async () => {
  vite = await createServer({
    configFile: false,
    appType: "custom",
    server: { middlewareMode: true, ws: false },
    optimizeDeps: { noDiscovery: true },
  });
  const mod = await vite.ssrLoadModule("/src/components/TaxModeInput.tsx") as typeof import("./TaxModeInput.tsx");
  TaxModeInput = mod.TaxModeInput;
  MAX_FLAT_WITHHOLDING_BP = mod.MAX_FLAT_WITHHOLDING_BP;
});

after(async () => {
  await vite.close();
});

const NOOP = () => {};

/** 三层税制（2026-10-08 决策）的新局选项面：三选一（默认简税）、
 * 比例输入仅在简税时出现、非法比例不写草稿。 */
test("三层税务模式选项默认勾选简税并展示比例输入", () => {
  const markup = renderToStaticMarkup(createElement(TaxModeInput, {
    value: "FlatWithholding",
    onChange: NOOP,
    flatWithholdingBp: 1000,
    onFlatWithholdingBpChange: NOOP,
  }));
  assert.match(markup, /简税（分红到账时按比例直接扣，默认）/);
  assert.match(markup, /大 A 方式（个人差别化计税）/);
  assert.match(markup, /不扣税（连印花税也免）/);
  assert.match(markup, /代扣比例/);
  assert.match(markup, /1000bp/);
  const flatChecked = markup.match(/value="FlatWithholding"[^>]*checked/m) ?? markup.match(/checked[^>]*value="FlatWithholding"/m);
  assert.notEqual(flatChecked, null, "简税默认勾选");
});

test("非简税模式不展示比例输入但保留三选一", () => {
  for (const mode of ["AShareIndividual", "Exempt"] as const) {
    const markup = renderToStaticMarkup(createElement(TaxModeInput, {
      value: mode,
      onChange: NOOP,
      flatWithholdingBp: 1000,
      onFlatWithholdingBpChange: NOOP,
    }));
    assert.doesNotMatch(markup, /代扣比例/);
    assert.match(markup, /简税（分红到账时按比例直接扣，默认）/);
  }
});

test("比例上界常量与引擎契约一致", () => {
  assert.equal(MAX_FLAT_WITHHOLDING_BP, 10000);
});

type InputProps = {
  children?: React.ReactNode;
  "aria-label"?: string;
  "aria-invalid"?: boolean;
  role?: string;
  value?: string;
  onInput?: (event: { currentTarget: { value: string } }) => void;
};

function nodes(tree: React.ReactNode): React.ReactElement<InputProps>[] {
  if (Array.isArray(tree)) return tree.flatMap(nodes);
  if (!React.isValidElement<InputProps>(tree)) return [];
  return [tree, ...nodes(tree.props.children)];
}

/** 沿用组件交互测试的 dispatcher harness，执行真实 onInput 与草稿回写后的 effect。 */
function mountRateInput(initialBp = 1000) {
  const slots: { value: unknown; deps?: readonly unknown[] }[] = [];
  const effects: (() => void)[] = [];
  const writes: number[] = [];
  let cursor = 0;
  let draftBp = initialBp;
  let tree: React.ReactNode;
  const internal = (React as unknown as {
    __SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED: { ReactCurrentDispatcher: { current: unknown } };
  }).__SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED;
  const dispatcher = {
    useState(initial: unknown) {
      const index = cursor++;
      if (slots[index] === undefined) slots[index] = { value: initial };
      return [slots[index].value, (value: unknown) => { slots[index].value = value; }];
    },
    useEffect(effect: () => void, deps: readonly unknown[]) {
      const index = cursor++;
      const previous = slots[index]?.deps;
      if (previous === undefined || deps.some((value, position) => !Object.is(value, previous[position]))) {
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
      tree = TaxModeInput({ value: "FlatWithholding", onChange: NOOP, flatWithholdingBp: draftBp,
        onFlatWithholdingBpChange: (bp) => { writes.push(bp); draftBp = bp; } });
    } finally {
      internal.ReactCurrentDispatcher.current = previous;
    }
    while (effects.length > 0) effects.shift()!();
  }
  function input() {
    const node = nodes(tree).find((node) => node.props["aria-label"] === "简税代扣比例（百分比）");
    assert.ok(node);
    return node;
  }
  render(); render();
  return {
    writes,
    draftBp: () => draftBp,
    input,
    markup: () => renderToStaticMarkup(tree),
    enter(raw: string) {
      assert.ok(input().props.onInput);
      input().props.onInput!({ currentTarget: { value: raw } });
      render(); render();
    },
  };
}

test("负百分比输入不会因 bp 舍入被接受，保留草稿并显式提示", { timeout: 10000 }, () => {
  // 草稿设为舍入后的 0bp，确保错误提示不能仅靠「输入与草稿不同」触发。
  const ui = mountRateInput(0);
  for (const raw of ["-0.004", "-1"]) {
    ui.enter(raw);
    assert.deepEqual(ui.writes, [], `${raw}% 不应写入草稿`);
    assert.equal(ui.draftBp(), 0);
    assert.equal(ui.input().props.value, raw, "保留非法文本，不能静默归零");
    assert.equal(ui.input().props["aria-invalid"], true);
    assert.match(ui.markup(), /role="alert".*草稿保持 0bp/);
  }
});

test("略超 100% 的输入不会因 bp 舍入被接受，保留草稿并显式提示", { timeout: 10000 }, () => {
  const ui = mountRateInput(10000);
  for (const raw of ["100.004", "101"]) {
    ui.enter(raw);
    assert.deepEqual(ui.writes, [], `${raw}% 不应写入草稿`);
    assert.equal(ui.draftBp(), 10000);
    assert.equal(ui.input().props.value, raw);
    assert.equal(ui.input().props["aria-invalid"], true);
    assert.match(ui.markup(), /role="alert".*草稿保持 10000bp/);
  }
});

test("合法百分比保持 1bp 舍入与边界，并可修正非法输入", { timeout: 10000 }, () => {
  const ui = mountRateInput();
  ui.enter("101");
  assert.equal(ui.input().props["aria-invalid"], true);
  for (const [raw, bp] of [["0", 0], ["0.004", 0], ["0.005", 1], ["10.125", 1013], ["99.996", 10000], ["100", 10000]] as const) {
    ui.enter(raw);
    assert.equal(ui.writes.at(-1), bp, `${raw}% 转为 ${bp}bp`);
    assert.equal(ui.draftBp(), bp);
    assert.equal(ui.input().props["aria-invalid"], false);
    assert.doesNotMatch(ui.markup(), /role="alert"/);
  }
});
