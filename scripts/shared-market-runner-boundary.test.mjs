import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { it } from "node:test";

const read = (name) => readFile(new URL("../" + name, import.meta.url), "utf8");

it("三种产品的推进入口均调用同一 Rust runner", { timeout: 10000 }, async () => {
  const paths = [
    "apps/server/src/actor.rs",
    "apps/desktop/src-tauri/src/actor.rs",
    "apps/web-wasm/src/market_runner.rs",
  ];
  for (const path of paths) {
    const source = await read(path);
    assert.match(source, /\brun_cycle\s*\(/, path);
    assert.match(source, /\bPacing\b/, path);
    assert.doesNotMatch(source, /\bfn\s+fresh_interval\s*\(/, path);
  }
});

it("共享业务循环和发布策略不识别构建目标", { timeout: 10000 }, async () => {
  for (const path of ["packages/engine/src/session/runner.rs", "packages/engine/src/session/publication.rs"]) {
    const source = await read(path);
    assert.doesNotMatch(source, /\b(?:target_arch|target_family|target_os|window\.__TAURI__|is_platform)\b/, path);
    assert.doesNotMatch(source, /\b(?:tokio|web_sys|js_sys|tauri|axum)::/, path);
  }
});

it("浏览器权威 Worker 不以 UI 数据确认驱动自动模拟", { timeout: 10000 }, async () => {
  const source = await read("apps/web/src/host/wasm-worker.ts");
  assert.doesNotMatch(source, /\bnew\s+WasmTickLoop\b|case\s+["']uiFrame["']\s*:/);
  const control = await read("apps/web/src/host/engine-control-worker.ts");
  assert.match(control, /wasm-publisher-worker\.ts/);
  const publisher = await read("apps/web/src/host/wasm-publisher-worker.ts");
  assert.doesNotMatch(publisher, /\b(?:run_cycle|market_runner_turn|step_frame)\s*\(/);
});

it("三端完成同一调度阶段且手工操作不重写日界策略", { timeout: 10000 }, async () => {
  for (const path of [
    "apps/server/src/actor.rs",
    "apps/desktop/src-tauri/src/actor.rs",
    "apps/web-wasm/src/market_runner.rs",
  ]) {
    const source = await read(path);
    assert.match(source, /\bapply_cycle_pacing\(/, path);
    assert.match(source, /\bmanual_tick\(/, path);
    assert.match(source, /\bmanual_civil\(/, path);
    assert.doesNotMatch(source, /\b(?:target_arch|target_family|target_os)\b|\bpause_preferences\.pauses\(/, path);
    assert.doesNotMatch(source, /\bfn\s+stop_cycle_with_unpublished_frames\(/, path);
  }
});

it("日终快槽由权威交接给独立归档任务，不由观察端写入", { timeout: 10000 }, async () => {
  const app = await read("apps/web/src/App.tsx");
  assert.doesNotMatch(app, /getBrowserSaveRepository\(\)\.save\(/);
  assert.match(app, /mirrorAuthorizedCheckpoint/);
  for (const [path, sink] of [
    ["apps/server/src/actor.rs", /\blane\.stage\(/],
    ["apps/desktop/src-tauri/src/actor.rs", /\blane\.stage\(/],
    ["apps/web-wasm/src/market_runner/checkpoint.rs", /\bself\.tx\.try_send\(/],
  ]) {
    const source = await read(path);
    assert.match(source, /\bCapturedCheckpoint::take\(/, path);
    assert.match(source, sink, path);
  }
  const archiveWorker = await read("apps/web/src/host/wasm-checkpoint-worker.ts");
  assert.match(archiveWorker, /market_archive_take/);
  assert.doesNotMatch(archiveWorker, /ackPublisher|market_publisher_take/);
});

it("三种活跃传输共同运行重同步事务", { timeout: 10000 }, async () => {
  for (const path of ["worker-host.ts", "tauri-host.ts", "remote-host.ts"]) {
    const source = await read("apps/web/src/host/" + path);
    assert.match(source, /\brunObserverResync(?:<[^>]+>)?\(/, path);
  }
});
