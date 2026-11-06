# 日历夹具回归修正（2026-10-30）

基于 `a1fc292c`，仅修正两条测试夹具的自然日/交易日对齐，不改生产逻辑或市场日历规则。

- `accepted_retail_observation_dates_a_fixture_buy_and_decays_only_its_influence`：按 `CivilClock::completed_trading_sessions_expected()` 与 `CivilPhase` 逐自然日推进到第 19、20 个交易会话；推进后清空 shadow 的派生日历缓存，再校验 `shared_sessions_before_date` 分别为 19、20。原 `failure_influence` 与体验历史断言保留。
- `full_trading_day_and_day_boundary_keep_one_production_per_tick`：完整交易日后执行 `end_civil_day()`，跨过休市日直至下个交易日；每次自然日日结前后 `attempt_seq` 不变，后续首 tick 的原计数断言保留。

验证：按任务限制未运行 Cargo、metadata、rustc、Node 或 target 命令；仅运行 `rustfmt --check` 与 `git diff --check`。独立语义复核由任务发起者完成。
