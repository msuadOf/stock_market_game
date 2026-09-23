//! 桌面端可执行入口（最小）。
//!
//! 防御式（铁律二）：`run()` 失败时 panic + 退出码，绝不静默吞错。
//! 实际逻辑（命令注册 / actor / 会话管理）在 lib，供集成测试黑盒。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    stock_market_game_lib::run();
}
