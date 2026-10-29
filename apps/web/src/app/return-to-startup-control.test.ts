import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import React from "react";
import { createServer, type ViteDevServer } from "vite";

let vite: ViteDevServer;
let App: typeof import("../App.tsx");
before(async () => {
  vite = await createServer({ root: new URL("../..", import.meta.url).pathname, configFile: false, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  App = await vite.ssrLoadModule("/src/App.tsx") as typeof import("../App.tsx");
});
after(async () => { if (vite) await vite.close(); });

test("ready 游戏与存档区域的返回控件等待旧局结束，清楚显示提示和失败", { timeout: 10000 }, async () => {
  let release!: () => void;
  const barrier = new Promise<void>((resolve) => { release = resolve; });
  let stopped = false;
  let returning = false;
  let nextStartupReady = false;
  let selection: Promise<void> | null = null;
  const stopSession = async () => { stopped = true; };
  const returnError = "返回启动选择失败：原宿主停止失败；尚未开始下一局，请反馈此错误或重新打开应用。";
  const ReturnControl = (App as typeof App & { ReturnToStartupControl?: (props: { onReturn: () => void }) => React.ReactElement }).ReturnToStartupControl;
  assert.equal(typeof ReturnControl, "function", "ready settings 区域必须提供真实返回控件");
  const section = ReturnControl!({ onSelectHost: (stop) => {
    selection = (async () => {
      returning = true;
      await stop();
      await barrier;
      nextStartupReady = true;
      returning = false;
    })();
    return selection;
  }, stopSession, error: returnError });
  assert.equal(section.props["aria-label"], "返回启动选择");
  assert.match(section.props.children[0].props.children, /未保存的日内进度会丢失/);
  const button = section.props.children[1];
  assert.equal(button.props.children, "返回启动选择");
  button.props.onClick();
  assert.equal(stopped, true, "点击必须调用当前实例的 stop 函数");
  assert.equal(returning, true);
  assert.equal(nextStartupReady, false, "stop 完成但屏障未释放时不能开始下一局");
  release();
  await selection;
  assert.equal(nextStartupReady, true);
  assert.equal(section.props.children[2].props.role, "alert");
  assert.equal(section.props.children[2].props.children, returnError);
});
