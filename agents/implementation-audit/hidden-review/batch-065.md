# 批次 065 隐藏复核

## 范围与来源完整性

复核者：owner5。调用方工作树：`/data1/baiyifan/workplace/stock_market_game/.worktree/implementation-reaudit`，HEAD `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`；源仓库：`/data1/baiyifan/workplace/stock_market_game`；scan-plan 基线：`43b1aa5`。本轮重新从头读取三份来源至 EOF，行数与 SHA-256 均完全匹配，各 `aliases: 1`。特别区分历史候选记录与 43b1aa5 当前实施状态：ES03 的 `engine-session-03.md` 是旧候选调查入口，称 A01 未实施只描述其记录形成时状态，不能据此断言基线仍未实施。batch064 已按同一基线核实该候选现已落地。

| 来源 | EOF / 行数 | SHA-256 | 核查范围 |
|---|---:|---|---|
| `agents/oop-refactor-audit/exhaustive/reviews/engine-session-03.md` | EOF / 13 | `3e21e59b4390b4825cd0335445e335d48cccaf7b7a28067388d69b733738f1fc` | 最终复核入口；注明继承历史源码审阅及 A01 最终绑定，不声称本文件本身是源码复核。 |
| `agents/oop-refactor-audit/exhaustive/reviews/engine-session-04-manager-delta.md` | EOF / 27 | `75fc970869d556baa1bc4690fea6a81b436124dded5643c1cbe8e2898652bd84` | ES04 增量核查；逐项确认无 action、母单边界、panic 事务边界及成本差值反例。 |
| `agents/oop-refactor-audit/exhaustive/reviews/engine-session-04.md` | EOF / 13 | `419896cef61b4eba3c50d2ad89894e571eb28c0aec8a9303da8bdf335c5819a8` | ES04 最终入口；继承完整复核与 manager delta，明确没有实现或测试完成声明。 |

已核对 `docs/principles.md`、`docs/open-questions.md`、ADR-0017、ADR-0018、ADR-0023、ADR-0025、ADR-0026 及 `docs/trading-rules.md`。工程原则要求核心纯逻辑、显式错误和 TDD；ADR-0017/0018 的候选隔离、typed continuation、局部真实冲突顺序及单点提交约束仍是相关契约。此批仅复核 OOP 调查材料，没有改动交易政策，不据此新增交易所规则主张。Q11 个人风险/经验方向已由 ADR-0026 明确，不能表述为待定真实市场规则；历史候选清单中基于来源类别设交易优先级的旧约定已被 ADR-0018 修订取代。

## 基线调用链与 owner 核验

以下基线核验使用 `git grep` / `git show` 读取 `43b1aa5`，不是仅凭报告转述：

- **ES03 A01 历史候选与基线实施状态分开判断。** 指定报告所述“只读候选调查，未实施”是其历史记录状态；不能作为 43b1aa5 的现状。基线 `decision_chain/roots.rs:6-29` 定义并捕获 `RootReadContext`，`roots.rs:188-221` 定义 `InstitutionDecisionRoot` 并产出个人状态、typed operation batch 与 diagnostics。`plan_chain_candidates.rs:290-339` 的 `start_ready_accounts` 捕获并共享 `Arc<RootReadContext>`，对就绪账户取出私人 `PlanPersonalState` 后启动 root。结果接收/安装由同文件后续 `prepare_ready_accounts` 路径负责；生命周期收集/应用仍由 `plan_chain_candidates/adaptive.rs` 基于 typed route outcome 与当前 candidate 编排。真实 root 捕获入口由 `pipeline/adaptive_plan_chain.rs`、`pipeline/ready_ingress.rs`、`pipeline/npc_tick_preparation.rs` 到达；`DecisionChainObservation` 仍独立承载冻结市场/路径/技术观察。因此 A01 的主要 owner/input 分离已在基线实现；不是尚待实施的候选，也不等于其全部性能目标已实现。
- ES04 A01 的 `PlanPersonalState` 在 `decision_chain/personal_state.rs` 持有五项逐账户个人事实；基线 `PlanChainOperationBatch::start_ready_accounts` 对就绪账户取出，worker root 使用，`prepare_ready_accounts` 路径恢复安装到同一个私有 candidate。不是第二份持久权威状态。缺 key、重复 install 和完整恢复的直接测试缺口由报告明示，manager delta 没有掩饰。
- ES04 母单/执行边界由 `execution/orders.rs`、`execution/reconcile_plan.rs`、`execution/records.rs` 和对应 candidate/交易流程共同承担。manager delta 将同向改价、反向替换、符号价格透传与下游校验描述为游戏内母单执行行为，没有把母单说成交易所委托类型；单位与实际市场撮合边界没有据此变更。
- `institutional_behavior.rs` 的风险观察/评估由 session 和 `BeliefBook`/个人经历事实承接。ES04 将其标为 ADR-0026 下的个体游戏行为，不泛化成强制止损规则，语义限定恰当。
- `minimal_snapshot.rs` 中 `SaveMarketSnap`、`SaveAccountSnap`、`SaveSnapshot` 是 serde/ts-rs DTO；消费者为 snapshot、生成 TypeScript 类型和持久化接线。ES04 将 DTO 与完整 `SaveSlot` 区分，并标出直接序列化/字段边界测试缺口，没有提议无关改动。

## G/Q 与发现

- 当前实施审计 `agents/implementation-audit/reaudit-engine.md` 将 G06–G09、G16、G28、G35–G38 及 Q02/Q11 明确列为仍存缺口或待定边界；G16 尤其是 `RootReadContext::capture` 仍复制完整 `PlanBook`。A01 的基线落地不核销 G16；本批不改 G/Q 映射或状态。其他 G/Q 与本批无直接因果，不据此新增重复登记。
- ES04 报告发现 `checked_add(...).expect(...)` 之后续执行可能 panic，而本方法不提供局部 panic rollback；按 ADR-0017 进程级致命故障边界描述，没有误称为可恢复事务或建议 `catch_unwind`，结论成立。
- ES04 对 unit-018 差值溢出反例的拒绝正确：`price` 与过滤后的 `cost` 都为正 i64，差值范围绝不达到 i64 溢出边界。无需新缺陷或无根据测试。
- ES04 报告保留直接测试缺口；通过结论限于复核范围，不等于测试执行。ES03/04 最终入口明确声明继承式/限定审阅且未运行测试；ES03 当时的候选实施状态不代替本批基线状态核验。
- 未发现报告中对 caller、owner、consumer 的实质误述，也未发现跨层 A 股概念漂移。旧披露文档准确性线索在模块审计中已单独记录，与本批三份报告的结论无关，不把它混成新发现。

## 三门禁结论

1. **大 A 语义及依据：通过。** 本批为已有边界审阅，没有交易语义变化；母单是游戏内执行目标，交易受理/撮合规则由既有 ADR 与引擎边界负责。ADR-0026 的个体经验被准确标为游戏假设。
2. **必要性及最小范围：通过。** ES03 候选文本的 `proposed` 状态只代表历史计划状态；基线已有相应 owner 与调用链，没有理由再新增重复 manager 或权威状态。ES04 记录保留现有窄 owner，没有制造额外状态。历史报告的限定复核结论不冒充完整实现验收。
3. **遗漏边界、跨层漂移及复杂度：通过（保留已披露缺口）。** 关键 caller、状态取出/恢复、typed continuation、母单状态变化、panic 边界、snapshot consumer 均能对应基线；直接测试覆盖缺口仍需在未来相关改动中处理。

状态修正依据：指定历史来源 `engine-session-03.md:5-13` 将材料标为候选调查；batch064 的 `batch-064.md:18-25,30,39` 和 `batch-064.json` 的 `engine-session-03-A01` disposition 已明确记录基线落地及 G16 仍开放；本轮重新读取的基线代码位置为 `roots.rs:6-29,188-221`、`plan_chain_candidates.rs:290-339`。参见 [batch064](batch-064.md)。

仅进行只读静态核验；未运行测试/构建/回归，未写产品代码，未执行 Git 写操作。输出仅为本审阅记录及其 JSON 元数据。
