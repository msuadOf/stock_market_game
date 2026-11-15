//! Native 端运行与 Browser 验证入口同源的 API 合同。
#![cfg(not(target_family = "wasm"))]
#[path = "support/wait_api.rs"]
mod shared;

#[test]
fn 时刻算术与错误边界() {
    shared::clock_arithmetic();
}

#[test]
fn 协调者队列超时与断连() {
    shared::coordinator_queue();
}

#[test]
fn worker_按序排空并区分硬停() {
    shared::worker_queue();
}

#[test]
fn 两组关停哨兵幂等且不跳过仍存活接收端() {
    shared::shutdown_wake();
}
