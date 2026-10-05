# CompanySystem 场景测试迁移记录

## 范围

迁移 `packages/engine/tests/company_scenarios/` 下消费者测试，使场景断言读取当前 `CompanySystem` 存档事实，不扩充 `GameSession` 的旧经营状态兼容。

## 改动

- `main.rs` 显式导入 `WithinKindDistribution`，供现有简单公司 fixture 和市场分配配置使用。
- `controlled.rs` 的发行股数来源改为存档 `company_system.issuers()` 中的发行人规格；观察上下文仍基于真实公开报告与个人信息状态。
- `lifecycle.rs` 闭市日和年末推进断言改为检查存档 `company_system.advanced_through()`；年度公开报告仍经真实披露调度查询，并保留报告期间、资产正值、发布时间及发布游标断言。
- 年末 close 版本数量通过真实 `SaveSlot.company_system` 序列化值提取其 `SimpleFinanceState`，再读取该实例 `ClosingEngine.versions`；保留结算前后年度版本数增长约束。没有给 Session 增加旧经营状态。
- 旧 Session scheduler 的待办非空断言移至 `company_operations` 年末低层 fixture，以独立 `CompanyOperations` 执行 2030-12-31 后检查仍有严格晚于结算日的待办。旧耦合断言原文保存在 `legacy-session-annual-close-reference.md`，供审阅迁移映射。

## 验证与边界

- 已静态搜索 `company_scenarios`，无剩余 `company_operations` 或 `closing_registry` 字段访问；close 查询依据是真实序列化保存状态，不是合成财务 JSON。
- 按任务要求未运行 Cargo 检查或测试；由根任务统一验证。
- 本工作记录不代表独立复核完成；完整 diff 复核由根任务安排非作者 Luna agent。
