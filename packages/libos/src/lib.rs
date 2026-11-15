//! Native/WASM 共用的执行设施。这里只处理时钟、线程、阻塞和死亡通知，
//! 不依赖撮合、游戏会话或具体宿主。

#![cfg_attr(target_arch = "wasm32", feature(stdarch_wasm_atomic_wait))]

pub mod allocator;
pub mod clock;
pub mod deferred_drop;
pub mod lifecycle;
pub mod thread_facade;
pub mod wait;

// Native 产品使用 std 线程，不把浏览器槽协议编入 Native 生产代码。
#[cfg(any(target_family = "wasm", test))]
mod facade_slot;
#[cfg(any(target_arch = "wasm32", test))]
mod wasm_tombstone;
