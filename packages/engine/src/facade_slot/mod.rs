//! wasm 线程门面·请求槽协议核心（R4a 阶段 2）。
//!
//! 设计锚：`.tmp/wasm-thread-abstraction-design-input.md` §12.1（单复合字
//! W=gen<<3|state、H1-H5、A1-A3）、§13.1-13.3（ANSWERING 写权、门链前提版
//! I4′/I5′、RA/RB 两类退休）、§14.1（五态闭合枚举 + 可用账口径恒等式）。
//!
//! 本模块平台无关（native 亦编译以承载确定性测试），native 生产路径
//! **零引用**——native 门面仍为 std::thread 纯 re-export（§0 硬约束，
//! asm 对照留证于 agents/wasm-thread-r4a/ 台账）。

pub mod arena;
pub mod codec;
pub mod platform;
pub mod roles;
#[cfg(test)]
pub(crate) mod test_supervisor;
#[cfg(test)]
mod tests;
