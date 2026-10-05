# Save 与经营事件测试迁移

本次将 Session 存档断言对齐到当前 `CompanySystem` / `SimpleFundamentals` 契约。
Session 档检查所选公司系统配置和 `advanced_through`。真实经营调度器的存档连续性
仍由独立 `CompanyOperations` fixture 检查：推进两个自然日，验证 pending dues 非空、
到期日不早于当前日、`next_expected_date`、serde 恢复后的 pending dues 和日期一致。
这些低层断言没有替代或影射成 `Simple` Session 状态。规模恢复 fixture 的月度前史
数值保持不变，只将配置字段迁移为 `prehistory_periods`。

旧 `GameSession` 注入 `ContractWon` 并验证公告事件的集成场景不属于当前 `Simple`
Session 契约。原测试文本归档于
[`reference/session-shock-announcement-legacy.rs.txt`](reference/session-shock-announcement-legacy.rs.txt)，
供未来明确授权的 `Simulation` 接线参考，不声称当前 `Simple` 能发布经营冲击公告。
当前通用 Session 披露事件仍由 scheduled report 用例验证。低层 `CompanyOperations`
fixture 独立保留冲击公告公开库和恢复测试；它不验证 Session 事件映射。

此前定向 `cargo check` 通过；本轮接收委托后不运行 Cargo 命令。完整的历史全量检查
仍被其他测试文件编译错误阻断，详见父任务交接；未执行完整回归。
