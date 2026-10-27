// 真实 engine crate 提供所有依赖；直接编译待审的新模块，避免全库单测链接。
pub use engine::{calendar, observation, stock};
#[path = "../../../packages/engine/src/session/exchange_calendar_model.rs"]
mod exchange_calendar_model;
