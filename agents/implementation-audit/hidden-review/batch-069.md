# 隐藏扫描批次 069

- 基线：`43b1aa5`；source root：`/data1/baiyifan/workplace/stock_market_game`；仅在指定 output root 写本批 Markdown/JSON。
- 方法：三项计划来源均逐份连续读取至 EOF，并核对实测行数和 SHA-256；阅读 `AGENTS.md`、`docs/principles.md`、最新 ADR-0028，并对照相关 ADR-0016/0026、审计总账、Strategy 模块调查及 43b1aa5 所对应的策略 owner/caller/consumer。未运行测试、构建或 Git 命令，未修改产品文件。

## 来源核对

| 来源 | EOF / 行数 / SHA-256 | 内容与复核 |
|---|---|---|
| `agents/oop-refactor-audit/exhaustive/reviews/engine-strategy-01-rereview.md` | 是；26 行；`44aeb5c585f1304419972d70861d9541eadd34a08fdd3d9907857d4c88b1ff0d` | 记录当时修订稿仍有四项阻断：测试调用方范围措辞矛盾、逐文件验证模板化、JSON action schema 缺字段、模块注释过时。结论“未通过”是针对那个修订稿版本，不能单独代表后来最终稿状态。 |
| `agents/oop-refactor-audit/exhaustive/reviews/engine-strategy-01.md` | 是；13 行；`751d20ba2dd2a6ecdbe39ee1e748bc98568b43127169e7b757e52319676d0fa7` | 最终入口记录后续 caller 与字段 binding 文件并给出通过结论；注明实际 action 仅 A01，缺陷/行为候选不混进 refactor。其结论受其引用的最终材料范围限定，不表示源码方案已经实施。 |
| `agents/oop-refactor-audit/exhaustive/reviews/engine-strategy-02-manager-final.md` | 是；27 行；`25045874a92a27bd84c0eab32b649e4b7edc0ec442ca40a3d15df0ff1fbce517` | 只做 strategy-02 最终增量复核；核实逐文件验证具体性、unit 归属与缺口诚实性、无 OOP action，测试仅静态定位。它明确继承既有复核，不应扩张为全 23 文件重新验证。 |

## 当前 owner / caller / consumer 对照

- `TradingPlan` 是单计划状态 owner；`PlanBook` 管理计划历史、标识与活动索引。字段最终 binding 精确列出 18 个现有状态字段，保护 Serde 字段名/反序列化接受范围，并保留“真实成交推进 filled_qty”的边界。没有证据要求增加独立计划服务。
- `NpcObservationContext` 是借用组合；生产调用为 `session/decision_chain.rs::run_chain_for_account`，传入完整 `MarketView` 与 `PublicLibrary`。策略消费端位于 `strategy/mod.rs`、`strategy/beliefs.rs`。旧审查对测试 caller 的范围限制仍需保留；最终 caller binding 只确认生产调用名称与构造参数，不声称穷尽测试调用点。
- `PublicLibrary` 持有公开报告/公告与派生索引；`from_parts` 恢复校验缺口属于独立行为契约线索，不是 owner 重构。策略-02 的 `BeliefBook` 持有个人机构政策及风险记忆；`InstitutionExperiencePolicy` 是每实例冻结参数，`StrategyFactory` 负责构造。策略 kernel、profile/data DTO、技术指标仍是纯函数或值契约。
- 组件引用显示策略调查边界与当前生产连接相符：个人信息消费走 `NpcObservationContext`，资金/持仓受理仍由 account/session/router 权威处理。未发现历史记录要求策略层改变 A 股成交制度或账户资产。

## G/Q 与历史结论反证

- 对照 `implementation-audit-2026-10-02.md` 的 G01–G68 总账，策略范围直接相关的是 G07（散户基本面分析和五路信号消费未接入）及 G08（散户 dated experience/衰减未接入）。历史 Strategy 主干已完成、机构 ADR-0026 个体经验已接入，均不能核销 G07/G08；本批三篇来源也没有把它们误报为已解决。其余 G 项属于宿主、界面、公司/结算或工具等独立路径，未见本批候选能直接核销的项。
- Q11 必须区分 `docs/open-questions.md` 中由 ADR-0026 明确的机构策略补充，与实现审计内部更正/违约 cause 路由问题；三篇来源没有混淆二者，也未试图借 OOP 调查替用户决定未决契约。
- 第一份复核的“未通过”与第二份策略-01最终入口的“通过”按时间/修订版本可以并存：最终入口声明引用 caller/fields binding，并继承此前管理复核。抽读两个 binding 后，发现 caller binding 修正真实函数名、范围受限；fields binding 列出的 18 字段及 Serde 边界与当前模块调查陈述一致。未发现足以推翻最终结论的新反证。需要注意，现有三份指定来源本身并未包含完整的 manager-delta / D01-D02 复核全文，本批只能确认引用链与摘要一致性，不能替代那些完整复核。
- ADR-0016 规定共享事实、个人判断、已获知信息隔离；ADR-0026 明确机构经验阈值属于可替换游戏假设，不是实证或交易所规则。ADR-0028 是当前最新 ADR，但其发布/Pages 决策与策略重构无关。检查范围不构成交易所规则重新取证。

## 门禁判断

1. **大 A 语义：通过（限本批历史方案核对）。** 金额仍以分、数量以股；策略候选不绕开路由器资金/股份约束，现行计划成交、T+1、整手与费用语义没有被历史方案改写。机构策略经验按 ADR-0026 标识为游戏假设。
2. **必要性与最小范围：通过。** Strategy-01 唯一 OOP action 为 A01，围绕 `TradingPlan` 写入口归属；strategy-02 actions 为空，保留现有对象、值契约和纯计算。行为缺陷线索不冒充重构。
3. **边界 / 跨层 / 复杂度：通过（证据有界）。** 生产 caller、状态 owner、consumer 分工吻合；`PublicLibrary::from_parts` 缺口、G07/G08 与测试未运行状态均未被对象化方案掩盖。测试调用方未穷尽，完整旧 delta 复核未在三份指定来源中复读，故不扩张结论。

结论：三份指定材料均完整核验。未发现可推翻后续最终结论的源码或语义反证；保留 G07/G08 等独立实现缺口。此结论不是实现完成或测试通过声明。
