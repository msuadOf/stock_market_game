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

// ---------------------------------------------------------------------------
// wasm 线程门面槽协议 env 面（R4a 阶段 2，设计 §12-14）。
//
// 这些名字是 Rust `facade_slot::platform` 的 `#[link(wasm_import_module =
// "env")]` extern。**必须在本模块导出**——否则 wasm 实例化期即 link error
// （import 解析发生在实例化，不在调用）。当前宿主尚未装配 supervisor/子 Worker，槽协议不可达，函数体一律**显式抛出**，不静默
// no-op 伪装成功（铁律二）。真实实现由 W1 宿主 facade-supervisor 接线承接。
// ---------------------------------------------------------------------------

function facadeUnsupported(name: string): never {
  throw new Error(
    `facade_thread_env.${name}：当前宿主尚未装配 supervisor/子 Worker/bootstrap，` +
      `槽协议路径不可达——这是构建形态违约而非运行时容错（设计 §12-14，阶段边界见 agents/wasm-thread-r4a/）`,
  );
}

/** futex 等待（Atomics.wait 等价）：返回 0=醒 / 1=期望不符 / 2=超时。 */
export function facade_futex_wait(_wordAddr: number, _expected: number, _timeoutMs: number): number {
  return facadeUnsupported("facade_futex_wait");
}

/** futex_wake_all（Atomics.notify 全体）。 */
export function facade_futex_notify(_wordAddr: number): void {
  facadeUnsupported("facade_futex_notify");
}

/** H5：REQ 发布后的 facadeSpawn 消息。 */
export function facade_spawn_notify(_slot: number, _gen: number, _namePtr: number, _nameLen: number): void {
  facadeUnsupported("facade_spawn_notify");
}

/** W3：workerExit（宿主承接 W4 rust_done + W5 self.close）。 */
export function facade_worker_exit_notify(_workerId: number, _slot: number, _gen: number): void {
  facadeUnsupported("facade_worker_exit_notify");
}

/** 迟到构造子 terminate（幂等卫生义务）。 */
export function facade_terminate_child(_workerId: number): void {
  facadeUnsupported("facade_terminate_child");
}
