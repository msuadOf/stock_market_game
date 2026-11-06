# Attention discovery civil fixture 修复记录（2026-10-30）

## 范围与基线

- 独占 worktree：`.worktree/attention-civil-fixture-2026-10-30`
- 基线 commit：`02f9887ff4cc707faf6e754f65baf0398508a938`（development-main-2026-10-29）
- 仅改测试夹具 `packages/engine/tests/attention_discovery/failures/discovery.rs`；未触及根工作树、生产代码、Cargo 文件或 target。
- 依据：`docs/decisions/0017-escrow-parallel-tick.md` 规定自然日更新走独立 `end_civil_day`，不伪造 tick；`docs/decisions/0025-day-end-only-persistence.md` 规定交易日收盘与休市自然日均需日结；`docs/simulation-calendar.md` §4 规定交易日/休市日共享 civil 时钟语义。`GameSession::end_civil_day` 的 API 注释也要求完成 `ticks_per_day` 个 step 后显式日结，休市日直接调用。

## 修改

`session_bytes()` 仍执行原有 150 次 `step()`，不缩短采样或改变市场 tick 密度。每 10 次（fixture `ticks_per_day`）step 后显式 `end_civil_day()`；若 civil 时钟落在 `CivilPhase::ClosedDay`，继续逐自然日显式日结，直到下一开市日再进行市场 step。事件数组和完整 `session.save()` 序列化后继续逐字节比较，未清除字段或弱化断言。

测试真正约束是：只变私有经营事实后，公开发现权重、固定 RNG 的候选序列和完整会话事件/存档字节保持相同。原 150 tick 长模拟是订单/事件/存档投影，修改只补足新的“市场 step 与 civil 日终分开”合同，不改该 oracle。

## 验证边界

- 已执行 `rustup run 1.96.1 rustfmt --edition 2021 packages/engine/tests/attention_discovery/failures/discovery.rs`（报告 rustfmt 1.9.0-stable）与该文件 `git diff --check`，均成功。
- 按任务要求未运行 Cargo、Node 或 target 测试；已知基线回归红结果不作为本次 green 证据。
- 冻结后须由未实施者独立复核完整 diff；本记录不替代该门禁。
