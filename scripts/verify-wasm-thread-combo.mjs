#!/usr/bin/env node
// WASM 组合验证入口。--browser 使用正式产品 build 和真实 host/tick，
// 同时以独立 verification-harness 构建执行 Worker 故障注入。
// 两种产物的证据分别报告；故障注入不代表产品包的同链注入。
// 长验收包含构建、浏览器与清理，由外部 supervisor 共享 300000ms。

import { execFileSync } from "node:child_process";
import { availableParallelism } from "node:os";
import { mkdir, mkdtemp, readFile, readlink, rm, unlink } from "node:fs/promises";
import { tmpdir } from "node:os";
import { preparePinnedSysroot } from "./wasm-panic/prepare-sysroot.mjs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { verifySharedMemoryArtifact, verifyWebReleaseWasm } from "./check-web-release-wasm.mjs";
import { runBoundedCommand } from "./run-with-deadline.mjs";

const rootDirectory = path.resolve(fileURLToPath(import.meta.url), "..", "..");
export const LOCKED_TOOLCHAIN = "nightly-2026-09-05";
export const LOCKED_WASM_PACK = "wasm-pack 0.13.1";
export const LOCKED_WASM_BINDGEN = "0.2.93";
const wasmPackage = path.join(rootDirectory, "apps/web-wasm/wasm-pack-output/frontend/wasm/pkg-combo");

export function parseWasmBindgenPin(cargoToml) {
  const match = cargoToml.match(/^wasm-bindgen\s*=\s*"=(\d+\.\d+\.\d+[^"]*)"/m);
  return match === null ? null : match[1];
}

async function runGate(name, body) {
  try {
    const detail = await body();
    return { gate: name, status: "green", detail: detail ?? "" };
  } catch (error) {
    return { gate: name, status: "red", detail: error instanceof Error ? `${error.name}: ${error.message}` : String(error) };
  }
}

export function combineGateResults(results) {
  const red = results.filter((entry) => entry.status === "red");
  const skipped = results.filter((entry) => entry.status === "skipped");
  return {
    green: red.length === 0 && skipped.length === 0,
    red: red.map((entry) => entry.gate),
    skipped: skipped.map((entry) => entry.gate),
    results,
  };
}

export async function runBrowserCombo({ jobs, startedAt, now = Date.now, run = runBoundedCommand, root = rootDirectory, prepare = prepareBrowserFailureFixture, verifyShared = verifySharedMemoryArtifact }) {
  const work = await prepare(root, process.env.STOCK_GAME_WASM_COMBO_DIR);
  try {
    const controller = new AbortController();
    const command = async (spec, env = {}) => {
      const remaining = Math.floor(startedAt + 299000 - now());
      if (remaining <= 1000) throw new Error("browser-combo 共享 deadline 已耗尽");
      await run({ ...spec, cwd: root, env: { ...process.env, ...env, CARGO_BUILD_JOBS: String(jobs) }, timeoutMs: remaining, cleanupReserveMs: 1000, signal: controller.signal }, (chunk) => process.stdout.write(chunk));
    };
    const guard = async (body) => { try { await body(); } catch (error) { controller.abort(error); throw error; } };
    const outcomes = await Promise.allSettled([
      guard(async () => {
        await command({ command: process.execPath, args: [path.join(root, "scripts/frontend-build.mjs"), "--jobs", String(jobs), "--package-manager", "pnpm"] });
        await verifyShared(path.join(root, "apps/web/wasm-pkg"));
        await command({ command: process.execPath, args: [path.join(root, "scripts/browser-combo-dist.mjs")] });
        await command({ command: process.execPath, args: [path.join(root, "agents/wasm-sync-parity/browser-runtime.mjs")] });
      }),
      guard(() => command({ command: "sh", args: [path.join(root, "agents/wasm-tombstone-followup/browser-fixture/batch.sh")] }, work.env)),
    ]);
    const failures = outcomes.filter((outcome) => outcome.status === "rejected").map((outcome) => outcome.reason);
    if (failures.length) throw new AggregateError(failures, `浏览器组合验收失败：${failures.map((error) => error instanceof Error ? error.message : String(error)).join("；")}`);
  } finally { await work.cleanup(); }
  return "产品包真实 host/tick 与独立 verification-harness 故障注入均通过；故障注入不证明产品包同链注入";
}

async function prepareBrowserFailureFixture(root, directory) {
  if (!directory || !path.isAbsolute(directory)) throw new Error("browser-combo 缺少 supervisor 工作目录");
  try {
    const rustc = execFileSync("rustup", ["which", "--toolchain", LOCKED_TOOLCHAIN, "rustc"], { encoding: "utf8", timeout: 5000 }).trim();
    const sysroot = path.join(directory, "sysroot");
    await preparePinnedSysroot(rustc, sysroot);
    const target = path.join(root, ".tmp/wasm-thread-combo-harness-target");
    await mkdir(target, { recursive: true });
    return { env: { WASM_PANIC_SYSROOT: sysroot, CARGO_TARGET_DIR: target, WASM_PKG: path.join(directory, "pkg") }, cleanup: async () => {} };
  } catch (error) { throw error; }
}

async function runWorker(withBrowser, lockOnly, jobs) {
  const startedAt = Date.now();
  process.stdout.write(`[combo/resources] ${JSON.stringify({jobs, available: availableParallelism(), sharedDeadlineMs:300000})}\n`);
  const results = [];

  // 门 1：锁定矩阵（rustc 工具链 + rust-src + wasm32 target + wasm-pack 版本
  // + wasm-bindgen CLI 与 crate 同版）。--lock-only：只跑本门（CI 常驻断言
  // R3-1 的"wasm-bindgen 版本一致性"行，构建/组合门仍由本地长验收承载）。
  results.push(await runGate("lock-matrix", async () => {
    const toolchains = execFileSync("rustup", ["toolchain", "list"], { encoding: "utf8", timeout: 5000 });
    if (!toolchains.includes(LOCKED_TOOLCHAIN)) throw new Error(`锁定工具链 ${LOCKED_TOOLCHAIN} 未安装（rustup toolchain list）`);
    const components = execFileSync("rustup", ["component", "list", "--installed", "--toolchain", LOCKED_TOOLCHAIN], { encoding: "utf8", timeout: 5000 });
    for (const required of ["rust-src", "rust-std-wasm32-unknown-unknown"]) {
      if (!components.split(/\r?\n/).some((line) => line.trim() === required)) throw new Error(`${LOCKED_TOOLCHAIN} 缺组件 ${required}`);
    }
    const wasmPackVersion = execFileSync("wasm-pack", ["--version"], { encoding: "utf8", timeout: 5000 }).trim();
    if (wasmPackVersion !== LOCKED_WASM_PACK) throw new Error(`wasm-pack 版本漂移：期望 ${LOCKED_WASM_PACK}，实际 ${wasmPackVersion}`);
    const cargoToml = await readFile(path.join(rootDirectory, "apps/web-wasm/Cargo.toml"), "utf8");
    const pin = parseWasmBindgenPin(cargoToml);
    if (pin !== LOCKED_WASM_BINDGEN) throw new Error(`web-wasm 的 wasm-bindgen 未按锁定矩阵精确锁版 =${LOCKED_WASM_BINDGEN}（实际 ${pin ?? "未解析"}）`);
    return `${LOCKED_TOOLCHAIN} + rust-src + wasm32 + ${wasmPackVersion} + wasm-bindgen ${pin}`;
  }));
  if (lockOnly) {
    const lockSummary = combineGateResults(results);
    process.stdout.write(`[combo/lock-only] ${JSON.stringify(lockSummary)}\n`);
    process.exitCode = lockSummary.green ? 0 : 2;
    return lockSummary;
  }

  if (withBrowser) {
    results.push(await runGate("browser-combo", () => runBrowserCombo({ jobs, startedAt })));
    const summary = combineGateResults(results);
    process.stdout.write(`[combo] ${JSON.stringify(summary)}\n`);
    if (!summary.green) process.exitCode = 2;
    return summary;
  }

  // 门 2：产物构建（注入锁定工具链；build-std/atomics flags 由
  // apps/web-wasm/.cargo/config.toml 按调用目录链生效）。
  results.push(await runGate("wasm-build", async () => {
    await runBoundedCommand({
      command: "wasm-pack",
      args: ["build", "apps/web-wasm", "--target", "web", "--release", "--out-dir", "wasm-pack-output/frontend/wasm/pkg-combo"],
      cwd: rootDirectory,
      env: { ...process.env, RUSTUP_TOOLCHAIN: LOCKED_TOOLCHAIN, CARGO_BUILD_JOBS: String(jobs), CARGO_TARGET_DIR: process.env.CARGO_TARGET_DIR ?? path.join(rootDirectory, ".tmp/wasm-thread-combo-target") },
      timeoutMs: Math.max(1, Math.min(290000, startedAt + 299000 - Date.now())),
      cleanupReserveMs: 1000,
    }, (chunk) => process.stdout.write(`[combo/wasm-build] ${chunk}`));
    return "wasm-pack build 完成（nightly + build-std + atomics）";
  }));

  // 门 3：产品门（时间 import 白名单 + 原子码位 tripwire——必须绿）。
  results.push(await runGate("product-gate", async () => {
    const files = await verifyWebReleaseWasm(wasmPackage);
    return `产品门绿：${files.join(", ")}`;
  }));

  // 门 4：共享内存门（必须绿——import+shared+明确 maximum+胶水 SAB 引导；
  // 阻断已解除，登记见文件头）。
  results.push(await runGate("shared-memory-gate", async () => {
    const files = await verifySharedMemoryArtifact(wasmPackage);
    return `共享内存门绿：${files.join(", ")}`;
  }));

  results.push({ gate: "browser-combo", status: "skipped", detail: "未传 --browser，未执行真实浏览器验收" });

  const summary = combineGateResults(results);
  process.stdout.write(`\n[combo] ${JSON.stringify(summary, null, 2)}\n`);
  const registeredRed = new Set();
  const unregisteredRed = summary.red.filter((gate) => !registeredRed.has(gate));
  if (unregisteredRed.length > 0) {
    process.stderr.write(`[combo] 未登记的红门：${unregisteredRed.join(", ")}——组合验证失败（显式红之外不允许新红）\n`);
    process.exitCode = 2;
  } else if (summary.red.length > 0) {
    process.stderr.write(`[combo] 显式红（已登记）：${summary.red.join(", ")}——阶段边界如实呈现，非静默\n`);
    process.exitCode = 1;
  } else if (summary.skipped.length > 0) {
    // 无红门但有显式跳过门：如实呈现"必绿门全绿 + 跳过明细"，不打印笼统"全绿"
    //（复核建议项 a：跳过在场时整体 green=false，措辞需与 JSON 一致）。
    process.stdout.write(`[combo] 必绿门全绿；显式跳过（已登记）：${summary.skipped.join(", ")}\n`);
  } else {
    process.stdout.write("[combo] 全绿\n");
  }
  return summary;
}

/** 正式长入口由进程外supervisor覆盖矩阵、构建、检查与清理的共享总时限。 */
export async function main(argv, { run = runBoundedCommand, env = process.env, nodeExecutable = process.execPath, allocate = () => mkdtemp(path.join(tmpdir(), "stock-combo-")), cleanup = cleanupBrowserWorkspace, acquire = acquireBrowserLock } = {}) {
  const args = [...argv];
  const jobsIndex = args.indexOf("--jobs");
  let jobs = availableParallelism();
  if (jobsIndex !== -1) {
    const raw = args[jobsIndex + 1];
    if (!/^[1-9]\d*$/.test(raw ?? "") || !Number.isSafeInteger(Number(raw))) throw new Error("--jobs需要正整数");
    jobs = Number(raw); args.splice(jobsIndex, 2);
  }
  for (const arg of args) if (!["--lock-only", "--browser", "--internal-worker"].includes(arg)) throw new Error(`未知组合参数${arg}`);
  if (args.includes("--internal-worker")) {
    if (env.STOCK_GAME_WASM_COMBO_INTERNAL_WORKER !== "1") throw new Error("组合worker必须由进程外supervisor启动");
    return runWorker(args.includes("--browser"), args.includes("--lock-only"), jobs);
  }
  const lockOnly = args.includes("--lock-only");
  if (!args.includes("--browser")) return run({
    command: nodeExecutable,
    args: [fileURLToPath(import.meta.url), "--internal-worker", ...args, "--jobs", String(jobs)],
    cwd: rootDirectory,
    env: { ...env, STOCK_GAME_WASM_COMBO_INTERNAL_WORKER: "1" },
    timeoutMs: lockOnly ? 10000 : 300000,
    cleanupReserveMs: lockOnly ? 200 : 1000,
  }, (chunk) => process.stdout.write(chunk));
  const release = await acquire();
  let directory;
  try {
    directory = await allocate();
    return await run({ command: nodeExecutable, args: [fileURLToPath(import.meta.url), "--internal-worker", ...args, "--jobs", String(jobs)], cwd: rootDirectory,
      env: { ...env, TMPDIR: directory, STOCK_GAME_WASM_COMBO_DIR: directory, STOCK_GAME_WASM_COMBO_INTERNAL_WORKER: "1" }, timeoutMs: 290000, cleanupReserveMs: 1000 }, (chunk) => process.stdout.write(chunk));
  } finally {
    try { if (directory !== undefined) await cleanup(directory); } finally { await release(); }
  }
}

async function acquireBrowserLock() {
  const parent = path.join(rootDirectory, ".tmp");
  const lock = path.join(parent, "wasm-thread-combo.lock");
  await mkdir(parent, { recursive: true });
  try { await mkdir(lock); } catch (error) {
    if (error.code === "EEXIST") throw new Error(`浏览器组合验收已在运行或残留锁：${lock}`);
    throw error;
  }
  return async () => { const { rmdir } = await import("node:fs/promises"); await rmdir(lock); };
}

async function cleanupBrowserWorkspace(directory) {
  const link = path.join(rootDirectory, "agents/wasm-tombstone-followup/browser-fixture/pkg");
  try {
    const target = await readlink(link);
    if (target !== path.join(directory, "pkg")) throw new Error(`fixture pkg 指向其他位置，拒绝清理：${target}`);
    await unlink(link);
  } catch (error) { if (error.code !== "ENOENT") throw error; }
  await rm(directory, { recursive: true, force: true });
}

if (process.argv[1] !== undefined && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const args = process.argv.slice(2);
    if (args.includes("--browser") && process.env.STOCK_GAME_WASM_COMBO_SUPERVISED !== "1") {
      await runBoundedCommand({ command: process.execPath,
        args: [path.join(rootDirectory, "scripts/run-long-validation.mjs"), "300000", "--", process.execPath, fileURLToPath(import.meta.url), ...args],
        cwd: rootDirectory, env: { ...process.env, STOCK_GAME_WASM_COMBO_SUPERVISED: "1" }, timeoutMs: 300000, cleanupReserveMs: 1000 }, (chunk) => process.stdout.write(chunk));
    } else await main(args);
  } catch (error) { process.stderr.write(`${error.message}\n`); process.exitCode = 1; }
}
