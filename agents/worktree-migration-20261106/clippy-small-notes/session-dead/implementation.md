# Session dead-code 精确 cfg 实施记录

基线：`50d493701aa0fbb7d62563855a84dc07c89c29b0`。

本批仅给以下测试代码使用的 helper 加 `#[cfg(test)]`：

- `session/account_book.rs`：`AccountBook::unlock_t1_positions`，仓库内唯一调用位于本文件测试模块。
- `session/attention.rs`：`NpcAttentionScheduler::iter`，仓库内调用仅位于本文件测试模块。
- `session/candles.rs`：`SessionCandleBook::record_trade_or_mark` 与 `GameSession::update_active_daily_candle`，仓库内调用仅位于测试代码（后者包含 session 与 pipeline 测试）。
- `session/exchange_calendar_model.rs`：`CalendarDayModel::start_date` 与 `StockCalendarClock::last_completed_minute_key`，仓库内调用仅位于本文件测试模块。

未改写任何交易逻辑、断言或运行时路径。只运行 `git diff --check` 和全仓 `rg` 调用点核查；按委派要求未运行 Cargo/target 命令。完整 Clippy 未复验。

未覆盖：允许范围内其他 dead-code 项仍需逐项区分运行时迁移候选与测试专用入口，尤其 `attention::pop_due_npc_ids`、`administrative_mirror::apply_administrative_operations`、`civil_information`、`reconcile_plan`、`failure`、`verify_metrics`、`persistence`、`protocol/civil`、`retail_lifecycle`、`self_views` 和 `plan_chain_candidates`。这些路径较复杂或缺乏明确仅测试调用证据，本批保留并交主线后续评估。明确排除目录/文件均未触碰。
