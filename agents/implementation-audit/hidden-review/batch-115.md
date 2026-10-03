# 批次 115：中文化复核记录独立复核

## 基线与范围

- 代码基线：`43b1aa5f25226c72976ca172d32f4a8eeb2272ad`（目标 worktree）。仅阅读指定三份中文化审查记录；为核对其代码术语与当前调用边界，只读检查相应 `PlanBook` / `TradingPlan` 定义、会话调用点、实现审计 G/Q 总账及最新 ADR-0028。未修改产品源码、未运行测试/构建/回归、未做 Git 写操作，也未联网查询 A 股规则。
- `agents/oop-refactor-audit/chinese-localization/reviews/final-07.md`：22 行，SHA-256 `309c013845c1f8a3eeb929d60cd801e8d95466658e6ba9ec30298a310d86aeba`，连续阅读全文至 EOF。
- `agents/oop-refactor-audit/chinese-localization/reviews/final-08-recheck.md`：14 行，SHA-256 `aebe98888f262ac38eeab01330a24302026f1b231b26cd37859659e25826f01a`，连续阅读全文至 EOF。
- `agents/oop-refactor-audit/chinese-localization/reviews/final-08.md`：22 行，SHA-256 `d83f4cc8bd29bef49b60f42b589466ea9e6fab72397c69af0e3fca661b823238`，连续阅读全文至 EOF。

## 独立复核结论

1. **大 A 语义：通过（仅限这三份文字审查记录所覆盖的中文化差异）。** 内容均声称只改审查材料文字，没有提出改变 A 股制度、交易单位或实现的证据；本批也未把它们扩写成新的交易规则审计结论。ADR-0028 的发布/Pages 决策与这些文本术语问题无关，且明示不改交易、资金、股数单位及存档语义。此项不能替代官方规则核验。
2. **必要性与最小范围：通过。** 报告中的修改建议集中于中文叙述可读性及代码概念命名一致性，合理；不支持据此改产品实现。
3. **术语与审查结论一致性：发现一项实质审查记录冲突，尚不能判定修订是否完成。** `final-08-recheck.md` 称 group-08 整体通过，仅有 `current` 漏译；`final-08.md` 则以“最终复核”判为不通过，并列举重复用语、不自然空格及英文叙述标题等多项待修项。两文均称针对 `group-08.diff`，但未提供足以确定先后版本的 diff 指纹或修订证据。因此二者不能合并成一个确定的通过结论：需以实际修订后完整 diff 再复核；当前无法确认 final-08 所列问题已修复。

## 当前代码与领域术语对照

- `packages/engine/src/plans/state.rs` 中 `TradingPlan` 持有 `review: ReviewConditions`，公开 `review()` accessor，并定义 `record_review`、`record_resource_review` 写入口；`packages/engine/src/plans/mod.rs` 的 `PlanBook` 转发这些写操作。
- 当前生产 caller 在 `packages/engine/src/session/decision_chain.rs`：生命周期已有计划与新建计划都会通过 `PlanBook.record_review` / `record_resource_review` 记录事实；计划生命周期后续读取 `review()` 参与判断。故 final-07 指出的 `review` 是真实代码概念，翻成泛称“复核记录”会丢失其与字段/API 的直接对应关系。建议按其意见保留代码名 `review`，在叙述中辅以中文解释。
- 该术语本身描述计划观察/复核条件，不涉及证券交易制度语义；不能据函数名推导真实交易所流程。

## G/Q 与 ADR 交叉检查

- 当前 `agents/implementation-audit/implementation-audit-2026-10-02.md` 明确自述审计产品源码基线为 `08e4fc7`，不是本批 `43b1aa5`。其中 G01–G68、Q01–Q23 仅作交叉参考，不能冒称已经在本批基线重做 G/Q 全量源码核查；本主题不对应需要新增或改写的 G/Q 项。
- 最新 ADR-0028（2026-10-03）确认发布链和静态 Pages 边界，不涉及 PlanBook/TradingPlan 的 `review` 命名，也未对该术语提供产品变更授权。

## 门禁裁定

三份记录对文字差异没有证明任何 A 股语义变化；关于必要且最小的建议基本成立。但 group-08 两份复核结论存在未消解冲突，且 final-07 所列 `review` 命名对应问题仍应作为有效修订要求。**本批审查记录复核：有条件不通过；在补充 group-08 修订 diff 的连续全文与指纹，并确认术语修订后再复核前，不应宣称中文化审查全部通过。**
