# Session assembly 测试迁移记录

旧 `session_company_assembly.rs` 完整内容保存在 `legacy-session-assembly-reference.rs`，作为后续 `Simulation` 实现的历史验收参考。旧测试依赖的 Session `groups`、`company_operations` 与 consolidated `closing_registry` 已不属于当前 `Simple` 存档或 `SessionSetup` 契约；不能给 Simple 注入影子经营状态。`ConsolidatedSimple` 当前也没有可表达范围，查询会以 `ScopeNotRepresented` 拒绝，不构造假集团数据。

当前集成测试只验证真实 `GameSession` 路径：四个显式 `CompanyKind` 的发行人、上市股票和总股本映射；真实 `SimpleFinanceState` 中月度账簿结账、`ClosingEngine` 月报版本和公开独立报告；以及保存恢复后继续日历推进。财务状态从 `save.company_system` 的 serde 表示取出并反序列化公开 `SimpleFinanceState`，不拼装 JSON。

旧测试中的四行业经营流、集团抵销与少数股东、内部交易及集团报告断言不由这个 Simple 集成测试替代。未来 `Simulation` 获得正式 Session 能力后，应以归档文件中的全部断言为参照恢复相应验收。已有低层经营、合并报表、发布测试仍位于各自 suite；本次不声称旧 Session 集成覆盖已迁完。
