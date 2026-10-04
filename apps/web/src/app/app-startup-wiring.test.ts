import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const source = readFileSync(new URL("../App.tsx", import.meta.url), "utf8");
const lifecycle = readFileSync(new URL("./useSessionHostLifecycle.ts", import.meta.url), "utf8");
const controls = readFileSync(new URL("./session-control-commands.ts", import.meta.url), "utf8");

test("外层 App 先选择，再挂载游戏；一次读档源在外层生命周期共享", () => {
  const app = source.slice(source.indexOf("function App()"));
  const shell = source.slice(source.indexOf("function AppShell("), source.indexOf("function App()"));
  assert.ok(app.includes("new InitialSaveSource<StrictSaveEnvelope>()"));
  assert.equal(shell.includes("new InitialSaveSource"), false);
  assert.ok(app.indexOf("<StartupScreen") < app.indexOf("<MarketRuntimeProvider"));
  assert.ok(app.includes("initialSaveSourceRef={initialSaveSourceRef}"));
  assert.ok(app.includes("startupTarget={startupTarget}"));
});

test("WASM 校验在快速槽读取和 Worker 创建前，远程地址显式传入宿主", () => {
  const initialization = lifecycle.slice(lifecycle.indexOf("let ownedHost"), lifecycle.indexOf("function createSessionHost("));
  assert.ok(initialization.indexOf("checkWasmEnvironment()") >= 0);
  assert.ok(initialization.indexOf("checkWasmEnvironment()") < initialization.indexOf("initialSaveSourceRef.current.read("));
  assert.ok(initialization.indexOf("checkWasmEnvironment()") < initialization.indexOf("await createHost("));
  assert.match(initialization, /if \(startupTarget.kind === "wasm"\) checkWasmEnvironment/);
  assert.match(lifecycle, /return createRemoteHost\(setup, seed, \{ baseUrl: startupTarget.baseUrl \}\)/);
  assert.doesNotMatch(initialization, /VITE_ENGINE_HOST|VITE_REMOTE_BASE_URL/);
});

test("加载和失败可以结束当前启动并重选，普通设置不重建宿主", () => {
  assert.match(source, /onClick=\{selectHost\}/);
  assert.ok(source.includes("void onSelectHost(stopStartupRef.current)"));
  assert.match(source, /role="status"[^>]*>正在加载行情引擎/);
  assert.match(lifecycle, /\[sessionSetup, startupTarget, pausePreferencesReady, refreshPlayerOrders, setIndicatorCalculator\]/);
  assert.ok(lifecycle.includes("releaseOwnedHost();"));
  assert.ok(lifecycle.includes("autoOrderMgrRef.current = null"));
  assert.doesNotMatch(source, /localStorage\.setItem\([^\n]*(?:mode|startupTarget)/);
});

test("重选先关闭当前实例并等待外层日终屏障，显式新局配置跨 Shell 共享", () => {
  const app = source.slice(source.indexOf("function App()"));
  const shell = source.slice(source.indexOf("function AppShell("), source.indexOf("function App()"));
  assert.ok(app.includes("new DayEndPersistence()"));
  assert.equal(shell.includes("new DayEndPersistence()"), false);
  assert.ok(app.includes("dayEndPersistenceRef={dayEndPersistenceRef}"));
  assert.ok(app.includes("sessionSetup={sessionSetup} setSessionSetup={setSessionSetup}"));
  const returning = app.slice(app.indexOf("const returnToStartup"), app.indexOf("if (startupTarget === null)"));
  assert.ok(returning.indexOf("stopSession()") < returning.indexOf("dayEndPersistenceRef.current.invalidate()"));
  assert.ok(returning.indexOf("await dayEndPersistenceRef.current.idle()") < returning.indexOf("setStartupTarget(null)"));
  assert.ok(shell.includes("companyCoordinatorRef.current?.dispose()"));
  assert.ok(shell.includes("stopStartupRef, returningToStartupRef"));
  assert.ok(lifecycle.includes("options.returningToStartupRef.current"));
});

test("已清理宿主的晚到更新和失败不能修改下一次启动的状态", () => {
  const initialization = lifecycle.slice(lifecycle.indexOf("let ownedHost"), lifecycle.indexOf("function createSessionHost("));
  assert.ok(initialization.includes("!cancelled && host === hostRef.current ? hostUpdateRef.current(update) : false"));
  assert.ok(initialization.includes("if (!cancelled && host === hostRef.current) fatalHostErrorRef.current(failure)"));
  assert.equal((controls.match(/if \(host === this\.ports\.hostRef\.current\) this\.ports\.onFatal\(failure\)/g) ?? []).length, 2);
  assert.match(source, /onFatal: \(failure\) => fatalHostErrorRef\.current\(failure\)/);
});

test("重选后即使 Redux 留有旧行情，也必须等待当前宿主的权威基线", () => {
  assert.ok(source.includes("const [hostBaselineReady, setHostBaselineReady] = useState(false)"));
  assert.ok(lifecycle.includes("options.setHostBaselineReady(false)"));
  const baseline = source.slice(source.indexOf("onBaseline("), source.indexOf("onApplied("));
  assert.ok(baseline.indexOf("setHostBaselineReady(true)") > baseline.indexOf("installBaselineRef.current(protocolState)"));
  assert.ok(source.includes("if (!ready || !hostBaselineReady || !hasSnapshot)"));
});
