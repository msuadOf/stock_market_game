// 绿后负对照：故意把未知 StockId 查找改成首股，不能用于生产验收。
pub use engine::{calendar, observation, stock};
#[path = "/data1/baiyifan/workplace/stock_market_game/.tmp/mixed-calendar-c1a/unknown-negative.rs"]
mod exchange_calendar_model;
