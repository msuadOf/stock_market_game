# 批次209 独立复核

审查基线：`43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。三份来源均为历史 OOP review 记录，不代表当前实现状态。本轮只读复核来源和相关现行 owner/caller；未运行测试、构建或 Git 命令，未改产品文件。

## 来源读取与完整性

| 来源 | SHA-256 | 计划/实读行数 | 读取状态 |
|---|---|---:|---|
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/history/engine-strategy-01-summary-before-caller.md` | `7f3106d42ecb842e2409a601de9eea4f817bad63c390d9e5cf4dad86b805c537` | 21 / 21 | 连续读至 EOF，校验匹配 |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/history/engine-strategy-02-before-manager.md` | `a26eb3c74f8d002ecd184e0a0a4a3d53ea921055c26657e831ea306a7a79defe` | 73 / 73 | 连续读至 EOF，校验匹配 |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/history/engine-strategy-03-before-manager.md` | `515930e47323b6c2b67ac22ba0b5839b4e64849df5994569c88bc55926443eff` | 28 / 28 | 连续读至 EOF，校验匹配 |

## 历史结论与当前 owner/caller 核对

- `engine-strategy-01` 的历史摘要确认 A01/18 fields 与两份 binding 一致，重点是 `TradingPlan` ownership、直接 apply 与 staged atomic batch 边界。该材料明确声明仅做文档一致性检查，不是源码实施或验证结论。当前这一批未独立复核其完整领域行为，不将摘要当作新产品缺陷证据。
- `engine-strategy-02` 的历史终审结论为未通过，阻断点是调查材料没有逐文件核销调用关系、可见符号和具体验证位置；其文中同时判断当时 23 个源码文件均 retain，且 BeliefBook restore 身份组合疑点属于 behavior-fix candidate 而非 OOP action。这是历史审计交付质量问题，不是当前代码行为候选。
- `engine-strategy-03` 的历史结论为通过：`BeliefInstitutionStrategy` 保有逐机构参数，`ZiNoiseStrategy` 的策略参数投影属于现有 owner，策略 kernel/意图转换保持纯函数边界。
- 现行实现中 `ZiNoiseStrategy` 在 `packages/engine/src/strategy/zi_noise.rs:16` 定义，私有 `strategy_data` 在 `:42`，`Strategy` 实现在 `:120`，完整参数投影测试位于 `:296`。该 helper 已存在，故历史记录中的抽取建议不是待修复候选。
- `BeliefInstitutionStrategy` 现位于 `packages/engine/src/strategy/value.rs:45`，实现位于 `:123`。当前 `BeliefBook::apply_cause` 在 `packages/engine/src/strategy/beliefs.rs:250` 分派至 `apply_experience`、`apply_material`、`apply_horizon_expiry`；更新方法定义于 `packages/engine/src/strategy/fundamental/update.rs:68`、`:93`、`:134`。当前 session callers 包括 `packages/engine/src/session/decision_chain.rs:220` 的 `.apply_experience`，以及 `packages/engine/src/session/decision_chain/roots.rs:438`、`:456` 的 `.apply_cause`。这些边界与历史记录所述同 owner 分派、decision-chain 调用相符，未发现因本批材料而新增的 ownership 候选。

## 结论与限制

确认三份文件完整且哈希匹配；历史 review 间的通过/未通过状态已如实保留。现行调用者检查支持 retain 及 `ZiNoiseStrategy::strategy_data` 已实现的判断。未发现新产品候选。此结论限定于本批三个历史记录及上述相关符号，不替代全量交易语义审查。
