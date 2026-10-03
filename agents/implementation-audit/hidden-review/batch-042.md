# 隐藏扫描批次 042

## 范围与读取证据

- 基线仓库为 `/data1/baiyifan/workplace/stock_market_game`；产品/caller 基线为 `.worktree/implementation-reaudit`，HEAD `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`，与清单 `43b1aa5` 相符。
- 三份来源按连续行段从首行读至 EOF；清单与实测行数、SHA-256 一致，均 `aliases=1`。完整章节矩阵和元数据见配套 `batch-042.json`。
- 必读守则及 `docs/principles.md` 已读。领域决策对照基线 ADR-0006（独立策略实例、取消共同 V 的后续实现指针）、ADR-0016（个人信息与估值）、ADR-0021（资金/费用/报价边界）、ADR-0025（日终存档契约）、ADR-0026（逐账户机构经历）。本批为静态对象归属审查，没有改交易制度，无需把历史材料中的官方规则叙述当作本轮法源依据。
- 当前缺口对照 `.worktree/implementation-reaudit/agents/implementation-audit/reaudit-engine.md`、`reaudit-foundations.md`、`implementation-audit-2026-10-02.md`、`exhaustive-review/sweep73.md`、`sweep74.md`、`luna26.md` 与最新 `sweep81.md`；同时对照现行 `docs/open-questions.md`。来源里的“建议测试/未运行”、reader 指令和候选重构都是历史证据，不是当前任务授权或验收结果。

| 来源 | EOF 及完整章节核对 | 主结论 |
|---|---|---|
| `agents/oop-refactor-audit/exhaustive/modules/engine-strategy-01.md` | 完整；248 行；SHA-256 `3a8bcea31cfa8033835fee479423c1cd63afb1dcbff2b72ec6cc0edc469947b9`。信息、计划分配/候选/簿/报价/修订/状态/紧迫度、测试、D01/D02、跨文件关系、限制各章已读。 | 既有 `NpcInformationState`、`PublicLibrary`、`PlanBook`、`TradingPlan` 分别持有状态；无状态计算与值契约继续是函数/DTO。计划域 A01 已在基线完成。独立 D01/D02 与 VersionOverflow 局部写入须按其各自证据审查，不归 OOP 搬迁。 |
| `agents/oop-refactor-audit/exhaustive/modules/engine-strategy-02.md` | 完整；343 行；SHA-256 `17e17cf8b54910d01cb9b0cf9c58224a4f60f831cb450bcc89e7a67c53ef700b`。计划验证、analysis profile、beliefs、strategy data/factory/profile、基本面事实/预测/更新/估值、Hot/Institution/Retail kernel、policy、Strategy API/实例/参数/身份、采样/sizing/state/technical、跨模块、限制各章已读。 | 状态归属于 `BeliefBook`、逐账户策略实例、policy 与 `StrategyState`；算法保持纯函数。恢复 identity、期限溢出、极端算术是行为契约候选，不能据未跑测试或“未来建议测试”判作 OOP 设计缺失。 |
| `agents/oop-refactor-audit/exhaustive/modules/engine-strategy-03.md` | 完整；46 行；SHA-256 `e8e34d31b03eb97ffe968219e0b7527bed2f0343bfd8e0a7e3b50ed4046a245b`。`value.rs`、`zi_noise.rs`、文件关系、阅读范围及限制均读至 EOF。 | 机构实例与散户实例各自持有逐账户参数；`target_cents`、零售 kernel、意图转换继续纯函数。ZiNoise 参数投影 A01 已落地并有直接投影测试，不是当前缺口。 |

## 基线 owner / caller 复核

- **计划归属已落地。** `TradingPlan` 状态字段在 `packages/engine/src/plans/state.rs:167-190` 限制在 `crate::plans`，同一 owner 提供 getter、review 和生命周期方法（`:193-297`）；`apply_revision` 是 `TradingPlan` receiver（`packages/engine/src/plans/revision.rs:216-258`），由 `PlanBook` 按标识调度并持有活动索引/原子批次（`packages/engine/src/plans/mod.rs:244-282,397`）。跨 session、执行和持久化读取点使用 getter；审计来源列出的 A01 已是 43b1aa5 状态，不应再报未实现。
- **ZiNoise 投影已落地。** `ZiNoiseStrategy::strategy_data` 在 `packages/engine/src/strategy/zi_noise.rs:40-52` 映射完整实例参数；三个决策入口使用该投影，直接逐参数测试在 `:296`。这只集中映射，不改变策略 kernel、RNG、存档或账户行为。当前 engine 复核也确认参数映射无遗漏（`reaudit-engine.md:96`）。
- **belief owner 与保存 caller。** 新 NPC 在 `packages/engine/src/session.rs:1901` 创建 `BeliefBook`，存档 DTO 由 session 汇集（`:2672`）；恢复校验位于 `packages/engine/src/session/persistence.rs:1104` 起，现有逐项检查在 `:1222-1234` 至少拒绝 `book.npc()` 与 map key 不一致并检查机构 policy。`BeliefBook` 的 state/cause owner 是 `strategy/beliefs.rs`，生产 cause caller 在 `session/decision_chain/roots.rs:438,456`。来源提到的 profile/strategy identity 交叉约束仍不能被“已核 map key”扩大解释为已校验全部身份元组；本批未完成所有序列化恢复变体的端到端可达性核实。
- **PublicLibrary owner。** 保存恢复入口为 `packages/engine/src/information/public_view.rs:151-203`，恢复时重建索引和 digest。现有 `next_seq == max_id + 1` 检查（`:186-201`）可拒绝尾端不衔接，但无法仅凭最大 ID 发现内部空号；例如 ID 0、2 与 `next_seq=3` 满足当前此项检查。因此 D01 中“严格连续性”部分仍是有代码依据的候选行为线索；不要把这一结论扩写成已证明所有跨类型重复、校正链路径都可由任意存档构造触发，本批未遍历全部字段组合。
- **期限查询与版本更新。** `TradingPlan::last_valid_trading_day`（`plans/state.rs:336-339`）直接执行 `created_trading_day + horizon - 1`；`PlanOpen` 建构会验证输入，但 `TradingPlan` 派生 Deserialize 且 `PlanBook::from_parts` 从存档 DTO 恢复 plans（`plans/mod.rs:173,244`），故只依赖开户校验不足以覆盖恢复/查询。版本溢出写入顺序见 `plans/revision.rs:221-258`：转移字段在 checked increment 前写入；该裸 receiver 失败语义已由既有复核明确保留（`sweep73.md:47-49`、`sweep74.md:50`、`luna72.md:22`），本批不把它伪装成新回归或静默归入原子批次承诺。
- **现有 G/Q 边界。** 最新 engine 总账明确 G06–G09、G16、G28、G35–G38 与 Q02/Q11 状态及 caller；本批没有证据可核销这些项。strategy 候选材料中的测试不足不等于 G 缺失：G07/G08/Q02 的真实 session 消费链属于既有总账问题；`zi_noise` 投影单测和 `StrategyData` DTO 也不能核销它们。D01、D02、belief identity 线索没有在已读 G/Q 台账中找到可直接等价的编号，保持独立候选，不强行映射。

## 发现、反证与范围

- **已完成、关闭重复报告：** plans A01 的字段写口封装和 receiver transition 在产品基线已存在；`strategy_data` A01 也已存在并有参数投影断言。计划活动索引/批事务归 `PlanBook`，交易事实仍来自成交/账户交易层。无必要再抽 service/wrapper。
- **D01：恢复 ID 连续性候选保留。** `PublicLibrary::from_parts` 当前检查 id 小于 `next_seq`、重建索引并比较 `max+1`，但不检查每个连续序号均存在。注释明确承诺“无空隙”与实现的最大值检查之间存在可复现的契约差。归属 owner 是 `PublicLibrary`，caller 是 session 的公共资料库存档恢复链；这是独立存档一致性行为候选，不是 OOP 动作、已批准缺口或 A 股规则。跨类型 ID 唯一性/完整更正链按来源描述保留待核，未据现有只读证据扩大断言。
- **D02：`last_valid_trading_day` 全路径溢出候选保留。** 直接加法可能溢出，且恢复能绕过 `PlanOpen`。setter 可见性收窄与 receiver 方法不会解决字段反序列化和 query 错误通道问题。归属 `TradingPlan`/`PlanBook` restore-query 边界；没有找到对应现行 G/Q，不新立号。
- **belief identity 候选限缩。** 当前保存校验确实检查 belief map key 与 `book.npc()`；不能再把这部分笼统描述为缺失。profile 与 account/strategy family 的跨对象一致性目标在本批没有追完每条构造/恢复路径，来源自身也排除 provenance 争议。因此仅保留“尚未证明”而非判定已复现的新缺陷；暂无新增候选、G/Q 关联或代码修改依据。
- **无 A 股语义变更。** 这些材料讨论的是对象所有权、值 DTO、存档和策略算法。金额分、数量股、账户独立、持仓/费用/成交由权威交易层处理，与 ADR-0006/0016/0021/0026 边界相符；没有扩展交易规则或新增大 A 规则主张。

## 结论

完整来源支持的主要归属判断与当前实现相符。没有新的 OOP 遗漏，也没有可据此新增 G/Q。当前保留 D01 的序号连续性疑点、D02 的期限算术/恢复路径疑点；belief profile identity 仅列为未完成核实。VersionOverflow 局部修改是既有明确范围，不作为本批新候选。未改产品源码、正式文档或 Git；未运行测试、构建或回归。
