# 独立审读：批次 135（owner 5）

## 基线与范围

- 源码仓库 HEAD：`43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。审读限定于三份指定候选设计记录，以及为核验其生产 owner/caller/consumer 所需的当前代码与审计/决策索引；没有修改产品文件、运行测试/构建或执行 Git 写操作。
- 已按 output_root 的 `scan-plan.json` batch 135 精确核对相对路径、行数与 SHA；三份翻译前记录存在于 source_root，caller worktree 的 HEAD 同为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`，但未包含这些未跟踪翻译记录。因此全文审读取自 source_root，生产 caller/owner/consumer 则在 caller worktree 基线核对；没有把 caller worktree 缺少审计输入误作产品源码差异。
- 三份输入均全文读至 EOF，行数和 SHA-256：
  - `agents/oop-refactor-audit/chinese-localization/before/exhaustive/managers/domain-before/modules-engine-foundation-02.md`：52 行，`e293dfc270059893c341058232d5c8a83f1fa011a7282ac67fa092bd57cbe1ef`
  - `agents/oop-refactor-audit/chinese-localization/before/exhaustive/managers/domain-before/modules-engine-foundation-03.md`：21 行，`167ff694aa710382472355dd2f489e031ef48fa4bc7347ce81500ab40a3c0006`
  - `agents/oop-refactor-audit/chinese-localization/before/exhaustive/managers/domain-before/modules-engine-foundation-04.md`：31 行，`aa7ce64f6f12e431a7f989d563fe8aa11728c39f672d34e44b4ab85d7c04a646`
- 对照 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`、实现审计总账与 ADR-0023–0028。所有这些候选设计均标为未实施；审读结论不把保留既有 owner 的结构建议说成 OOP 迁移已完成。

## 审读结论

1. **02 的主要职责描述与生产 owner 一致。** `GameSession` 在 `packages/engine/src/session.rs` 持有诊断 collector、散户经历以及账户级 watchlist/price memory 的保存状态；session causal facade 将事实交给 `CausalCollector`，报告由 `CausalReport::from_facts` 投影。指标仍为纯输入/输出：WASM (`apps/web-wasm/src/lib.rs`)、server (`apps/server/src/routes.rs`) 和 desktop (`apps/desktop/src-tauri/src/lib.rs`) 都调用 engine `calculate_indicators`。此结构支持“保留现有对象/纯函数，不为 OOP 新增状态对象”的结论。
2. **02 的价格记忆修剪描述须保持为局部能力。** `PersonalPriceMemory::prune` 在代码中存在，但本基线生产 caller 搜索只找到测试；生产 `roots.rs` 的调用是 `watchlist.prune`。因此表中的“已核销……按持仓/计划保护集合修剪”只能表达 helper 的实现/行为覆盖，不能被解释为价格记忆已接入生产修剪生命周期。实现审计总账 G42 记载了相同边界，但该总账源码基线为 `08e4fc7`，本审读不把其完整 G 状态冒称为在 `43b1aa5` 重做审计；本次独立 caller 搜索确认了 `PersonalPriceMemory::prune` 无生产调用。候选原文没有声称该生命周期已经接线，避免扩大结论即可。
3. **02 的错误原子性边界记录具体且不误称事务。** 对 `RetailExperienceState` 的 dated writer，文档区分 feedback 提交边界与被委托 legacy writer 的部分变更，并列明计数/冷静截止溢出时可能改变的字段。它提出未来预检和全状态相等断言作为后续方案，标明本批只记载、不修复，符合“显式错误、不静默吞错”；不应将该建议误认为现有 writer 已具备整体原子性。
4. **03/04 的结构与调用边界可信。** `Market`/`OrderBook`/私有索引、纯 `Money` 运算、观察投影、evidence projector 与 `Collector` 都是已有明确 owner；`step_with_phase_timing` 用于两个性能 example，并在同一 `GameSession::step` 上产生 opt-in 的 tick 证据。`phase_timing.rs` 明确 Rayon registry 线程容量不是系统 runnable 数或 CPU 利用率，且说明 phase 时间重叠，04 未把采样说成性能验收。所列 `verification_evidence` 测试属于支撑模块而非生产领域类的说法准确。
5. **A 股语义、G/Q 关联与 ADR。** 三份材料没有改变撮合、费用、T+1、货币/股数单位或交易时段规则；“240 分钟”作为游戏观察窗口的限定正确，不冒充真实连续竞价时长。没有确认三篇候选对应任何尚未解决的 Q；Q11 当前边界已由 ADR-0026 补充，Q12 已解决。G42 是 price-memory 修剪生产接线的相关旧总账条目（基线差异如上），不能据 OOP 对象核销将其关闭。ADR-0023/24/25/26 约束历史、现金池、日终存档和机构经历，均不与候选的结构保留结论冲突；ADR-0027/28（截至本基线最新）只决策宿主/发布，和本批无直接关联。

## 有效发现及处置

- 未发现三篇候选文档中与其明确范围相冲突、足以判定为内容错误的断言；其中 02 对部分写入风险已有详细限定。
- 记录一项防止误读的生产边界：`PersonalPriceMemory::prune` 尚无生产 caller。它不推翻“保留当前对象边界”的结构结论，但说明 helper/单测覆盖不等于该行为接入会话生产生命周期；不能用本批文档或 G/Q 名称替代接线审计。
- 审读通过限于三份候选设计文档及指定调用边界，不代表所有列举源码已重新逐文件全文审计、不代表 G/Q 总账全面重验，也不代表测试、构建或 A 股官方规则重新核验。

## 代码与文档证据

- `packages/engine/src/session.rs:1148-1157,1439,1922-1923`：会话权威状态与 collector、watchlist、price-memory 的持有/初始化。
- `packages/engine/src/session/causal.rs:6-16`：session facade 查询事实并从事实构造只读报告。
- `packages/engine/src/session/decision_chain/roots.rs:486`：生产路径 prune watchlist。
- `packages/engine/src/experience/price_memory.rs` 与 `packages/engine/tests/technical_memory/memory.rs`：price-memory helper 与测试；生产调用搜索未命中。
- `apps/web-wasm/src/lib.rs:435-439`、`apps/server/src/routes.rs:632-646`、`apps/desktop/src-tauri/src/lib.rs:60-64`：三个宿主消费同一 engine 指标函数。
- `packages/engine/src/verification_evidence/phase_timing.rs:1-12`：样本语义、重叠 phase 与 CPU 利用率边界。
- `packages/engine/examples/production_entry_performance.rs:151-169`：实际 GameSession tick timing consumer。
- `agents/implementation-audit/implementation-audit-2026-10-02.md:49-54,127,150`：旧 G/Q 条目及其 08e4fc7 基线；仅作编号/范围上下文。
- `docs/open-questions.md:94-138`、`docs/decisions/0023-synthetic-history-and-matching-only.md`、`0024-shrinking-investor-cash-pool.md`、`0025-day-end-only-persistence.md`、`0026-individual-institution-experience.md`、`0027-runtime-deployment-and-build-targets.md`、`0028-tagged-release-and-static-pages.md`：现行问题与相关决策。

**结论：三份候选设计文档审读通过，未发现未处理的文档错误；明确保留 `PersonalPriceMemory::prune` 尚未接入生产的边界。**
