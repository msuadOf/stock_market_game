// engine 时钟门面的 JS bootstrap（wasm 线程抽象层 R4a 阶段 1）。
//
// 设计锚：设计输入 §7.3.2（唯一时间 import 白名单）/ §8.3（timeOrigin 映射）。
//
// wasm-bindgen 0.2.93 对裸 extern `#[link(wasm_import_module = "env")]` 生成
// `import * as __wbg_star0 from 'env'` 并整体挂到 `imports['env']`（探针
// 实证，agents/wasm-thread-r4a/ 台账）——本文件经 vite resolve.alias 的
// 精确匹配 `/^env$/` 成为该 ES 模块，`engine_clock_now` 即 wasm import 表
// 中的 `env.engine_clock_now`（名字稳定，产物门按"恰 1 次"白名单断言）。
//
// 返回 A_w = performance.timeOrigin + performance.now()（绝对 ms，§8.3：
// 每 Worker 的 timeOrigin=其真实创建时刻，跨 Worker 同源单调钟可比）。
// Rust 侧（engine::clock）减共享内存中的协调者 timeOrigin 得统一时间线。
//
// 旧浏览器无 performance.timeOrigin → 降级 §7.3.3 旧式（per-Worker 原点，
// 绝对差值含启动偏差 Δ）并**显式登记**（degraded 导出 + 一次性 console.warn，
// 不静默——铁律二）。诊断级精度语义不变（协议等待不依赖绝对对齐）。

const hasTimeOrigin = typeof performance !== "undefined" && typeof performance.timeOrigin === "number";

export const engineClockDegradation: "none" | "no-time-origin" = hasTimeOrigin ? "none" : "no-time-origin";

if (!hasTimeOrigin) {
  console.warn(
    "[engine-clock] performance.timeOrigin 不可用：engine 时钟门面降级为 per-Worker 原点（§7.3.3 旧式公式），跨 Worker 绝对时戳含启动偏差 Δ，仅诊断级可比（engineClockDegradation=no-time-origin）。",
  );
}

export function engine_clock_now(): number {
  const origin = hasTimeOrigin ? performance.timeOrigin : 0;
  return origin + performance.now();
}

// 共享 memory 先绑定，实际 facade_slot_layout() 后装入；模块加载阶段无 memory 访问。
import { createFacadeThreadBinding } from "./facade-thread-binding.ts";

const threadBinding = createFacadeThreadBinding();
export const {
  bindFacadeThreadEnvironment,
  installFacadeSlotLayout,
  getArenaView,
  requireExitIntent,
  facade_futex_wait,
  facade_futex_notify,
  facade_spawn_notify,
  facade_spawn_notify_v2,
  facade_worker_exit_notify,
  facade_terminate_child,
} = threadBinding;
