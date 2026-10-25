#!/usr/bin/env node
// R3-2 组合验证自动化入口（wasm 线程抽象层 R4a 阶段 1，设计 §6.3 R3-2）。
//
// 单脚本串起锁定矩阵 → 产物构建 → 产品门 → 共享内存门 →（可选）浏览器
// 组合冒烟，防"单件各自绿、组合红"。**工具链解阻塞批起 shared-memory-gate
// 为必须绿门**——阶段 1 登记的"threads-xform 与无 rayon 链不兼容"经源码
// 核清为误判（xform 输入符号全部由 rustc/lld/wasm-bindgen 运行时提供，
// `__stack_alloc` 是 xform 输出而非 rayon 注入；依据与逐轮红→绿证据见
// apps/web-wasm/.cargo/config.toml 登记与 .tmp/toolchain-unblock/
// symbol-responsibility.md）。仍显式跳过（阶段 2）——
//   R-worker 冒烟：G+N Worker spawn 依赖阶段 2 槽协议（设计 §12-14）；
//   判据③双实例最小门（同 module+memory 两 Worker 实例化/独立栈 TLS/
//   wait-notify/grow）已以最小复现产物验证 16/16 绿（.tmp/toolchain-unblock/
//   browser-probe/），产品产物接入随阶段 2 宿主装配落 --browser 臂。
// 其余门在本入口内必须真绿；任何未登记的红即失败。CI 常驻接入待全绿后挂。
//
// 用法：node scripts/verify-wasm-thread-combo.mjs [--browser] [--lock-only]
//   --browser：追加 dist 构建 + COOP/COEP 隔离服务 + Chromium 组合冒烟
//     （长验收，300000ms 共享 deadline，多核并行构建）。
//
// **browser-combo 转绿条件（阶段 2）**：G+N Worker spawn 断言（槽协议
//   §12-14 落地）+ ≥1 tick 全协议绿 + abort 注入走完 R2 协议；
//   shared-memory-gate 已随工具链解阻塞批转绿，不再是待转绿项。

import { execFileSync } from "node:child_process";
import { readFile } from "node:fs/promises";
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

async function main(withBrowser, lockOnly) {
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

  // 门 2：产物构建（注入锁定工具链；build-std/atomics flags 由
  // apps/web-wasm/.cargo/config.toml 按调用目录链生效）。
  results.push(await runGate("wasm-build", async () => {
    await runBoundedCommand({
      command: "wasm-pack",
      args: ["build", "apps/web-wasm", "--target", "web", "--release", "--out-dir", "wasm-pack-output/frontend/wasm/pkg-combo"],
      cwd: rootDirectory,
      env: { ...process.env, RUSTUP_TOOLCHAIN: LOCKED_TOOLCHAIN, CARGO_TARGET_DIR: path.join(rootDirectory, "target") },
      timeoutMs: 290000,
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

  // 门 5：浏览器组合冒烟（G+N Worker spawn + ≥1 tick + 跨源隔离）。
  // 阶段 1：未实施（依赖阶段 2 槽协议）——未加 --browser 时显式跳过登记。
  if (withBrowser) {
    results.push(await runGate("browser-combo", async () => {
      // 阶段 2 落点：frontend-build → COOP/COEP header 服务 → Chromium 断言
      // crossOriginIsolated / spawn G+N / ≥1 tick 全协议绿 / abort 走 R2。
      throw new Error("browser-combo 未实施（R4a 阶段 2：需 dist 构建 + 隔离服务 + spawn 断言；当前以门面 stub 显式拒绝为合同）");
    }));
  } else {
    results.push({ gate: "browser-combo", status: "skipped", detail: "未传 --browser（阶段 2 槽协议落地后接入 spawn/tick 冒烟）" });
  }

  const summary = combineGateResults(results);
  process.stdout.write(`\n[combo] ${JSON.stringify(summary, null, 2)}\n`);
  const registeredRed = new Set(["browser-combo"]);
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

if (process.argv[1] !== undefined && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await main(process.argv.includes("--browser"), process.argv.includes("--lock-only"));
}
