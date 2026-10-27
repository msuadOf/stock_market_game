// 真实 engine rlib 只提供既有 domain 依赖；C1a 与 C2-A 由本树源码重新编译。
pub use engine::{calendar, observation, stock};
#[path = "../../../packages/engine/src/session/exchange_calendar_model.rs"]
mod exchange_calendar_model;
#[path = "../../../packages/engine/src/session/market_calendar_context.rs"]
mod market_calendar_context;
