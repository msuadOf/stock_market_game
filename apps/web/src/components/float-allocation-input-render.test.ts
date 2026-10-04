import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";
import { DEFAULT_SETUP } from "../config/defaults.ts";

let vite: ViteDevServer;
let FloatAllocationInput: typeof import("./FloatAllocationInput.tsx").FloatAllocationInput;

before(async () => {
  vite = await createServer({ configFile: false, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  ({ FloatAllocationInput } = await vite.ssrLoadModule("/src/components/FloatAllocationInput.tsx") as typeof import("./FloatAllocationInput.tsx"));
});

after(async () => {
  if (vite) await vite.close();
});

test("新局流通盘设置显示类间比例与两种类内策略", () => {
  const markup = renderToStaticMarkup(
    createElement(FloatAllocationInput, { value: DEFAULT_SETUP.float_allocation, onChange() {} }),
  );

  assert.match(markup, /类别间分配方式/);
  assert.match(markup, /类别内分配方式/);
  assert.match(markup, /散户类别占比百分数/);
  assert.match(markup, /value="45"/);
  assert.match(markup, /value="53"/);
  assert.match(markup, /value="2"/);
  assert.match(markup, /等比例分配/);
  assert.match(markup, /随机分配/);
});
