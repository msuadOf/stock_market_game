# sweep20：三策略设计、稀疏盘口反馈与 Escrow 历史计划复核

审计日期：2026-10-03。生产基线 `b76ece3`；读取工作树 HEAD 为 `4ad5a2e`，后者为审计 merge，生产代码相同。本轮只新增本记录，未修改产品、测试或 Git 状态，未运行构建、测试或长验收。

## 阅读范围与判定依据

已连续阅读全部原文：

| 文档 | 全文行数 | 本轮覆盖 |
| --- | ---: | --- |
| `docs/superpowers/specs/2026-06-29-strategy-impl-design.md` | 183 | §1–11 全部，含类型、三策略、工厂、错误处理、边界、测试与 DoD |
| `docs/superpowers/plans/2026-09-26-sparse-continuous-book-feedback.md` | 30 | 当前判断、三项代码范围、验收及实测限制 |
| `.omo/plans/escrow-parallel-engine.md` | 362 | Scope、Verification、Runtime、组织规则、任务1–12、CivilUpdate、F1–F4、Commit与Success criteria |

同时读根 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`，核对架构分层与 ADR-0017 的后续修订。上述目录下未发现额外 `AGENTS.md`。历史计划第3–7行明确核销固定来源类序、跨 worker 整局相同及退役语料要求；第303–308行保留用户终止 Task9/10/F1–F4 的历史事实。以下“已有”表示静态代码及测试断言存在，不表示本轮运行通过。

## 三策略设计逐章台账

| 原文位置/任务族 | 当前状态 | 代码证据与限定 |
| --- | --- | --- |
| §1–3，10–78行：多股视图、带 code 的 Intent、空 Vec pass、注入 RNG | 已有；类型按后续规则演进 | `packages/engine/src/strategy/mod.rs:72` 的 StockView、`:98` 的 MarketView、`:108` 的 SelfView、`:135` 的 Intent；多股 map、Money、可卖股份与显式 Symbolic LimitPrice 均在现行接口。 |
| §2/4，22/87–97行：机构可见共同 V、Value/TrackV | 被批准替代 | `packages/engine/src/strategy/value.rs:1` 明示共同 V 已删除；`:29` 的 TargetPolicy 只保留 Fixed/DriftUp；`:39` 的 BeliefInstitutionStrategy 由个人 BeliefBook 与五路决策链驱动。不能要求恢复 TrackV 或共同 V。个人信息与分析链缺口仍分别见既有 G07/G09。 |
| §4，81–86行：ZiNoise 到达、追势、个体参数 | 已有，报价行为按后续 ADR 改变 | `strategy/zi_noise.rs:40` 将全部个体参数映射到 StrategyData，`:225` 决策交给 retail 内核；`:265` 与 `:283` 使用 Highest/Lowest。本轮未发现此映射遗漏。旧 best_bid+tick/best_ask−tick 不是现行必须恢复的报价。 |
| §4，98–103行：Momentum 多股趋势与可卖股份 | 已有；时间单位被批准替代 | `strategy/momentum.rs:127` 调统一 hot 内核；`strategy/mod.rs:85` 明确 complete market-minute 历史，tick recent_prices 不再充当游资趋势时间。holding_horizon 原文已明确简化，不登记主动按持仓时长平仓的漏实现。 |
| §5，105–125行：工厂、每实例参数、Player无策略、RNG | 主干已有；参数拒绝有新增候选 | `strategy/factory.rs:38` 按个体序号与 RNG 构造，`:148` Hot、`:166` Player→None；公共工厂非法参数见后文 S20-C01。 |
| §6，127–131行：不可行 Intent 记录拒单、非法参数显式拒绝 | 拒单路径已有；构造边界部分遗漏 | 生产 `pipeline/ready_stock_stream.rs:154` 处理 P3 outcome，`:69` 消费 P4 typed facts；`continuous_tick_transaction.rs:245` 将 P3拒单事实送最终输出。非法策略参数的公共 API 见 S20-C01。 |
| §7，133–138行：纯策略与会话/account/market分层 | 已有 | Strategy只产生Intent；P3账户资源、P4价格与撮合分别由 `account_validation_driver.rs`、`price_resolution.rs:9`、`continuous_matching.rs` 执行。 |
| §8，140–152行：构造/serde/策略/工厂/非法参数测试 | 原任务族测试已有；新边界未覆盖 | `packages/engine/tests/strategy.rs:2083` Player、`:2093` 各类工厂、`:2113` 非法参数仅测 Retail arrival_rate=-0.1；`strategy_state.rs:117` 非有限 wire state 拒绝不覆盖公开构造器/工厂。 |
| §9，154–166行：同模块布局 | 经实际规模拆分，非缺口 | `strategy/mod.rs:7` 注册细分模块并公开导出；不要求合回历史单文件。 |
| §10，168–175行：cargo/clippy/build、导出、纯逻辑 | 符号与实现存在，验收证据另列 | 公共导出位于 `strategy/mod.rs:25`；原文“无 f64 存储”与原文参数 struct 本身不一致，当前可恢复策略参数用 exact_float 保存，Money仍为整数，不能仅见 f64 参数就登记金额精度缺口。未重跑原文命令。 |
| §11，177–183行：旧 account 构造适配、引用边界、DriftUp计数 | 旧签名已演进，市场时间取代调用次数 | `strategy/value.rs:17` DriftUp 读取 elapsed_market_minutes；无恢复 decide-call tick 计数的现行要求。 |

## 稀疏反馈：三项代码范围与公开/本人反馈边界

1. 原文第12行身份索引已实现。`packages/engine/src/orderbook/book_state.rs:10` 同时维护 bids/asks/live_by_id/filled_orders；`:50` 插入，`:81` 撤单，`:117` 部分/全部成交，`:165` 清空。价格时间键仍是盘口键，身份索引只定位有效挂单。`orderbook/state_contract_tests.rs:18` 覆盖部分/全填/撤单/clone 与 FIFO；存档恢复走 `market.rs:358` 的 restore_resting_orders。
2. 原文第13行增量反馈已实现。`market.rs:368` 生成 MarketDelta（前后 latest price、book变化、序号）；`:389` 校验候选版本后 apply_changes。`pipeline/incremental_continuous_stock_shadow.rs:314` 产 market_delta；`pipeline/adaptive_plan_chain.rs:705` 先核消费身份，`:712` 通过 take 保证 delta 只消费一次，`:717` 校验应用到候选盘口。失败返回类型化错误，未看到静默退回整簿复制。
3. 原文第14行连续/盘前共用反馈已实现。连续 `continuous_tick_transaction.rs:227` 与盘前 `pre_open_transaction.rs:236` 都驱动 stock_stream，再由 ReadyStockStream 消费 typed结果与增量 projection。盘前 `:257` 只收尾一次并在 `:276` 推进静默时钟；集合竞价由 auction_progress/独立finalizer保持自身规则。
4. 公开观察与本人执行事实保持区分。`decision_snapshot_capture.rs:101` 固定市场观察；`adaptive_plan_chain.rs:181` capture root 决策观察，`:700` 更新候选执行簿；`ready_stock_stream.rs:69` 以 candidate身份定位 outcome 后推进对应计划，`:191` 只登记未完成的 `(owner,stock)`。测试 `adaptive_plan_chain_tests.rs:96` 断言同股其他账户请求不阻塞本人的 root；`:300` 核对旧子单被他人部分成交后重新检查本人剩余量。不能把候选执行簿包含完整盘口误判为“公开行情每轮重新做策略决策”。
5. 保留真实报价与续行：`price_resolution.rs:9` 在权威受理点解析 Symbolic LimitPrice，`:41` 拒绝超出 P3预留的价格；`local_admission.rs:198` 建同账户同股票 Cancel→Place依赖。`adaptive_plan_chain_tests.rs:173/223/651/656/661` 分别覆盖两撤下新、撤旧换新、即时全填/部分填、第一/第二撤失败不生成后继；`:997` 重复事实身份拒绝。测试存在，不报告本轮通过。
6. 原文第16–30行要求性能证据并限制结论。文件已如实记录1/8 worker单样本且数量不同；代码存在 Rayon 不能证明稳定收益。该项属于后续同条件实际测量，不新增“缺少另一套会话线程池”，也不把稀疏反馈已经完成等同 G16 全历史计划复制已经解决。

## Escrow 任务族核销与实际工具

| 原文任务/位置 | 当前静态状态与边界 |
| --- | --- |
| Scope/Runtime，29–76行；任务1/2，167–203行 | 生产 shadow/一次资源截点/依赖续行主干已有；全局来源排序、全局 canonical收据跨订单交易优先及禁止所有临时channel已被 ADR-0017:7/30/38–51 修订。`stock_stream.rs:36` 的临时通知channel、`local_admission.rs:92` 的局部偏序不是违规恢复actor权威。 |
| 任务3，204–210行：双账本、费用、链 | ledger/conservation与真实 ReceiptAggregation 已接线；`receipt_aggregation.rs:63` 先validate，再安装created并消费收据；守恒和重复拒绝测试仍在。旧 preserved-test/baseline corpus工具退役不是现行接线遗漏。 |
| 任务4，212–218行：P3/P4拆分与ID | `ready_stock_stream.rs:154` 持续validator，`:165` 挂pending；价格解析与盘口权威校验在P4；`continuous_tick_transaction.rs:272` 最终推进next_order_id。局部受理替代历史npc→player→plan类序。 |
| 任务5，220–226行：收据结算与截点 | `settlement.rs:113` Buy后Sell、`:157` 累计gross与实收分项；无将释放当现金额外收入的第二结算路线。`adaptive_plan_chain_tests.rs:516` 保留同tick撤单不回填validator cash边界。 |
| 任务6，228–234行：持续股票shadow与竞价/日界 | `stock_stream.rs:91` 初始化连续shards，`:109` finish_once；`continuous_tick_transaction.rs:248` 只在流排空后capture边界并finalize。竞价单独shards/finalizer，历史固定来源优先已被替代。 |
| 任务7，236–242行：P9、失败及宿主 | `candidate_commit.rs:46` 完成所有可失败前置，`:98` 才commit_tick_shadow；`continuous_tick_transaction_tests.rs:925/958` 下游与后轮溢出回滚；`pre_open_transaction_tests.rs:412` 盘前失败回滚。宿主完整门禁的独立审计归宿主记录，本轮不重新声明三宿主验收完成。 |
| 任务8，244–250行：Save/StrategyState/恢复 | `strategy/state.rs:107` 校验全量具体策略状态，账户保存在StoredStrategy；`continuous_tick_transaction_tests.rs:521` 非空计划部分成交恢复再成交；`pre_open_transaction_tests.rs:228` 竞价rollover存档恢复。公共存档已演进到日终Archive，不能恢复旧随时UI存档承诺；内部quiet-point不等于公共日内存档。 |
| 任务9，252–258行：三门禁/负控/性能 | 旧语料与零现金witness门禁已核销，现行工具仍有G39。`escrow-verification-contracts.mjs:130/160` 比各budget全artifact；`run-escrow-verification-matrix.mjs:583` 用单baselineVector比较所有正常artifact，没有绑定相同实际受理事实。负控 `:447` 仍要求typed rejection+业务哈希回滚；不能连负控也随旧“整局相同”一起删除。 |
| 任务10，260–266行：诊断再基线 | 历史Task9-complete→Task10-complete区间未存在，原文295/305行明确保留未勾并终止迟补；不是新增生产代码stub。当前诊断跨层遗漏另见G37，不依据旧勾选推翻。 |
| 任务11，268–274行：K7 | 保留当前矩阵/构建/timeout/来源fingerprint工具；旧94次验收记录不能覆盖b76ece3。G39为两个当前比较入口的契约问题，不能要求恢复退役verify-k7-root路径来解决。 |
| 任务12，276–282行：文档 | 符号、panic、测试deadline政策已有正式文档；旧文档残留不能充当验证通过。当前产物/规则依正式ADR，不由本轮任务报告覆盖。 |
| F1–F4，324–341行 | 原文303–308行用户已终止剩余验证，保留历史状态。独立语义复核仍是新改动门禁；本轮只有记录，由父任务统一独立复核。 |
| 执行组织/Commit/Success，78–163/343–362行 | 主要为历史实施流程与证据要求，不当作生产功能待实现；本轮无Git写操作，无全量验收，未补勾历史checkbox。 |

## 既有总账复核

- **G16保留。** `session/decision_chain/roots.rs:25` 仍 `plans: session.state.plans.clone()`，`plan_chain_candidates.rs:304` 每次 capture后给roots共享Arc。Arc共享capture结果不消除PlanBook全历史深复制。稀疏盘口delta只削减簿反馈，不能核销计划历史复制。
- **G38保留。** `session/decision_chain.rs:972` 与 `:1236` 两处生产 AllocationRequest 均 ExistingPlan，软预算没有提供NewOpportunity分类。本人软预算排序不得反向变为跨账户的撮合优先。
- **G39保留。** 两个实际脚本的正常比较仍是跨worker整体artifact强等同；真实入口允许不同局部并发受理轨迹。保持价时、依赖、资金股份、类型化失败负控，同时增加相同受理事实重放，不恢复全局排序以“修绿”。

## 新候选 S20-C01：策略公共构造/工厂没有完整兑现非法参数拒绝

原文契约：strategy设计第130行“参数非法→工厂构造时Err……绝不静默用默认值”，第152行参数非法测试。当前公开API存在以下两个同族边界：

- `MomentumStrategy::new`（`strategy/momentum.rs:59`）对 trend_threshold 只在`:70` 检查 `<0.0`，所以 NaN与正Infinity均返回Ok；`StrategyParams::validate`（`strategy/params.rs:64`）同样只调用该构造器，无法拒绝这些hot参数；`StrategyFactory` Hot分支（`factory.rs:153`）构造后在`:164` 直接返回，不调用validate_state。单独调用这些公开接口可得到无效策略，hot内核 `strategy/hot.rs:25/39` 的阈值比较可能全部为false而无动作。
- `StrategyFactory` Inst分支 `factory.rs:139` 先对输入 margin乘个体系数再 `.min(0.95)`，对NaN/正Infinity及不少≥1输入直接变为0.95后返回合法策略；`BeliefInstitutionStrategy::new` 本身 `value.rs:75` 有正确范围拒绝，工厂先裁剪掩盖了输入错误。保留合法参数的个体偏差封顶与拒绝非法原始参数是两回事。

**反证与影响限制：** `momentum.rs:46` 的validate_state已有is_finite；`strategy/state.rs:108` 的from_strategy会validate；`session.rs:1015` 先validate setup，`:1858` 在生成NPC时再次from_strategy，故不能据此宣称NaN/+Infinity策略能进入正式GameSession或污染存档。Inst非法margin在正式setup的 `StrategyParams::validate`→`BeliefInstitutionStrategy::new` 亦会先被挡住。`strategy_state.rs:117` 的wire非有限参数测试也已存在。候选范围是**公开构造器、公共工厂及公共参数validate的拒绝契约不一致**，不是已有正式会话防线不存在。

建议父任务独立复核后决定是否转新增G；以短case覆盖公开new/validate/build的NaN、±Infinity与margin≥1，断言Err且无静默裁剪，同时保留合法margin经个体变化封顶的行为。无需改变大A撮合、费用或受理顺序。本轮为静态反例推导，未编造运行复现。
