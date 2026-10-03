# 独立审读：batch-056（owner 1）

## 基线与来源

- 目标 worktree HEAD 为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`；来源计划 `scan-plan.json` 为 batch 56，owner 1。
- 按计划从主工作区连续逐篇阅读三份来源至 EOF，行数与 SHA-256 均匹配：
  - `agents/oop-refactor-audit/exhaustive/reviews/engine-pipeline-01-final.md`：23 行，`99300a3a697c77b5a7bd3f9877ded97583370255ecc24c904371d443fb75e25a`。
  - `agents/oop-refactor-audit/exhaustive/reviews/engine-pipeline-01-full-supplement.md`：33 行，`b06fb8979345893e6fd3bfde39066daa8e91e9b8795d27eb4dac1ae8c9ab5537`。
  - `agents/oop-refactor-audit/exhaustive/reviews/engine-pipeline-02.md`：45 行，`8d11cbb6e2e7fcb61cd2d86a847198fc1791a6089dc140d281396e6207844211`。
- 已读 worktree `AGENTS.md`、`docs/principles.md`。未运行测试、构建或 Git 写操作，未改产品代码。

## 当前 caller 与裁定

1. **engine-pipeline-01 / A01：旧结论再证。** 01-final 是对 lane patch staging 时间差异的 delta 复核；full-supplement 明确扩展到 `account_validation.rs` 全文，并说明候选 A01 仍未实施。当前 `account_validation.rs` 仍由 `AccountValidationState` 执行惰性预算装载（`:683-714`）、验证后写入 `BudgetUpdate`（`:662-680, 770-776`），`AccountBudget::apply_update` 仅装载已验证值（`:1139` 起）。生产入口经 `AccountValidatorDriver` 被连续、竞价及盘前 transaction 使用；各股票反馈由 `ready_stock_stream.rs:78,114,182` 进入后续流程。职责提取仍是 proposed OOP 候选，不是修复或产品缺口。
2. **engine-pipeline-02 / D01：缺陷再证，上一结论有精确边界。** 当前 `AdaptivePlanChainCoordinator::advance_after_typed_outcomes` 与 `advance_after_auction_outcomes` 在 `:305-328` 使用 `outcomes_in_plan_identity_order(steps)?` 早退，错误早于设置 `failed` 的 `advance_*_batch` 错误处理。排序预检在 `:544-566` 验证 pending 子集与重复 key。故这些早退 Err 不会停用 coordinator；后续 `next_ready_batch` 不会仅因该错误被拒绝。`finish` 在 `:865-875` 另要求 roots exhausted 且 pending 为空，因此非完成态仍报 invariant；满足完成条件时则可能成功。02 最终 delta 已准确区分这些情况，旧版“finish 不拒绝继续”的较宽表述不应再单独引用。缺少 P3 预校验早退后的停用测试仍是独立待办，当前源码没有修复。
3. **G/Q 与后续决议。** 现行实现审计总账记录 G01–G68，G27 已核销、其余 67 项为缺口（`agents/implementation-audit/coverage-index.md:7,294`）。`resolution.md` 将 OOP 结构审查与 G 缺口分开；本批两份 pipeline OOP 候选没有明确映射到 G 编号，也不构成核销。`open-questions.md` 中 Q11/Q12 已由后续决定收敛；ADR-0023–0028 涉及合成历史、资金池、日终持久化、经历模型和部署发布，没有取代本批账户预算对象边界或 coordinator 预检失败状态契约。交易制度语义沿用现有 ADR-0017/项目规则；本批不提出新 A 股规则，也未做官方规则再核验。

## 三项门禁

- **大 A 语义：** 候选 A01 只转移私有预算字段写入归属，仍保留现金（分）、可卖股份（股）、已配置证券类别和费用等原约束；D01 是错误生命周期处理，不改变受理顺序或交易规则。没有发现语义漂移。
- **必要性与范围：** A01 属局部所有权提议，源码尚未迁移。D01 是已定位的实现缺陷，但不应混入 OOP action 或宣称修复；待办建议将预检 Err 也设置失败状态并补连续/竞价路径测试。
- **边界、跨层与复杂度：** 01 补充覆盖了 round-local disposable staging 与累计 state commit 的时点区别。02 当前候选文本正确区分 `next_ready_batch` 与 `finish` 条件；P3 重复/错误 identity 的预检用例仍缺。没有新 G/Q 候选或证据支持增加对象抽象。

## 结论与限制

三份历史记录的来源版本和 EOF 覆盖已核实。A01 候选仍未实现；D01 行为仍存在，审计记录准确说明了预检早退、后续 `next_ready_batch`、`finish` 的各自条件。未运行测试，不将本次静态审读写成回归通过或产品验收。
