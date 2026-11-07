import assert from "node:assert/strict";
import { test } from "node:test";
import { main, combineGateResults, LOCKED_WASM_BINDGEN, LOCKED_TOOLCHAIN, LOCKED_WASM_PACK, parseWasmBindgenPin } from "./verify-wasm-thread-combo.mjs";

// R3-2 组合验证入口·纯逻辑合同（重执行面由入口本体在长验收预算内承载）。

test("锁定矩阵常量与设计 §6.3 R3-1 一致", () => {
  assert.equal(LOCKED_TOOLCHAIN, "nightly-2026-09-05");
  assert.equal(LOCKED_WASM_PACK, "wasm-pack 0.13.1");
  assert.equal(LOCKED_WASM_BINDGEN, "0.2.93");
});

test("wasm-bindgen 锁版解析接受精确锁定并拒绝浮动/未锁形态", () => {
  assert.equal(parseWasmBindgenPin('wasm-bindgen = "=0.2.93"\n'), "0.2.93");
  // 浮动版本（^0.2 / "0.2"）与未声明都不满足锁定矩阵——返回 null/异值由门 1 判红。
  assert.equal(parseWasmBindgenPin('wasm-bindgen = "0.2"\n'), null);
  assert.equal(parseWasmBindgenPin('wasm-bindgen = "^0.2.93"\n'), null);
  assert.equal(parseWasmBindgenPin("serde = \"1\"\n"), null);
});

test("组合结果聚合：未登记红门使整体失败，登记红/跳过显式呈现", () => {
  const summary = combineGateResults([
    { gate: "lock-matrix", status: "green", detail: "" },
    { gate: "shared-memory-gate", status: "red", detail: "threads-xform 阻断（已登记）" },
    { gate: "browser-combo", status: "skipped", detail: "阶段 2" },
  ]);
  assert.equal(summary.green, false);
  assert.deepEqual(summary.red, ["shared-memory-gate"]);
  assert.deepEqual(summary.skipped, ["browser-combo"]);
  const allGreen = combineGateResults([{ gate: "lock-matrix", status: "green", detail: "" }]);
  assert.equal(allGreen.green, true);
});


test("组合长入口由进程外共享300000ms supervisor驱动且显式jobs", async () => {
  const calls = [];
  await main(["--jobs", "16"], { run: async (options) => { calls.push(options); }, env: {}, nodeExecutable: "/node" });
  assert.equal(calls.length, 1);
  assert.equal(calls[0].timeoutMs, 300000);
  assert.equal(calls[0].cleanupReserveMs, 1000);
  assert.equal(calls[0].command, "/node");
  assert.ok(calls[0].args.includes("--internal-worker"));
  assert.deepEqual(calls[0].args.slice(-2), ["--jobs", "16"]);
  assert.equal(calls[0].env.STOCK_GAME_WASM_COMBO_INTERNAL_WORKER, "1");
  await assert.rejects(main(["--internal-worker"], { env: {} }), /supervisor/);
});

test("浏览器组合入口按同一预算构建产品并执行真实 host 与故障注入", async () => {
  const { runBrowserCombo } = await import("./verify-wasm-thread-combo.mjs");
  const calls = [];
  await runBrowserCombo({ jobs: 8, startedAt: 1000, now: () => 2000, run: async (options) => calls.push(options), root: "/repo", prepare: async () => ({ env: {}, cleanup: async () => {} }), verifyShared: async (directory) => assert.equal(directory, "/repo/apps/web/wasm-pkg") });
  assert.equal(calls.length, 4);
  assert.match(calls[0].args[0], /frontend-build\.mjs$/);
  assert.match(calls[1].args[0], /browser-fixture\/batch\.sh$/);
  assert.match(calls[2].args[0], /browser-combo-dist\.mjs$/);
  assert.match(calls[3].args[0], /browser-runtime\.mjs$/);
  assert.ok(calls.every((call) => call.timeoutMs <= 298000));
  assert.equal(calls[0].env.CARGO_BUILD_JOBS, "8");
  assert.equal(calls[2].env.CARGO_BUILD_JOBS, "8");
});

test("组合 supervisor 在子进程失败后清理本批目录，重跑使用新目录", async () => {
  const cleaned = [];
  const paths = ["/tmp/combo-a", "/tmp/combo-b"];
  const seen = [];
  const dependencies = {
    env: {}, nodeExecutable: "/node", acquire: async () => async () => {},
    allocate: async () => paths.shift(),
    cleanup: async (directory) => cleaned.push(directory),
    run: async (options) => { seen.push(options); throw new Error("deadline"); },
  };
  await assert.rejects(main(["--browser"], dependencies), /deadline/);
  await assert.rejects(main(["--browser"], dependencies), /deadline/);
  assert.deepEqual(cleaned, ["/tmp/combo-a", "/tmp/combo-b"]);
  assert.deepEqual(seen.map(({ env }) => env.STOCK_GAME_WASM_COMBO_DIR), cleaned);
  assert.ok(seen.every(({ timeoutMs }) => timeoutMs === 290000));
});

test("真实进程超时后删除工作目录，随后可再次运行", { timeout: 10000 }, async () => {
  const { mkdtemp, access, readFile, rm } = await import("node:fs/promises");
  const { tmpdir } = await import("node:os");
  const path = await import("node:path");
  const { runBoundedCommand } = await import("./run-with-deadline.mjs");
  const directories = [];
  const evidence = await mkdtemp(path.join(tmpdir(), "combo-tree-test-"));
  let sequence = 0;
  const options = {
    env: process.env, acquire: async () => async () => {},
    allocate: async () => { const directory = await mkdtemp(path.join(tmpdir(), "combo-timeout-test-")); directories.push(directory); return directory; },
    run: (spec) => {
      const filename = path.join(evidence, String(sequence++));
      const source = "const {spawn}=require('node:child_process');const {writeFileSync}=require('node:fs');const child=spawn(process.execPath,['-e','setInterval(()=>{},1000)']);writeFileSync(process.argv[1],String(child.pid));setInterval(()=>{},1000)";
      return runBoundedCommand({ ...spec, command: process.execPath, args: ["-e", source, filename], timeoutMs: 500, cleanupReserveMs: 100 });
    },
  };
  await assert.rejects(main(["--browser"], options), /timed out|deadline/i);
  await assert.rejects(main(["--browser"], options), /timed out|deadline/i);
  for (const directory of directories) await assert.rejects(access(directory), { code: "ENOENT" });
  try {
    for (let index = 0; index < sequence; index++) {
      const pid = Number(await readFile(path.join(evidence, String(index)), "utf8"));
      assert.throws(() => process.kill(pid, 0), { code: "ESRCH" });
    }
  } finally { await rm(evidence, { recursive: true, force: true }); }
});
