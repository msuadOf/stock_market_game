import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createElement, type ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";

let vite: ViteDevServer;
let screen: typeof import("./StartupScreen.tsx");

before(async () => {
  vite = await createServer({
    configFile: false, appType: "custom", mode: "production",
    server: { middlewareMode: true, ws: false },
    optimizeDeps: { noDiscovery: true },
    define: {
      "import.meta.env.DEV": "false",
      "import.meta.env.VITE_ENGINE_HOST": JSON.stringify("remote"),
      "import.meta.env.VITE_REMOTE_BASE_URL": JSON.stringify("https://compiled.example"),
    },
  });
  screen = await vite.ssrLoadModule("/src/app/StartupScreen.tsx") as typeof screen;
});

after(async () => { if (vite) await vite.close(); });

const props = {
  mode: "local" as const, remoteAddress: "", error: null, developmentHint: false,
  onModeChange: () => {}, onAddressChange: () => {}, onStart: () => {},
};

test("启动页使用原生可访问表单，显示未连接与非热迁移语义", () => {
  const html = renderToStaticMarkup(createElement(screen.StartupScreen, props));
  assert.match(html, /<form[^>]*novalidate/);
  assert.match(html, /<main[^>]*aria-labelledby="startup-title"/);
  assert.match(html, /<fieldset><legend>引擎运行位置<\/legend>/);
  assert.match(html, /<input(?=[^>]*type="radio")(?=[^>]*value="local")(?=[^>]*checked)[^>]*>/);
  assert.match(html, /尚未连接游戏引擎/);
  assert.match(html, /不会再次读取快速槽/);
  assert.match(html, /WebSocket 自动派生/);
  assert.doesNotMatch(html, /inputMode="url"|compiled\.example|开发环境变量/);
});

test("远程地址可输入且错误显式展示，只有提交才发起启动", () => {
  const addresses: string[] = [];
  const modes: string[] = [];
  let starts = 0;
  let prevented = 0;
  const remoteProps = {
    ...props, mode: "remote" as const, remoteAddress: "https://server.example/game",
    error: "Server 地址无效", developmentHint: true,
    onModeChange: (mode: string) => { modes.push(mode); },
    onAddressChange: (value: string) => { addresses.push(value); },
    onStart: () => { starts += 1; },
  };
  const html = renderToStaticMarkup(createElement(screen.StartupScreen, remoteProps));
  assert.match(html, /Server HTTP\(S\) 地址/);
  assert.match(html, /value="https:\/\/server.example\/game"/);
  assert.match(html, /aria-invalid="true" required/);
  assert.match(html, /role="alert" aria-live="assertive">Server 地址无效/);
  assert.match(html, /开发环境变量仅作为初值提示/);
  const view = screen.StartupScreen(remoteProps);
  const form = (view.props.children as ReactElement[]).find((child) => child.type === "form")!;
  assert.equal(starts, 0);
  const fieldset = form.props.children[0];
  fieldset.props.children[1].props.children[0].props.onChange();
  fieldset.props.children[2].props.children[0].props.onChange();
  const addressLabel = form.props.children[2];
  addressLabel.props.children[1].props.onChange({ currentTarget: { value: "http://server.example/game" } });
  assert.deepEqual(modes, ["local", "remote"]);
  assert.deepEqual(addresses, ["http://server.example/game"]);
  assert.equal(starts, 0);
  form.props.onSubmit({ preventDefault() { prevented += 1; } });
  assert.equal(prevented, 1);
  assert.equal(starts, 1);
});

test("生产 App 即使有旧编译环境变量，也先显示选择而不读取浏览器存档", async () => {
  const app = await vite.ssrLoadModule("/src/App.tsx") as typeof import("../App.tsx");
  const html = renderToStaticMarkup(createElement(app.default));
  assert.match(html, /股票模拟游戏 · 启动选择/);
  assert.match(html, /尚未连接游戏引擎/);
  assert.doesNotMatch(html, /compiled\.example|正在加载行情引擎|开发环境变量/);
});
