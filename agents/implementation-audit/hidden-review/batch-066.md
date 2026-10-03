# 批次 066：OOP 会话审计材料复核

## 范围与完整性

- 计划清单 `agents/implementation-audit/hidden-review/scan-plan.json` 的 batch 66 指定三份来源。按要求从主工作区 `/data1/baiyifan/workplace/stock_market_game/` 逐篇连续读取至 EOF；实际行数与计划一致，SHA-256 全部匹配计划值。
- 已读取本工作树 `AGENTS.md`、`docs/principles.md` 全文。产品代码基线 `43b1aa5` 的完整提交为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`；当前工作树的生产调用接线可由 `packages/engine/src/session/pipeline/adaptive_plan_chain.rs:260-288` 追至 `PlanChainOperationBatch::yield_adaptive_candidates_with_root_wait`，并由 `packages/engine/src/session/plan_chain_candidates/adaptive.rs:77-112` 驱动 ready roots。协议 caller/owner 为 `ProtocolSession`：`packages/engine/src/session/protocol/civil/session.rs:71-81,333-341`。
- 后续 `engine-session-06-retention-final.md` 明确复核并替代 manager delta 中 8 个 `retained_symbols.to` 文案问题：20 个文件均 `actions: []`，8 个 retained boundary 指向既有对象，symbols 仅含来源和理由。该旧材料问题已修正，不再作为当前候选。

## 来源逐篇复核

1. `engine-session-05-manager-delta.md`（22 行，全文至 EOF）：延续 A01–A04 的 retain 结论。A01 的 `InFlight` 已拥有 receiver、remaining、snapshot；当前 `prepare_ready_accounts` 在 `packages/engine/src/session/plan_chain_candidates.rs:342-380` 接收、安装 personal state 并推进剩余数。A02 记载完成/轮询时机可能影响 route 入 FIFO 与候选接受次序；这是调度现象，不应把 identity 说成交易优先级，也不承诺跨调度顺序固定。A03 连续 identity 已有 `adaptive_source_generation_is_continuous_across_distinct_root_accounts`，而压力调度、key 冲突、typed outcome 乱序关联仍仅是明确待补测试边界。A04 未使用参数事实可由 `push_restructure` (`plan_chain_candidates.rs:487-502`) 复核，方法只构造 Restructure event。
2. `engine-session-05.md`（13 行，全文至 EOF）：它是最终复核入口而非新代码调查；明示无 OOP actions、owner/DTO/纯函数/测试继续保留，并限制其通过结论仅代表文档调查，不代表方案实现或测试运行。没有独立的新产品缺口主张。
3. `engine-session-06-manager-delta.md`（40 行，全文至 EOF）：协议复核区分裸 `GameSession::end_civil_day_update` 与 `ProtocolSession` 外层 checkpoint/rollback；核对 immutable `Deref`、`tick_batch(&self)` 的 published runtime、panic swap 与进程级故障边界。现行实现可见 `ProtocolSession::checkpoint/rollback` (`protocol/civil/session.rs:73-81`) 及成功 `tick_batch_delta` 后才发布 baseline (`:333-341`)。其中 8 项 retained_symbols 目标用语的问题已由随后 retention-final 完成修订。manager delta 保留的自由调度、失败时不外发部分帧等行为覆盖是测试边界提示，现有材料没有给出已触发的产品失败证据。

## 现行总账、ADR 与候选状态

- 对照 `implementation-audit-2026-10-02.md` 的 G01–G68、Q01–Q23 总账及 `reaudit-engine.md`：此三篇没有提出可映射到新增 G/Q 的独立产品缺陷。总账 Q22 已明确限定实际接收轨迹需核验，来源拼接/向量顺序本身不证明来源优先级；它与这里的调度次序提醒相容，不能据此恢复全局固定来源排序。相关产品事实仍以总账已有条目及调用链为准。
- ADR-0017 的现行约束明确临时 channel 只回传完成结果、不持有交易权威状态或决定交易优先级；typed plan continuation 使用身份关联结果。旧来源对 FIFO/ready 时机的提醒不得扩大成跨账户固定排序要求。ADR-0018 对实际受理和同实体依赖的后续修订优先于旧来源类序描述。ADR-0020 规定 allocator 不决定请求何时受理；ADR-0025 区分内存 checkpoint 与公开日终存档候选。这些现行决策与 manager-delta 的限定一致。
- A 股语义：本批仅核对调度身份、会话状态恢复和审计分类，不改撮合、委托受理、结算、T+1、费用或证券类别规则；未产生需新查交易所规则的主张。identity/sequence 不应解释为交易优先级。
- 必要性与范围：三篇均是 OOP 审计记录，未要求生产改动。OOP 提取不是缺陷修复；文件内历史行动/审查指令不视为当前任务要求。
- 新增候选反证：没有足够的新产品行为证据新增缺口。调度扰动、key 冲突和 typed outcome 乱序覆盖仍是可考虑的测试边界，不升级为已确认 G；此前关于 `GameSession` 裸 adapter 后续失败非局部原子的材料结论也保持窄范围，不推翻 `ProtocolSession` 完整外层事务边界。
- EOF：三篇来源各自均已连续读至文件末尾。未运行测试、构建或回归；未改产品代码。
