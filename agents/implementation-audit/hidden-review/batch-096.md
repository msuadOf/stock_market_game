# Batch 096 独立复核

基线为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`（`.worktree/implementation-reaudit`）。依计划从主工作区绝对路径连续全文读取三篇来源至 EOF；实测每篇 22 行，SHA-256 均匹配 scan-plan。已阅读目标 worktree 的 `AGENTS.md`、`docs/principles.md`。本文核对翻译复核报告与其绑定的原始审计结论，并追踪当前生产 owner/caller；历史 subagent 指令仅作历史记录，不视为当前工作指令。未改产品源码、未运行测试/构建、未执行 Git 写操作。

## 来源及章节状态

| 来源 | 结论及完整性 | 复核状态 |
|---|---|---|
| `engine-session-02.md` | 仅中文化差异通过；记录原语义审查和两个修订绑定。全文到 EOF。 | 翻译门禁结论属翻译范围；不构成源码重新审计或实现验收。原文的 candles A03 不能再称未实施。 |
| `engine-session-03.md` | 仅中文化差异通过；A01 target 精度变更沿用原 review。全文到 EOF。 | 原有 RootReadContext/DecisionChainObservation/InstitutionDecisionRoot 职责与 coordinator 边界仍可作历史设计依据；不将 OOP 对象提取记为修复。 |
| `engine-session-04.md` | 仅中文化差异通过；原语义复核为只读候选调查。全文到 EOF。 | 多数章节为 retain/support 或既有边界；没有依据将翻译审查升级为产品验收。 |

三文档复核的是中文化相对原文的语义/字段保持。原 review 明确未重新审计源码或重新访问官方规则。大 A 语义沿用已登记的游戏简化，金额/股数单位和连续竞价/集合竞价区别未见因翻译改变；本批没有新的交易规则依据主张。

## 现行代码追踪

- **A03 candle owner 已落地。** 当前 `packages/engine/src/session/candles.rs:90-205` 定义 `SessionCandleBook`，集中拥有 histories 与 active maps，并提供 `record_trade_or_mark`、`commit_active`。`packages/engine/src/session.rs:1143` 将它放入 `CommittableSessionState`，初始化见 `:1426`；连续成交 caller 在 `packages/engine/src/session/pipeline/continuous_tick_finalizer.rs:411`，集合竞价/日终 caller 在 `packages/engine/src/session/pipeline/auction_day_end.rs:1133-1136,2167`。因此旧提案“待实施”状态过时；此 owner 提取只确认结构落地，不单独证明首笔成交、完整历史或双字段存档合同正确，也不核销其他 G。
- **AccountPagedMap U060 线索仍在源码，但当前调用处于 shadow。** `packages/engine/src/session/account_paged_map.rs:86-115,118-160` 的并行更新可能在后续 Err 前修改该 receiver；当前决策快照通过私有候选 `shadow` 调用（`packages/engine/src/session/pipeline/decision_snapshot_capture.rs:95-114,228-232`）。外层候选丢弃与 receiver 本地原子性是不同层级；不应把此历史线索误报成已证明的权威状态泄漏或新 G。
- **CivilClock 序号耗尽仍是未决行为边界。** `packages/engine/src/session/civil_clock.rs:330-351` 当前直接 `next_due_seq += 1`，没有耗尽检查；生产调用位于 `packages/engine/src/session/company_operations.rs:69`。engine-session-02 原记录已将 `u32::MAX` 哨兵描述为待决定建议，而非既定规则。由于没有找到更新 ADR 接受该 policy，本批将其保留为待决候选，不擅自登记为已接受契约/G；若后续确定行为需更新 ADR/测试。
- Attention 的 `pop_due_npc_ids` 仍由 `GameSession` 接线（`packages/engine/src/session/attention.rs:321-329`），个人 RNG/候选时刻由 `NpcAttentionState::evaluate_candidate_with_signal` 修改（`:332-349`）；与历史报告的状态 owner 结论相符。原报告自身指出 discovery 分支、stale-entry/popped 语义缺直接测试；这属于既存覆盖线索，不等于功能缺失或本批新增候选。

## G/Q/ADR 对照与结论

已对照 worktree 的 `agents/implementation-audit/implementation-audit-2026-10-02.md`、`docs/open-questions.md` 与 ADR-0011、0017、0018、0019、0025 及更新至 ADR-0028 的决策目录。总账定义 G01–G68 及现存 G16 历史复制边界；本批三篇来源没有为其提供新的生产消费链证据，不能由对象提取/审查文档核销。Q1–Q12 在 open-questions 的已解决表有对应 ADR；本批未发现需要重开的问题。ADR-0018 整体仍为 proposed，不能将旧 OOP 方案整体视作已接受。未发现被上述更新决策明确替代后仍需沿用的旧交易 policy。

**结论：三篇中文化 binding 在其限定范围内有效；当前源码核对确认 A03 已实现，旧“未实施”结论需按历史快照解释。没有由本批材料独立确立的新 G；CivilClock 序号耗尽建议维持待决，U060 维持局部行为线索且当前在 shadow 路径中。** 本文不构成 G01–G68 全量复审、OOP 提取的行为验收、测试通过声明或官方规则复核。
