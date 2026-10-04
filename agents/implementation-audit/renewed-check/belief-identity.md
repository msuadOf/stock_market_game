# BeliefBook 身份恢复复核

## 范围与读取证据

- 基线为审计 worktree `/data1/baiyifan/workplace/stock_market_game/.worktree/implementation-reaudit`，HEAD `c0ab4299d104f07589008fee1af886198a2f783b`。本记录仅静态读取，不运行测试/构建，不改产品代码或 Git。
- 必读 `AGENTS.md` 与 `docs/principles.md` 已读。指定来源均连续从首行读到 EOF；行数和 SHA-256：

| 来源 | EOF 证据 | 主题 |
|---|---|---|
| `docs/decisions/0016-fundamental-factor-model.md` | 119 行；SHA-256 `1b3c7deef0a29a0f3f628b2b4455713760f9a73619d477f72ac4863070a52ae5`；以“研究参考与适用范围”及“计划契约对接”末段读至 EOF。 | NPC 个人认识、个体预期、计划契约及策略风格不决定全部行为。 |
| `/data1/baiyifan/workplace/stock_market_game/agents/oop-refactor-audit/exhaustive/modules/engine-strategy-02.md` | 343 行；SHA-256 `17e17cf8b54910d01cb9b0cf9c58224a4f60f831cb450bcc89e7a67c53ef700b`；各源码章节、跨模块关系、限制均读至 EOF。 | `BeliefBook` 所有权、策略/profile、持久化候选身份差异及合法跨身份组合。 |
| `agents/implementation-audit/hidden-review/batch-042.md` | 35 行；SHA-256 `35c953978121f3fc19e6669c5a884d81ce50b2f7a0a23fd8326c9a3c3c9f9cbe`；结论段读至 EOF。 | 旧审计明确将 belief profile 身份列为未完成核实，未判已覆盖或已复现。 |

## 保存、恢复与身份校验

- `GameSession::save` 从同一个 `belief_participants` map 汇集 `information_states`、`belief_books`、`watchlists`、`price_memories`（`packages/engine/src/session.rs:2666-2688`）。`BeliefBook` 序列化字段包含 `npc`、`profile`、`analysis`、`assumptions`、条目、经历及 policy（`packages/engine/src/strategy/beliefs.rs:86-100`）。
- `GameSession::restore` 先调用 `validate_save_slot`，重建 session 后将四张个人状态表按相同 account key 组装为 `BeliefParticipantState`（`packages/engine/src/session.rs:2716-2721,2943-2977`）。
- `validate_personal_states` 确认四张表 key 集一致（`packages/engine/src/session/persistence.rs:1104-1115`）、key 指向 NPC（`:1122-1133`），并检查 `book.npc() == map key`（`:1222-1227`）、policy 非空（`:1229-1234`）、经历和条目引用等边界（`:1235-1403`）。因此“BeliefBook 账户 owner/key 完全未校验”是错误结论；账户标识这一边已有真实检查。
- runtime恢复还校验StrategyState账户集合，并将存档策略profile与按setup/seed重建的确定性profile比较（`packages/engine/src/session/persistence/v2.rs:278-313`），策略变体参数在`:334-339`校验。此处只描述固定`c0ab429`产品树，不引用主工作区未提交的saved_runtime命名迁移；该检查验证策略状态身份，不验证BeliefBook.profile。
- 缺失的关系是：`validate_personal_states` 没有把 `book.profile` 与该账户确定性策略 profile 对照。`BeliefBook.profile` 影响个人假设构造与未来条目更新（`packages/engine/src/strategy/beliefs.rs:111-123`；`packages/engine/src/strategy/fundamental/update.rs:190,208`）；机构消费则从账户策略读 style（`packages/engine/src/session/decision_chain.rs:1023-1030`），并从 `BeliefBook.analysis()` 读权重（`packages/engine/src/session/decision_chain/roots.rs:73-90`）。因此，从结构上存在恢复后策略 style 与 belief profile 不同的候选元组；本证据只证明当前校验未绑定两者，不证明该元组违反已批准契约。
- 该差异能由 serde 表示：`BeliefBook` 派生 `Deserialize`（`beliefs.rs:86-89`），`SaveSlot.belief_books` 由 serde 持久化（`session.rs:484-486`）。创建路径确实把同一个策略 profile 传给 `BeliefBook::new`（`session.rs:1874-1901`），但这是构造一致性，不能单独证明所有合法存档都必须保持全等。本轮没有执行伪造存档或运行复现。

## 合法个体参数与必需身份

- 不应把 `AnalysisProfile` 权重、个体假设或已冻结机构 policy 阈值的差异当身份错误。ADR-0016 明确允许个体信息、方法、经验和预期不同；策略风格也不决定全部行为。`analysis_profiles/invariants.rs::cross_identity_combinations_are_legal` 与 `fundamental_method_linkage_follows_style_and_account_parity` 明确支持合法组合（`packages/engine/tests/analysis_profiles/invariants.rs:33-72`），例如机构 `ActiveTrader` 可无基本面权重、散户可有基本面分析。策略 `StrategyFamily` 也不等同 account kind。
- **正式契约核对：** `StrategyProfile` 源码称其为“账户策略的可恢复身份档案”（`packages/engine/src/strategy/profile.rs:75-80`）；ADR-0006 §8 明确 `ActiveTrader` 机构保留机构账户身份、使用动量 `StrategyFamily`，并说明策略族与账户身份不再强制一一对应（`docs/decisions/0006-npc-strategy-module.md:91-99`）。ADR-0016 明确账户身份不强制是否估值（`docs/decisions/0016-fundamental-factor-model.md:29-34`），策略保留主导风格但身份不决定全部行为（`:58`）。这些依据确认策略 profile 是账户策略身份、分析能力与账户类别可分离；没有明文规定 `BeliefBook.profile` 是该身份的唯一镜像或必须与策略 profile 全等。因此目前应作为需明确的 Q 类契约候选，不虚构必需绑定条款。
- 已有正式依据支持确定性重建的 account key → strategy profile，以及 belief book owner → 同一 account key。是否还必须要求同账户 `BeliefBook.profile` 与策略 profile 全等，现行 ADR 未明文规定；应先明确它是冗余策略身份镜像还是允许分离的个人认知 profile。不要要求个体化 `AnalysisProfile` 必须等于默认抽样表，也不要按风格强制 policy 阈值等于 style preset；这些是个人参数，不是 account 标识。
- 当前对 policy 只检查存在性，不查 profile/style provenance。engine-strategy-02 已明确把 policy provenance 定为保存契约限制/未决，而非缺陷。本次不把独立 policy provenance 推成新增问题；这里确认的是核心 `BeliefBook.profile` 与账户策略 identity 未关联，profile 直接参与后续业务计算。

## 去重与结论

- **保留为待明确契约的身份候选。** 账户 map key/`book.npc()` 已互相校验，strategy state profile 已与确定性策略校验；`BeliefBook.profile` 没有与同账户策略 profile 做恢复交叉校验，serde 表达上可出现不相同的组合。但正式 ADR 仅定义各自 profile 的用途，没有规定 belief profile 必须与策略 profile 全等；不得据此称为已确认缺陷。应明确是否允许 belief profile 与策略风格不同，再决定是否增加拒绝校验或保留分离。
- **既有 G/Q 不等价。** G07 是账户身份与是否拥有基本面分析能力的解耦问题；要求独立于账户 kind 的 profile 能力合法，不能解释为允许同一 `BeliefBook` 与所属策略互相矛盾。G42 是价格记忆修剪边界，G43 是淡出股票候选过滤，G69 是计划期限恢复算术，均非该身份链接。Q11 是更正/违约 cause 的开放分发语义，也非存档 profile 身份校验。本缺口不核销或改写这些条目；batch-042 曾将其作为未完成身份核实线索，当前证据收敛为上述窄边界。
- **大 A 语义：** 未发现 A 股撮合、T+1、申报单位或费用语义变化；恢复身份一致性不构成交易制度规则。无需引入交易所法源。是否登记独立 G 属根审计总账裁定，不在本只读复核中改总账。
- **必要性/范围：** 在身份契约明确前不提出代码修复。若决定必须全等，再在权威 restore validation 将 belief profile 与同账户已重建策略 profile 联系起来，并覆盖身份错配和合法个体 AnalysisProfile 保留；若允许分离，则应保留现状并准确说明双 profile 语义。不应约束合法跨账户类型能力或个体阈值，不涉及 OOP 新对象。
- 未改产品源码、正式文档或 Git；未运行测试、构建或回归。
