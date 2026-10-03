# S74：Session 生命周期、持久化快照、计划策略后续记录全文复核

## 范围与阅读

2026-10-03。连续全文读取 `agents/oop-refactor-implementation/session/lifecycle.md` 54 行、`persistence-snapshot.md` 43 行、`plans-strategy.md` 60 行，共 157 行。基线为 `b76ece3` 对应产品源码，审计merge只增加工作文档。本轮只新增本文件，没有产品改动、Git写入、测试或构建。

按照正式公司计划与 ADR0011/0015/0017/0019/0025/0026 的现行语义核对；历史OOP记录只承诺迁移原owner/caller及指定保护，不自动授权修复旧业务行为。三文引用的旧action-index在当前树缺席，记录具体动作的当前owner和实际caller作为可核查证据。另读后续 `session/implementation.md` 及 lifecycle/leaf review 的关闭段落，避免只读早期“等待review/集中编译”就认定今日仍待实现。

## lifecycle 全章矩阵

| 原文位置/条款 | 当前caller → owner → consumer | 状态 |
|---|---|---|
| :3–5 范围/规则来源/无新交易制度 | 对应四动作及正式ADR，不新增派钱/冻结或交易类别 | 过程边界；旧适用日期不等于本轮重新联网核验。 |
| :11 N01母单真实child生命周期 | 连续 `execution/records.rs:17/35` → ParentOrderPlan receiver（execution.rs:163/180/217）→ pending PlanBook事实；竞价 `auction_day_end.rs:1847/1957/2076` → checked receiver | 连续/竞价两生产caller都接；不是只完成连续侧。filled来自实际结算，不把submission计成交。 |
| :12 N02协议rollback/cursor | 三宿主ProtocolSession → `protocol/civil/session.rs:9/34` ProtocolState/PublicationFactCursor → checkpoint:73/rollback:79与发布facts | 同tick cursor与完整回滚集合已存在，测试flags在外层:61–64，不入checkpoint。 |
| :13 A03 candle owner | market成交更新→`candles.rs:200` SessionCandleBook.record_trade_or_mark→commit_active:203；竞价日终:2167→归档 | histories/active确实由同owner持有；save/hash/snapshot仍分别投影原字段。 |
| :14 R2N01注意力heap | Session enqueue/current candidate只读→`attention.rs:263` NpcAttentionScheduler→pop_due匹配tick/排序去重 | 真实owner、恢复与hash consumer都有；到期不是全体实际观察。 |
| :16外部owner与DTO读取 | session state、getter与DTO不同边界 | 不把当时跨组交接当未实现事实；下面追具体caller。 |
| :20 Candle API/checked/零占位 | `candles.rs:132/181`记录/commit；hash.rs:108–109取双投影；snapshot/save取histories/active | 原真实首笔替代占位、trade_stats/完整历史语义保留。 |
| :21 Scheduler API/hash/restore | scheduler enqueue/iter/fromiterator/pop_due；hash.rs:118排序取队列 | 不额外持有NPC RNG或复制权威candidate tick；过期条目过滤已有方法。 |
| :22–24 parent配对/不同错误边界/transition | execution.rs:140 replace_child_facts、145 restore剩余数量、225/241/268 checked方法；records及auctioncaller消费linked_plan_id | 两Option恢复接受范围/真实child关联有owner；不提前安装候选或新加校验改旧首错。 |
| :25 diagnostics fill | records.rs:17→CausalCollector::record_continuous_fill | 实际call；此项不核销DEV根诊断G37空events。 |
| :27–29保持语义/存档/flags/空cursor | from_game（protocol/civil/session.rs:154–168）restore新cursor；checkpoint:19–27同组clone；历史使用AppendOnlyHistory | 内存checkpoint不是公共日内save，ADR0025边界保持。 |
| :31–48短测filters/集中门禁/格式 | Parent/candle/scheduler/protocol测试存在；后续implementation:36–44登记集中build/定向case | 工作者没跑不等于根agent后来没有跑；本轮没有重新认证日志或运行case。 |
| :50–52 L1/L2修正 | execution.rs:140实际paired writer；continuation_tests:768 getter读取实际存在 | lifecycle-review:21–22已再次静态关闭；不重列旧编译阻断。 |
| :54 L3未关联母单反向短测 | execution.rs:399 `unlinked_parent_reverse_rebuilds_execution_from_the_new_target` →生产adapter+typed成功fill fixture | 测试实际存在，review:23关闭、implementation:36有增量运行记录；不是完整撮合/结算复现。 |

## persistence-snapshot 全章矩阵

| 原文位置/条款 | 当前caller → owner → consumer | 状态 |
|---|---|---|
| :3四动作/optional范围 | 开局、校验、fee audit、snapshot临时owner | 不新增格式、迁移器、交易账本。 |
| :7 OpeningFigures | company_assembly.rs:74→OpeningFigures:131/141→registry_opening_lines/industrial_opening_lines→公司前史 | code+total_shares精确默认命中，未命中按总股本推工商；这不核销G36四行业自定义会话。极小股本不平衡显式失败是已有接受集。 |
| :8 SaveValidationContext | persistence.rs:271 derive clock→:479 context→:1101 personal / :1409 plan校验→GameSession恢复 | 只读context/派生集合实际使用；账户恢复后的订单校验独立，不混造另一个authority。 |
| :9 CumulativeFeeAuditV2 | persistence/v2.rs:630构造→:635 validate→外层含key错误；owner:646/679 | Result<bool>实际consumer保留key上下文；买方==nominal、卖方分量及成交额上界明确。不能从累计费重建逐笔路径。 |
| :10 LiveOrderReservations | snapshot.rs:131一实例→continuous:133/auction:145 record_order→take_for:102→AccountSnap预留 | 两簿同实例汇总，真实remaining；无单账户0是合法结果，不是异常fallback。 |
| :11state/candle/DTO | snapshot/save/hash读取candlebook；SaveSlot/JSON仍原双字段 | 已接owner，不增加persisted candle owner字段或旧档迁移。 |
| :13–17领域依据/收费/可编辑save | 当前validator拒绝结构非法，恢复策略getter/receiver；不核验资产历史来源 | 正式ADR0019/0025优先，正常公共日终档无活动冻结/委托，不重开日内挂单存档义务。 |
| :19–29测试交接 | company_assembly:444起、persistence context tests、v2_tests:1316起、snapshot:307起owner保护 | 代码存在；后续总实施记录有精确短测与类型检查。未运行完整套件的推荐不冒称全回归。 |
| :31–36worker仅fmt/diff/等待门禁 | implementation:36–44后续根集中验收与leaf review | 已有后续，保留历史限制但不按旧时点新增G。 |
| :38–40poison路径修复 | v2_tests:1301实际`session.poison`；GameSession facade保留poison | 原误迁state.poison已修；不是存档丢失poison的功能缺口。 |
| :41 TradingPlan getters | persistence:1409验证plan/linked getters→原错误分支 | 外部raw读已迁移；DTO本身字段访问合法，不机械改所有字段。 |
| :42 restore_strategy | persistence/v2.rs:436→Account::restore_strategy；Snapshot/Account getters | 实际恢复caller存在；可编辑DTO现金仍可修改，无历史证明要求。 |
| :43修复后仍待review | leaf-review:49及implementation:21/38后续关闭/构建 | 早期交接已被后续承接，不当当前未实现。 |

## plans-strategy 全章矩阵

| 原文位置/条款 | 当前caller → owner → consumer | 状态 |
|---|---|---|
| :3/7–8 TradingPlan字段/五transition/PlanBook批事务 | plans/state.rs getters→revision.rs receiver→PlanBook原子apply；外部lifecycle/persistence消费 | 实际写口归计划，PlanBook仍拥有索引/批次；单个direct revision旧VersionOverflow失败面不升级为全方法新事务承诺。 |
| :9 review_preview | decision_chain/lifecycle.rs:417→state.rs:267克隆预览→urgency评估 | getter只读，clone覆盖direction/confidence不是新修订事件；生产caller确实已迁。 |
| :10 ZiNoise完整投影 | zi_noise.rs:151/191/234→private strategy_data:42→StrategyData/原decide路径 | 全参数投影helper真实被三个入口使用，RNG与serde不因抽取改变。 |
| :11 BeliefBook风险owner | roots.rs:297→institutional_behavior.rs:8/13 adapter→beliefs.rs:161/172→冻结policy/experience/latch | owner真实更新latch；无peak保留、真实失败20日影响衰减、>=触发/>恢复遵ADR0026。 |
| :12 RetailDecisionContext | retail.rs追势抽样后的from_selected_stock:160→classify_chase:192→原Intent/sizing | pure context不使用RNG；仍以可卖/T+1/实际资金限制数量，噪声卖出另选股票。 |
| :13重导出/visibility | CandidateTargetProposal在plans/candidates及mod重导出；root调用session assess_institution_behavior | 类型接线已存在，不要求保留未使用的迁移包装器。 |
| :15–22行为/0 stop/单位/接受集 | retail.rs:167–179个人cost/loss/阈值；原零stop和无cost语义注明保留 | OOP没有授权改旧行为；不将本记录“保留”反向解释为零stop修复待办。 |
| :24–36测试/未独立运行/集中root | owner_tests/risk_owner_tests/retail decision_context tests存在；implementation:36–44后续统一结果 | 测试推荐与定向实际结果分开，本轮未运行。 |
| :38–42外部getter/preview/risk迁移 | lifecycle:417、roots:297与persistence:1409为当前caller | 已承接；旧session helper留薄委托不是第二份风险模型。 |
| :44–50十个caller/state迁移 | implementation:23登记caller-review完整diff/旧case断言保持，当前文件仍存在 | 不据语义相同getter移交再造业务；完整diff复核属于历史证据，不由本次源码搜索再认证所有断言。 |
| :52–56 N07 adapter/非法fixture | institutional_behavior:23→observe_institution_position_dated，仍输入policy个人threshold；failure fixture保留极端价格输入 | 机构真实观察已接，G08散户dated仍不因此核销。 |
| :58–60 ZiNoise补直接投影测 | zi_noise.rs:296 `strategy_data_projects_all_individual_retail_parameters`逐参数/to_bits，leaf-review:15初发现→后续关闭 | 原缺测试已补；不能只读“待review”忽略implementation:21最终再次复核。 |

## 残余、反证与后续证据

- **L1私有paired writer缺失/L2测试raw getter漏迁/L3反向替换无短测**：当前定义和短测均有，lifecycle-review明确逐项关闭；无新产品G。
- **poison误塞state/计划私有字段恢复漏读/ZiNoise没测完整投影**：当前路径和非默认/to_bits测试有，leaf-review最终关闭；不沿用早期编译失败。
- **三文说“集中运行待安排”意味着当前未编译/未短测**：`session/implementation.md:36–44`已追加build07/default09/final10–12及代表性case结果，包含沙箱EPERM后保留原断言重跑说明；不篡改初失败，不据短验收声称完整回归。
- **CandleBook/注意力owner无外部call**：真实成交更新/日终commit/hash/save/恢复及候选heap caller均存在；完整历史未按条数截断。单体CandleBook owner不等于前端增量分时G10/G11都正确。
- **内部checkpoint保存日内活跃状态违反ADR0025**：ProtocolState:19–27是未发布批次内存回滚；公共restore:134–140拒绝活动委托，public save由日终候选提供。两者不同范围，核销该假候选。
- **snapshot预留0掩盖错误**：LiveOrderReservations::take_for移出账户聚合后没有订单返回0，原合法状态；checked累积仍显式expect。没有确认非法资金被default吞掉的生产链。
- **累计收费audit只允许卖方nominal唯一值**：正式卖方实收封顶/路径依赖允许分量小于nominal；validate:687–693保留范围，bool外层附key错误。不得恢复错误的唯一nominal约束或要求编辑存档证明交易来源。
- **新risk owner/有限context核销散户五路与日期衰减**：不成立。G07/G08是完整策略装配与Retail事实写入的更细生产义务，OOP保原行为的局部完成不核销它们；机构ADR0026已实现部分不重复提出。
- **snapshot/candle/context抽取核销所有公司初始化/估值**：不成立。G06/G09/G28/G35/G36等原总账范围保持；OpeningFigures仍只生产工商，其余非默认公司并未因owner抽取而新增。
- action-index历史链接缺席保留为导航/证据适用限制，本轮不据其缺失制造新游戏需求；未认证后续日志全部原始内容和源身份。

结论：157行内具名OOP动作、跨组caller交接及早期有效发现均已在当前代码找到承接；未确认独立于现有总账的新产品漏实现。正式需求的局部断链与长期/真实三宿主验收仍按现行总账登记，不以这些重构记录的“完成”覆盖。
