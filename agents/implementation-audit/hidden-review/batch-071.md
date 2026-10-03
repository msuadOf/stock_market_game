# Batch 071 独立复核

## 来源与范围

按 `scan-plan.json` batch 71 连续读取三篇来源至 EOF；行数、SHA-256 与清单一致。来源是 OOP 审计的最终整合/引用记录及 engine 测试清单复核历史。审查的是旧结论在基线 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad` 下是否仍适用，不把来源中的历史 agent 指令当作现行工作授权。已全文读取 caller `AGENTS.md` 与 `docs/principles.md`，并对照总账、裁定记录、开放问题及相关 ADR。未改产品代码，未运行测试、构建或 Git 写操作；HEAD 标识由只读查询确认。

| 来源 | 行数 | SHA-256 | 章节族与状态 |
|---|---:|---|---|
| `agents/oop-refactor-audit/exhaustive/reviews/engine-strategy-area-final.md` | 30 | `a20be83027fc9b743145814c10ca2b70f87232456c92fe313dd83734b43b3d82` | 范围/绑定、owner 与职责整合、三项门禁；仅继承指定批次的有限复核，不声称 48 源重读或源码实施完成。 |
| `agents/oop-refactor-audit/exhaustive/reviews/engine-strategy-area-reference-final.md` | 24 | `e92ad7cb3bd532b3b72a7de175e534dc471e4c0047accfce4cd47fcd4cc6ba18` | 当前引用角色及哈希绑定；明确不重审源码、规则或候选材料。 |
| `agents/oop-refactor-audit/exhaustive/reviews/engine-tests-01.md` | 86 | `ad3b4a01cc35aff76989da49fb6882447edfbfd19079b372d03e2cf4119c4754` | 18 个测试文件归属复核及历次 fixture 分类绑定；最终 evidence 分类复核通过，测试未执行。 |

## 当前调用与状态

- strategy-area 报告中的 ST03-A01 是重复策略参数投影收敛。当前 `packages/engine/src/strategy/zi_noise.rs:42-51` 有实例级 `strategy_data()` 投影，生产策略入口在 `:151`、`:191`、`:234` 复用，`:296-318` 有参数投影测试。`session/pipeline/npc_decisions.rs:179` 是 session 消费入口。旧记录提案在本基线已实现；未发现需新提取或扩充持久状态的候选。strategy-area final 仅限定性继承此前材料，不作全量源码复读声明。
- ST01-A01 对 `TradingPlan` 写口的收口属于 OOP/API 可见性候选，不是交易行为修复。当前定义位于 `packages/engine/src/plans/state.rs:167-193`，生命周期/集合操作由 `packages/engine/src/plans/mod.rs:140-182,328-401` 的 `PlanBook` 持有。area 文档要求保留 serde 名称与反序列化范围，也把 PublicLibrary restore、期限溢出等 D01/D02 线索独立保留；本批没有验证或授权这些行为变更。
- engine-tests-01 只审阅旧 manifest/evidence 与测试对象分类。最终记录明确将测试名、fixture/helper 分栏，旧的未通过绑定由后续 evidence binding 修订闭环；18 项全为 `support`、无产品 action。不得把“通过”理解成测试运行成功，或将纯测试支撑对象升级为 engine 领域对象。
- 大 A 语义没有新变化：来源没有提出制度变更；StrategyData 参数与测试 fixture 是游戏策略模型，不是交易所规则。沿用现有 ADR-0017/0021、交易规则简化边界，不宣称本批独立核验官方规则。

## G/Q、ADR 与门禁

当前总账 `agents/implementation-audit/implementation-audit-2026-10-02.md:27` 说明 G01–G68 中 G27 已核销，其余列为当前缺口；`exhaustive-review/resolution.md` 记载其合并、排除和后续替代边界。三篇来源是 OOP 审查/测试清单记录，没有生产缺口的新事实，也没有核销任何 G 项。相邻策略缺口 G06–G09、G16、G28、G35–G38 均不能由对象边界或参数投影核销；Q02/Q09/Q11 状态也不因本批改变。`docs/open-questions.md` 的 Q11 补充决定与 ADR-0026，以及 ADR-0023–0028 的后续范围决定，未取代这些来源记录的限定结论。ADR-0018 整体仍为 proposed，仅已接受的具体目标依总账处理。

结论：来源身份及历史修订链一致。旧结论中关于 strategy owner/参数投影及测试辅助符号分类的限定记录在当前代码/总账下仍可引用；未发现需新增 G/Q、OOP 产品候选或修复动作。通过仅指本批静态复核，不代表测试、构建、A 股规则来源或完整产品验收通过。
