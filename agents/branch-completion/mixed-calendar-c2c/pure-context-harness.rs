// engine rlib 仅提供既有 domain；本树 C1a/C2-C 纯模块按实际路径重新编译。
pub use engine::{calendar, observation, stock};
#[path = "../../../packages/engine/src/session/exchange_calendar_model.rs"]
mod exchange_calendar_model;
#[path = "../../../packages/engine/src/session/market_calendar_context.rs"]
mod market_calendar_context;
