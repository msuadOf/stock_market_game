# 批次 201：engine-strategy 旧复核材料再核

## 范围与读取证据

按任务指定顺序逐篇连续读取至 EOF；三篇均为完整读取，没有以搜索片段代替阅读。计划源位于主工作区的 `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/`。

| 文件 | 行数 | SHA-256 | 阅读 |
|---|---:|---|---|
| `engine-strategy-03.md` | 13 | `0af9b263cdf82cf04559d0753c489fb4828c112239db7d3c63e940c3c1b1a01a` | 至 EOF |
| `engine-strategy-area-final.md` | 30 | `550e1fca7ddc7dcb9ba3dc5b57c1a14efde80db62af4301788e54c1883b36cf2` | 至 EOF |
| `engine-strategy-area-reference-final.md` | 24 | `9c6b2799a3e3cb8b2aa2376de35c972796769f94828a32c346f0699ae3c11b6e` | 至 EOF |

章节族分别为：单项最终复核入口/版本哈希与范围限制；区域正文范围、候选及材料一致性、三项门禁；区域引用角色与当前正文哈希绑定。三篇都明确属于历史调查复核证据，不证明候选实施或测试执行。

## 对照当前源码与后续审计

旧材料记录 ST03-A01 将散户实例参数投影至临时 `StrategyData`，保留调用方的个人观察/经历及原 kernel，并记录六个被显式映射的策略字段。当前代码在 `packages/engine/src/strategy/zi_noise.rs:40`、`:42-51` 建立完全相同的投影；`:232-235` 仍委托 `decide_retail`。`packages/engine/src/strategy/data.rs:21-55` 定义并集参数，`:57-77` 提供散户构造值。旧结论关于该投影当前存在得到再证；这是已有实现，不能作为待实施工作重开。散户买卖意图仍受当前调用边界约束（例如该文件 `:255-275`），不能把纯参数投影解释成交易受理规则变更。

旧区域报告记录 ST01-A01 的 `TradingPlan` 字段可见性收口、`PlanBook` collection/index 所有权及 serde/source API 边界，并将期限校验、恢复旁路列为另行行为线索。当前 `packages/engine/src/plans/state.rs:164-190` 可见 `TradingPlan` 序列化契约与 `pub(in crate::plans)` 字段；`packages/engine/src/plans/mod.rs:135-179` 可见 `PlanBook` 的集合/索引及恢复时 `from_parts` 路径。更具体且更新的 implementation audit 将计划期限列为 G69：恢复路径未充分复核 horizon，期限运算存在无效查询/溢出线索（`agents/implementation-audit/implementation-audit-2026-10-02.md:58`）。因此，旧报告的“期限边界独立保留”被后续 G69 具体化；不得把旧候选复核通过当成该行为缺陷已修复。此处只记录候选反证/遗留，不改源码。

历史报告的“最终”是各自材料版本内的状态。区域正文明确不重新声称 48 文件源码复读；引用绑定报告又只核正文引用与 hash，未审源码。应以提交 `43b1aa5` 之后更新的 `agents/implementation-audit/implementation-audit-2026-10-02.md`、`coverage-index.md` 及其 G/Q 证据为当前需求遗漏与裁定索引，并以明确修订/取代旧描述的现行 ADR 为领域决定；不能让早期 area final 或历史 agent 方案覆盖这些后续材料。相关现行决策包括 `docs/decisions/0006-npc-strategy-module.md`、`0016-fundamental-factor-model.md`、`0021-strategy-position-choice-and-noise-pricing.md`、`0026-individual-institution-experience.md`。这次未新增 A 股制度主张；只确认字段投影与账户/Session/策略内核的层级边界。没有官方规则复核，也没有运行测试。

## 独立复核结论

1. 大 A 语义：旧复核没有提出新交易规则；StrategyData 参数映射不改变 A 股受理语义。证据只支持代码职责边界，不支持官方现行规则背书。
2. 必要性与范围：本批是历史文档核对，不产生产品改动；保留 ST01/ST03 的历史 OOP 候选状态，不扩展其授权。期限恢复边界应沿 G69 跟踪。
3. 遗漏与跨层漂移：历史报告的限定语清楚，但不可把限定通过外推为源码全面通过。G69 是后续审计对旧期限线索的具体候选反证；更晚的 G/Q、ADR 和明确取代记录优先。

结论：三篇历史复核文本与其声明的窄范围一致；ST03 投影状态再证，ST01 的期限行为仍有当前审计记录的缺口。未改游戏实现、未运行测试。
