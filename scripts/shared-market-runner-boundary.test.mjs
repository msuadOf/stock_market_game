import assert from "node:assert/strict";
import { readFile, readdir } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { it } from "node:test";
import ts from "../apps/web/node_modules/typescript/lib/typescript.js";

const root = new URL("../", import.meta.url);
const read = (name) => readFile(new URL(name, root), "utf8");
const hosts = ["apps/server/src/actor.rs", "apps/desktop/src-tauri/src/actor.rs", "apps/web-wasm/src/market_runner.rs"];
// 此词法门禁仅用于限制明确的 Rust 分支/依赖；它不冒充 Rust AST 或行为证明。
const code = (source) => source.replace(/\/\*[\s\S]*?\*\//g, " ").replace(/\/\/[^\n]*/g, " ").replace(/"(?:\\.|[^"\\])*"/g, '""');

it("三端完整自动与手动决策归 Authority，端点不得复制错误分类", { timeout: 10000 }, async () => {
  for (const path of hosts) {
    const source = code(await read(path));
    assert.match(source, /authority::drive\s*\(/, path);
    assert.match(source, /authority::manual_tick\s*\(/, path);
    assert.match(source, /authority::manual_civil\s*\(/, path);
    assert.doesNotMatch(source, /\b(?:CycleError|ManualCivilError)::|\bManualStepError::Step\b/, path);
    assert.doesNotMatch(source, /\b(?:apply_cycle_pacing|apply_authority_failure)\s*\(/, path);
    assert.doesNotMatch(source, /\b(?:target_arch|target_family|target_os)\b|\bpause_preferences\.pauses\(/, path);
  }
  for (const path of ["apps/server/src/actor.rs", "apps/web-wasm/src/lib.rs", "apps/desktop/src-tauri/src/actor/failure.rs"]) {
    const source = code(await read(path));
    assert.doesNotMatch(source, /\bstruct\s+(?:HostFailure|FailureContext|FailureCause)\b|\bfn\s+failure_description\b/, path);
  }
});

it("共享 authority 及生命周期闭包不识别平台或直接调用目标库", { timeout: 10000 }, async () => {
  const roots = ["authority", "runner", "checkpoint", "publication", "replacement"];
  const paths = roots.map((name) => `packages/engine/src/session/${name}.rs`);
  for (const name of roots) {
    try {
      for (const entry of await readdir(new URL(`packages/engine/src/session/${name}/`, root), { recursive: true })) {
        if (entry.endsWith(".rs") && !/(?:^|\/)tests\.rs$|_tests\.rs$/.test(entry)) paths.push(`packages/engine/src/session/${name}/${entry}`);
      }
    } catch (error) { if (error.code !== "ENOENT") throw error; }
  }
  for (const path of paths) {
    const source = code(await read(path));
    assert.doesNotMatch(source, /\b(?:target_arch|target_family|target_os|is_platform)\b/, path);
    assert.doesNotMatch(source, /\b(?:tokio|web_sys|js_sys|wasm_bindgen|tauri|axum|native_store)::/, path);
  }
});

it("Cargo 实际清单禁止 engine 平台依赖和 LibOS 反向业务依赖", { timeout: 10000 }, () => {
  // metadata 只在正式测试阶段执行；不构建产物，也不访问网络。
  const result = spawnSync("cargo", ["metadata", "--offline", "--locked", "--no-deps", "--format-version", "1"], {
    cwd: root, encoding: "utf8", timeout: 8000, maxBuffer: 8 * 1024 * 1024,
  });
  assert.equal(result.error, undefined, `Cargo metadata 无法完成：${result.error}`);
  assert.equal(result.status, 0, result.stderr);
  const metadata = JSON.parse(result.stdout);
  const engine = metadata.packages.find(({ name }) => name === "engine");
  const libos = metadata.packages.find(({ name }) => name === "libos");
  assert.ok(engine && libos, "缺少生产 crate");
  const allowed = new Set(["libos", "serde", "serde_json", "thiserror", "num-bigint", "ts-rs"]);
  for (const dependency of engine.dependencies.filter(({ kind }) => kind === null)) {
    assert.ok(allowed.has(dependency.name), `engine 新依赖须经 ADR 复核：${dependency.name}`);
    assert.equal(dependency.target, null, `engine 平台依赖：${dependency.name}`);
  }
  for (const dependency of libos.dependencies) {
    assert.ok(!["engine", "server", "web-wasm", "native-store"].includes(dependency.name), `LibOS 反向业务依赖：${dependency.name}`);
  }
  for (const [feature, dependencies] of Object.entries(engine.features)) {
    assert.ok(!/wasm|native|browser|desktop|server/i.test(feature), `engine 平台业务 feature：${feature}`);
    assert.ok(dependencies.every((name) => !/^(?:tokio|web-sys|js-sys|tauri|axum)(?:\/|$)/.test(name)), `engine feature 引入目标库：${feature}`);
  }
});

it("日终编排只在 ArchiveAuthority，端点只提供非阻塞传输", { timeout: 10000 }, async () => {
  for (const [path, sink] of [
    ["apps/server/src/actor.rs", /\blane\.stage\(/],
    ["apps/desktop/src-tauri/src/actor.rs", /\blane\.ok_or_else\([\s\S]{0,150}?\)\?\s*\.stage\(writer\.clone\(\), captured\)/],
    ["apps/web-wasm/src/market_runner/checkpoint.rs", /\b(?:self\.)?tx\.try_send\(/],
  ]) {
    const source = code(await read(path));
    assert.match(source, /\bArchiveAuthority\b/, path);
    assert.match(source, sink, path);
    assert.doesNotMatch(source, /\bCapturedCheckpoint::take\s*\(|\.(?:can_stage|staged|accept_receipt|accept_pending_seq)\s*\(/, path);
  }
  const app = await read("apps/web/src/App.tsx");
  assert.doesNotMatch(app, /getBrowserSaveRepository\(\)\.save\(/);
  assert.match(app, /mirrorAuthorizedCheckpoint/);
});

it("发布和存储 Worker 不拥有市场推进，市场不等待 UI ACK", { timeout: 10000 }, async () => {
  const authority = await read("apps/web/src/host/wasm-worker.ts");
  assert.doesNotMatch(authority, /\bnew\s+WasmTickLoop\b|case\s+["']uiFrame["']\s*:/);
  const control = await read("apps/web/src/host/engine-control-worker.ts");
  assert.match(control, /wasm-publisher-worker\.ts/);
  for (const path of ["wasm-publisher-worker.ts", "wasm-checkpoint-worker.ts"]) {
    const source = await read("apps/web/src/host/" + path);
    assert.doesNotMatch(source, /\b(?:run_cycle|market_runner_turn|step_frame)\s*\(/, path);
  }
  const archive = await read("apps/web/src/host/wasm-checkpoint-worker.ts");
  assert.match(archive, /market_archive_take/);
  assert.doesNotMatch(archive, /ackPublisher|market_publisher_take/);
});

it("客户端共享重同步策略的 AST 无平台选择、线程、时钟或网络机制", { timeout: 10000 }, async () => {
  const path = "apps/web/src/host/observer-resync.ts";
  const source = ts.createSourceFile(path, await read(path), ts.ScriptTarget.Latest, true);
  assert.equal(source.parseDiagnostics.length, 0, path);
  const forbidden = new Set(["Atomics", "Worker", "WebAssembly", "fetch", "WebSocket", "setTimeout", "setInterval", "performance", "Date", "__TAURI__"]);
  function visit(node) {
    if (ts.isIdentifier(node)) assert.ok(!forbidden.has(node.text), `${path}: 共享策略出现平台机制 ${node.text}`);
    if (ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) {
      if (node.moduleSpecifier !== undefined) {
        assert.ok(ts.isStringLiteral(node.moduleSpecifier), "共享策略依赖必须可静态解析");
        assert.doesNotMatch(node.moduleSpecifier.text, /tauri|wasm-bindgen|node:|worker-host|remote-host|tauri-host/);
      }
    }
    if (ts.isCallExpression(node)) assert.ok(node.expression.kind !== ts.SyntaxKind.ImportKeyword, "共享策略不允许动态平台导入");
    ts.forEachChild(node, visit);
  }
  visit(source);
  for (const name of ["worker-host.ts", "tauri-host.ts", "remote-host.ts"]) {
    assert.match(await read("apps/web/src/host/" + name), /\brunObserverResync(?:<[^>]+>)?\(/, name);
  }
});

it("完整故障脚本检查入口只在验证构建暴露", { timeout: 10000 }, async () => {
  const source = await read("apps/web-wasm/src/market_runner.rs");
  assert.match(source, /#\[cfg\(feature = "verification-harness"\)\]\s*#\[wasm_bindgen\]\s*pub fn market_runner_contract_snapshot/);
  const producer = await read("scripts/wasm-package-capability.mjs");
  assert.doesNotMatch(producer, /"market_runner_contract_snapshot"/);
});

it("原始推进 ABI 不得绕过已启用的产品 authority", { timeout: 10000 }, async () => {
  const source = code(await read("apps/web-wasm/src/lib.rs"));
  for (const name of ["step", "end_civil_day", "flush_runtime_sync"]) {
    const start = source.indexOf(`pub fn ${name}(`);
    assert.ok(start >= 0, `缺少 ${name}`);
    const next = source.indexOf("#[wasm_bindgen]", start + 1);
    assert.match(source.slice(start, next < 0 ? undefined : next), /require_compatibility_entry\s*\(/, name);
  }
});

it("产品故障hook只在host-parity与debug共同启用，默认分发不暴露", { timeout: 10000 }, async () => {
  const routes = await read("apps/server/src/routes.rs");
  for (const name of ["api_authority_contract_setup", "api_authority_contract_action", "api_authority_contract_snapshot"]) {
    const index = routes.indexOf(`pub async fn ${name}`);
    assert.ok(index > 0, name);
    assert.match(routes.slice(Math.max(0, index - 150), index), /cfg\(all\(feature\s*=\s*"host-parity",\s*debug_assertions\)\)/, name);
  }
  const producer = await read("scripts/authority-product-build.mjs");
  assert.match(producer, /authority-frontend-build/);
  assert.match(producer, /authorityFrontendDistHash/);
  assert.match(producer, /debug_assertions/);
  const driver = await read("scripts/session-authority-products.mjs");
  assert.match(driver, /\/proc\/\$\{pid\}\/exe/);
  assert.match(driver, /runBoundedCommand/);
  assert.match(driver, /AUTHORITY_PRODUCT_SUPERVISOR/);
});
