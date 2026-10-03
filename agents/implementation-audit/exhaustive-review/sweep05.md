# sweep05：Strategy 与机构个人经历逐章审计

## 范围与方法

- 生产源码基线：`b76ece3`。读取工作树 HEAD 为 `4ad5a2e`；主审确认这是合入 `b76ece3` 的审计分支，生产源码相同。
- 已读根 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`、`docs/architecture.md`，未发现本目录额外 `AGENTS.md`。
- 以下三份文件从首行连续读到 EOF，总计 426 行；章节表包含背景、备选、研究依据与边界，不以搜索 TODO 或任务勾选替代阅读。
- 追踪实际工厂、Session 装配、root 观察、五路候选、计划生命周期、预算、报价、结算经历投影与存读档校验。没有修改产品、运行测试或执行 Git 写操作。
- 对照总账 G07/G08/G38；本分片没有确认新的独立代码遗漏。以下“已有”只表示所列承诺有生产实现，不声称全系统或长期验收完成。

## ADR-0006（244 行，已读至 EOF）

| 原文章节/起始行 | 逐章状态与生产证据 |
|---|---|
| 状态、覆盖说明 1；上下文 14 | accepted 的 Strategy 与个人账户语义仍有效；共同 V 与投资者资金循环已分别由文末及 ADR-0024 取代，不能按旧要求重开缺口。`strategy/mod.rs:95` 的公共 MarketView 无 V；`strategy/factory.rs:113` 为逐实例机构构造。 |
| §1 独立模块 37 | 已有：`packages/engine/src/strategy/mod.rs:7` 分模块，调用侧在 Session；无 UI/I/O 依赖。 |
| §2 trait/意图/RNG 42 | 已有：`strategy/mod.rs:126` LimitPrice、`:135` Intent、`:169` Rng、`:194` Strategy；多股 Vec 意图替代旧单个 Intent/Pass。实际消费 `session/pipeline/npc_decisions.rs:172` 重建策略，`:179` 注入独立 RNG 后调用。隐藏 V 可见性属于已被取代的历史要求。 |
| §3 个体参数 57 | 已有：`strategy/factory.rs:48` 逐散户采样风格/阈值/订单量/步幅，`:113` 逐机构风格与观察节奏，`:148` 逐游资数量/量能。`session.rs:1913` 使用独立策略 policy 流一次采样 InstitutionExperiencePolicy。原文 N(μ,σ²) 为示例，不能把实际范围采样算遗漏。 |
| §4 扩展 63 | 现有三生产实现与工厂注册已齐；`strategy/state.rs:6` sealed 注册及 `:93` StrategyState 必须随新策略扩展，这是存档权威契约后的架构澄清，不是现有策略失效。对应总账 R04，不重复列代码缺口。 |
| §5 V/TrackV 68 | 被明确取代：文末 231–244；当前信念估值来自本人获知材料。`session/decision_chain/roots.rs:403` 建本人 NpcObservationContext，`:437` apply_cause；`strategy/fundamental/update.rs:101` 只取该 context 的报告。 |
| §6 玩家 77 | 已有：`strategy/factory.rs:166` Player 返回 None；NPC trait 不包裹玩家。 |
| §7 首批策略 83 | ZI、Momentum、BeliefInstitution 三生产实现仍在 `strategy/state.rs:93`。旧 Value/TrackV/隐藏 V 已被个人信念与计划链取代，旧 cancel_rate/holding_horizon 字段骨架不能绕过后续工作报价与计划期限契约重新要求实现。 |
| §8 人口/风格 91 | 默认人口已在 `apps/web/src/config/defaults.ts:83` 为 20000/5/2；个体风格 `strategy/factory.rs:54`、`:115`、`:154`。发现原文 97–99 “积极交易机构用动量族和普通工作报价”是旧描述：当前 `factory.rs:136` 明确所有机构风格走计划，ActiveTrader 零基本面、当日期限 `decision_chain/lifecycle.rs:518`、`:534`。这是旧策略被取代后的文档漂移，不列新代码缺口。 |
| 研究基础 105；备选 112 | 研究/历史选择，无额外当前实施授权；备选 D 的共同 V 已由后续明确决策覆盖。 |
| 后果 119；关联 137 | 实际稀疏个人注意力实现 `session/attention.rs:63` 最大概率、`:73` thinning、`:84` 几何等待；三实现/工厂已经存在；不把删除的 V 随机参数重新当待办。 |
| 下跌价格发现 143 | 散户实际 Highest/Lowest 主动报价 `strategy/retail.rs:57`、`:76`、`:113`，后续 ADR-0022 覆盖旧对手一档固定报价。机构旧四分之一试探与 Fixed/DriftUp 独立内核保留 `strategy/sizing.rs:6`、`strategy/value.rs:16`；生产改用个人信念计划链，不能按旧内核核销或重开公司链遗漏。A 股数量/费用守卫 `strategy/sizing.rs:16`、`:32`；订单合法性仍由交易层执行。 |
| 个体阈值与量价 167 | 工厂阈值/数量采样已有；相对量能由已完成至多20日真实日量与日内曲线生成 `session/views.rs:41`–`:65`，五档失衡 `:69`–`:85`。默认人口已有。旧共同 V 偏差与未来资金循环为被取代要求。 |
| 生命周期/风险/注意力 190 | 工作报价在生产对齐链处理；当前无公共仓位/现金保留 `plans/allocation.rs:39` 可用量直接为 cash−frozen_cash，`strategy/sizing.rs:32` 使用实际含费整手；注意力参数/独立 RNG/候选 tick 在 `session/attention.rs:14`，实时 thinning 在 `:73`，完整几何尾部在 `:84`。ADR-0021 的步幅取代公共上限。已存在的 G07/G08 不能由这些旧行为能力核销。 |
| 共同 V 取代更新 231 | 指针主干已有，个人获知→信念→五路信号→计划生产链见 `decision_chain/roots.rs:340`、`:400`、`:464`、`:467`、`:478`。但“NPC”跨身份能力未全接：G07 保持。 |

## ADR-0021（111 行，已读至 EOF）

| 原文章节/起始行 | 逐章状态与生产证据 |
|---|---|
| 状态/修订 1；用户决定 8 | 不再公共限制单股仓位或补钱；实际根候选目标取个人步幅，`decision_chain/lifecycle.rs:90` CandidateTargetProposal；真实冻结/T+1/费用由交易层执行。 |
| 仓位目标与实际订单 18 | 步幅取 `strategy/factory.rs:143`；`lifecycle.rs:90` 计算目标。含费买量 `strategy/sizing.rs:32`；软预算 `plans/allocation.rs:45`、`:76` 不预支卖单收入、不保留固定权益比例，不足手续费分配零预算并继续其他请求。原公共字段 `max_stock_fraction` 在本次检查的 strategy/plans/decision_chain 生产目录无残留。G38 是机会分类漏接，不能据预算算法已实现核销。 |
| 主动/等待 38；用户意图 40 | `strategy/retail.rs:113` 和 `strategy/hot.rs:35`、`:49` 选择 Highest/Lowest，固定限价与符号价共存 `strategy/mod.rs:126`。无需把 NPC 统一改 PlaceMarket。 |
| 实施边界 54 | 原文显式注明 ADR-0022 覆盖固定报价；当前视图合法范围 `session/views.rs:14`、`:99`、`:104`，连续竞价才套 cage，`config.rs:91` 必填开关。机构保护范围与交易带交集在 `decision_chain/quote.rs:122`、`:133`；资金按最终报价在 `:198` 再核算。不能按原文“没有动态最高/最低类型”认定当前缺失。 |
| 验证边界 88 | 股票数不会进入目标个人步幅、费用与冻结已有；Rust/Web 当前契约已有。极致缩量行情理想形态原文明确不作已完成承诺，属于实验与验收证据，不新增硬编码价格曲线需求。 |
| 交易规则依据 99 | 现行官方规则已列来源与2026-07-06适用日期；统一 PlaceMarket 的沪深简化原文已明示。本轮只审计文档→代码，不改变制度或伪称进行了新的联网规则复核。 |

## ADR-0026（71 行，已读至 EOF）

| 原文章节/起始行 | 逐章状态与生产证据 |
|---|---|
| 状态/范围 1；用户决定 7 | 个体成本响应/风险暂停并非强制卖出；实际 `session/institutional_behavior.rs:103` 成本候选，`:121` 半门槛迟滞，`:143` 真实买入身份与不利选择；`decision_chain/urgency.rs:152` 只暂停买计划。恢复本人观察≥2000bp在 `lifecycle.rs:289` 调 assess_recovery；观察 root 在 `roots.rs:198`。 |
| 游戏假设 20：逐风格表/抽样 | `strategy/institution_experience_policy.rs:140` 冻结逐实例采样，两种 loss_response 各有机会；`session.rs:1913` 生产装配；`strategy/beliefs.rs:97` 存档保留 policy。恢复不重抽，缺字段拒绝。数值属于游戏假设，不能暗称实证。 |
| 游戏假设 35：候选 | `institutional_behavior.rs:103` 成本动作；通过 `roots.rs:174` 进入五路 CandidateSignals 后在 `:95` 混合；无正成本明确 MissingObservation，不直接下单。 |
| 游戏假设 41：失败衰减/费用/信心 | 机构已接 dated writer：`pipeline/retail_projection.rs:529` InstitutionalFacts，`:597` record_institutional_fill_dated；失败数减距最后失败20日档 `strategy/beliefs.rs:196`。实际费用求和 `retail_projection.rs:530`、累计收费与净盈利 `:560`–`:578`。root 本人观察 `roots.rs:283` 写真实不利事件，`:465` 消费；`decision_chain.rs:172` 合并历史事件排序、逐订单去重，`:224` −1000/+500；`fundamental/update.rs:83` 只改信心与 cause，不改估值/预测。收据 identity 幂等在 `retail_projection.rs:347`。不得拿机构这些现成能力核销散户 G08。 |
| 二次补漏 50：账户风险记忆 | `strategy/beliefs.rs:99` 必填 bool，`:161` 仅本人观察更新，`:209` 触发/恢复半门槛；root `roots.rs:297` 调用。报价读取 `institutional_behavior.rs:63`；`urgency.rs:152` 应用买入暂停。计划状态消失不删除信念簿 bool；Web `save/schema/personal/beliefs.ts:35` 精确字段并用 boolean 校验，Rust `session/persistence.rs:1379` 信心≤10000。 |
| 二次补漏 60：升级/价格交集/资金 | `lifecycle.rs:253` 允许已暂停计划升级 RiskPressure；`quote.rs:31`–`:45` 尊重不可撤窗口；`:133` 保护限价与合法带输入；`:198` 含费缩量，`:210` 资金不足 Keep/Wait。状态恢复由本人 lifecycle 复核，并非每次报价自动清除账户暂停。 |
| 边界 65 | 是行为游戏模型，不新改撮合、交易单位、T+1、费用/价格带，不恢复旧共同仓位限额或注资。日终公共保存边界由ADR-0025及对应分片复核；本轮未用内部快照方法推翻此边界，也未冒称长验收完成。 |

## 对照已登记缺口与额外候选排除

1. **G07 保留。** `session/decision_chain.rs:606` 只有 belief_chain_params 能力进入信念 root；`strategy/factory.rs:48` Retail 仍构造 ZiNoise；`pipeline/npc_decisions.rs:179` 实际 Retail 消费为 decide_with_experience。工厂默认五路权重包含 Retail（`strategy/factory_profiles.rs:21`），不等于消费已接。
2. **G08 保留。** `pipeline/decision_snapshot_capture.rs:347` 调 legacy observe_position，`pipeline/retail_projection.rs:519` Retail 模式调 record_fill_with_order；机构 InstitutionalFacts 分支则明确 dated。不能把纯 dated 函数或机构日期链当散户已实现。
3. **G38 保留。** 两处生产 AllocationRequest 在 `session/decision_chain.rs:972`、`:1236` 均 ExistingPlan，而 `plans/allocation.rs:123` 明确支持 NewOpportunity 优先级。
4. **不新增“sealed 禁止扩展”缺口。** 新生产策略确需注册 state/serde/sealed，当前原文“只加 struct+工厂”的零侵入表述宜澄清；与已登记 R04 架构澄清相同，不是功能尚未实现。
5. **不新增“ActiveTrader 未持个人风险经历”缺口。** 当前所有机构统一进入信念计划链；零基本面权重不强制估值，个人 policy、dated 经历及账户暂停仍装配。旧 ADR0006:97 与生产链不符，属于旧策略描述已被取代。
6. **不新增“历史事件日期被改写”缺口。** `decision_chain.rs:201` 确实按历史时刻排序处理；apply_experience 的 CauseRecord.as_of_trading_day 记录本人本次复核日期（`fundamental/update.rs:84`），原真实 ExperienceMoment 历史未删未改。原文没有要求 cause 的复核日期伪装成成交日期。
7. **不新增“AllocationExperience 默认值”缺口。** 上游信心已消费真实机构事件；重新在预算层叠扣 failure_influence 会重复处罚。与总账 G38 限定一致。

本分片静态审计证据不足以宣布行为统计校准、长期流动性、三宿主或全量回归通过；无测试执行结果。
