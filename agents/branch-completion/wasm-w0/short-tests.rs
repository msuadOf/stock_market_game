// W0 短 fixture：直接编译纯 std 协议模块，避免把 engine 冷构建计入普通测试。
#![allow(dead_code)]
#[path = "../../../packages/engine/src/clock.rs"]
mod clock;
#[path = "../../../packages/engine/src/thread_facade.rs"]
mod thread_facade;
#[path = "../../../packages/engine/src/facade_slot/mod.rs"]
mod facade_slot;
#[path = "../../../packages/engine/src/runtime/wait_ledger_tests.rs"]
mod wait_ledger_tests;
