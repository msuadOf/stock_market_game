# 隐藏复核 batch 139（owner=4）

## 来源完整性与基线

按 `scan-plan.json` 连续读取三篇指定历史材料至 EOF。实测行数和 SHA-256 与计划完全一致，逐项见配套 JSON；基线 checkout 为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。当前源码检视以此 checkout 为准。worktree 原有审计材料处于修改/未跟踪状态；本任务没有更动它们、产品文件或 Git 状态。

## 历史材料与现状对照

- `areas-engine-foundation.md`、`managers-domain.md` 和 `modules-engine-foundation-01.md` 都把 Account 字段封装、恢复入口和策略替换迁移写成未实施候选；这不能代表当前基线状态。当前 `account.rs` 已将 `Account.id`、`AccountState` 及其字段封装，公开 `id/kind/cash/strategy/positions/position` 只读访问，恢复通过 crate-private `restore_balances/restore_strategy`，没有 `DerefMut`。当前恢复与 `NpcStrategyUpdate::Replace` 消费者使用这些入口；策略替换仍经过 `AccountBook::get_mut`。因此 A03 是历史上陈述的设计候选，不是当前仍待实施的缺口。
- 历史材料指出官方覆盖 `source_digest` 未进入 `CalendarPolicy::compute_content_digest`；基线源码仍如此：校验要求摘要非空，但摘要串只编码交易所、年份、出处 ID 和日期区间。仅修改来源摘要仍不会改变 policy digest。属仍可复现的源完整性缺口。
- 历史材料指出 `CivilInstant` derive `Deserialize` 可绕过 `new` 的秒域检查；基线仍 derive serde `Deserialize`，字段私有但反序列化可构造 `second_of_day >= 86400`。属仍可复现的输入不变量缺口。
- 历史材料指出 Account 买卖注释漏列过户费；基线仍存在：`apply_buy` 文档列成交额 + 佣金，代码还扣过户费；`apply_sell` 文档净入账列佣金与印花税，代码还扣过户费。属于注释与实现不一致，不改变费率结论。
- `GameConfig` 当前仍是 public 字段、可 serde 反序列化的配置聚合；`new/validate` 是显式校验边界，不能描述为对象始终有效。`lot_size` 与费用参数是现有模拟配置；本审查没有核验或改动大 A 规则及费率依据。

## 调用与消费边界

当前 Account 消费方包括 session 初始化/恢复、NPC 策略状态投影、结算 pipeline、snapshot/hash/decision/view 投影，以及 account/session 集成测试。读取路径主要走 getter；恢复余额路径在 session 恢复流程中使用受限入口。`apply_buy/apply_sell` 由账户结算及测试覆盖。CalendarPolicy digest 由 policy 构造/校验消费，CivilInstant 被日历/session 时间相关逻辑消费；这些序列化/恢复边界需覆盖反序列化秒域。此处为静态源码定位，不等同完整动态可达性验证。

## 结论与范围限制

三篇来源是 OOP 调查的历史候选材料，不含已批准功能承诺，也没有声称候选已实现。当前基线显示 Account 封装候选已落实；摘要绑定、CivilInstant 反序列化校验及费用注释问题仍存在，属源代码风险线索，应由 caller 结合总账决定是否立项。本批没有实施修复、没有运行测试/构建，也没有重新核验官方交易制度来源；不据此宣称领域规则或行为验收通过。
