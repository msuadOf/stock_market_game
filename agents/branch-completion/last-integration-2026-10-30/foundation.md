# Integration 38 foundation 修复记录

- 日期：2026-10-30
- 范围：`packages/engine/tests/session_company_foundation.rs` 中 3 个存档测试。
- 原因：测试连续执行 35 次 `step()`，但 fixture 的 `ticks_per_day` 为 30；`step()` 只推进 market tick，跨日需显式完成 `end_civil_day()`。存档要求完整日结边界。
- 修复：三个测试将各自 setup 的 `ticks_per_day` 设为 35，保留 35 次 market tick 覆盖，并显式调用 `end_civil_day()` 后保存。既有逐位公司字段比较、公司键严格缺席断言及账户恢复比较均保留。
- 验证：未运行 Cargo/metadata/target/binary 命令，按集成调度要求留由唯一 integration 调度；已运行 `git diff --check`。
