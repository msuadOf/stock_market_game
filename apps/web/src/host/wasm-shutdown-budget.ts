// 同一关停事务：归档退出之后仍须留出 Rust join、reclaimer 和终态 release 的预算。
export const WASM_SHUTDOWN_BUDGET_MS = 6_000;
export const CHECKPOINT_EXIT_BUDGET_MS = 5_000;
export const shutdownClock = (): number => performance.timeOrigin + performance.now();
