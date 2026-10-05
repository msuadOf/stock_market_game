# CompanyOperations 测试迁移记录

本轮仅迁移 `packages/engine/tests/company_operations/industry_sessions.rs`、
`income_tax.rs`、`session_boundary.rs` 及套件模块入口所需内容，不改生产代码。

## 所有权边界

- 行业 shock、收入税、TaxPayment、税款欠付、税务来源校验、payment failure 历史归
  `CompanyOperations` 与对应 IndustryBooks。恢复断言直接 serde 恢复
  `CompanyOperations`；行业公告直接调用 `DisclosureDispatch::run_day_end`、
  `CivilClock`、`ClosingEngine` 和 `PublicLibrary`，不经过旧 SaveSlot 接线。
- 当前 `Simple` `GameSession` 只拥有 `CompanySystem::Simple`。它不包含
  `company_operations`、旧 `groups` 或旧 `ops_wiring`。旧 Session 经营日结、公司账簿税务
  回滚及旧公开存档恢复不是现行 Session 契约；对应 Ops 规则由独立低层 fixture 验证。
- 当前 Simple Session 的本人交易、NPC 与 CivilClock 跨边界恢复属于 Session 的真实交易及
  存档测试，不应由本套件的经营 Ops 测试代替。

## 覆盖与限制

行业公告测试通过真实 `PublicLibrary` 保存公告，并检查公司、适用行业、发生日、发布时点、
非经营类 shock 排除和 active shock 的恢复结果。税务测试仍检查 assessment、分录种类、
`222104` 欠缴余额、支付金额、其他公司的推进、恢复后重试及税务 restatement 来源拒绝。

本迁移不声称 Simple Session 与经营 Ops 有集成关系，也没有复刻已删除的跨层调用。原依赖
旧 Session 后端的测试需以其旧原文作为历史证据留存，不能算作现行行为通过证明。

旧 `CompanyOperations` payment history 的 Session runtime calendar range 校验不属于 Ops 的
恢复职责；Ops 仅校验历史记录内部的一致性。本目录单独用 `TradingCalendar::current_default_calendar`
验证运行时开局日期窗口，不能据此声称 payment-history 存档仍验证 Session calendar policy。

当前根执行角色尚未运行本批编译或测试；迁移结论不包含通过声明。旧 Session 年末失败回滚
不能由 Ops 日期错误代替；税收账簿和固定资产的年末成功结果保留在独立 Ops case。
