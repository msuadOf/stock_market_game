import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { createElement } from "react";
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

/** 每股面值输入（2026-10-08 N2a 决策，默认 1 元/股）：渲染默认草稿、
 * 元↔分换算纪律、以及 SSR 可渲染性（交互级提交/失焦行为由 tsc 与
 * E2E 覆盖，SSR 不执行事件）。 */
test("面值输入展示默认草稿与失焦生效提示", () => {
  const markup = renderToStaticMarkup(createElement(ParValueInput, { value: "100", onChange: NOOP }));
  assert.match(markup, /每股面值/);
  assert.match(markup, /value="1\.00"/);
  assert.match(markup, /100 分，默认 1 元；失焦生效/);
  assert.match(markup, /面值 × 总股本/);
});

test("元文本换算保留规范分并拒绝非正/非法输入", () => {
  assert.equal(yuanTextToParCents("1"), "100");
  assert.equal(yuanTextToParCents("1.5"), "150");
  assert.equal(yuanTextToParCents("0.01"), "1");
  assert.equal(yuanTextToParCents(" 2 "), "200");
  assert.equal(yuanTextToParCents("0"), "0");
  for (const invalid of ["", "0.001", "1.25.5", "-1", "abc", "1,5", "０.５"]) {
    assert.equal(yuanTextToParCents(invalid), null, `${JSON.stringify(invalid)} 必须被拒绝`);
  }
});
