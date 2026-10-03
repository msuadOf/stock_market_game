# Sweep16：订单簿历史承诺与交易规则连续复核

## 范围与证据口径

- 完整连续阅读 `docs/superpowers/plans/2026-06-29-orderbook.md` 1–868 行、`docs/superpowers/specs/2026-06-29-orderbook-design.md` 1–141 行、`docs/trading-rules.md` 1–123 行，共 1132 行。覆盖正文、代码样例、全部任务步骤、Self-Review、规则来源和明确未模拟项。
- 已读根 `AGENTS.md`、`docs/principles.md`，检查适用的嵌套指令；未发现本报告目录有更深的 `AGENTS.md`。对照旧总账 R08、`coverage/r08.md` 与 `reaudit-core-contracts.md`；后两份的旧代码行号没有机械复用。
- 指定产品基线 `b76ece3`；读取时 worktree HEAD 已为 `4ad5a2e`。只读 `git diff b76ece3 HEAD` 检查本报告引用的 OrderBook/Market/session/persistence/validation/continuous_matching/stock_auction/trading-rules 文件，结果为空，引用的行号和产品结论适用于指定基线。
- 本轮仅静态检查生产 caller、实现、消费与测试源码；未运行测试、编译或官方来源联网核查，未执行 Git 写操作。规则有效性采用当前 trading-rules 记载的核查日期，不自行补充未承诺的真实制度。

## Plan 全部任务和 Spec 全部章节

| 原文条款 | 当前状态及生产证据 |
|---|---|
| Plan 5–19 行：目标、分层、Money、tick、防御式错误、依赖与历史 TDD/提交 | **核心实现存在；历史过程未验收。** `orderbook.rs:21` 依赖 Money，`orderbook.rs:283` 持有纯簿状态，`orderbook.rs:304` 正 tick 校验，`orderbook.rs:351` 重复身份/进度/正价/tick 校验。无 I/O 或定时器；真正推进者是 `continuous_matching.rs:480`。后续 ts-rs 类型导出与持久化索引扩展属于演进，不能把历史“只用已有依赖”当永久架构限制。静态代码不能证明每次历史先红后绿或逐项提交。 |
| Plan 21–31、Task1 35–130：文件布局、Side/OrderId/OrderError、导出 | **已实现。** `orderbook.rs:25,48,83`、`lib.rs:27`。OrderBook 已拆为 BookState 与索引子模块，公开语义仍有；文件拆分不构成漏项。 |
| Task2 132–235：Order/Trade/MatchResult/AccountId 与 serde | **已实现并扩展。** `orderbook.rs:150,156,221,248`；新单由 `continuous_matching.rs:480` 构造，结果由同文件 `517` 进入成交双方 receipt。额外 original_qty/filled_qty/filled_value 支持累积佣金和存档，未替换原承诺字段。serde 测试源码 `tests/orderbook.rs:30`。 |
| Task3 237–325：构造与最优买卖价 | **已实现。** `orderbook.rs:304,318,328`；`market.rs:323,328` 生产包装。构造测试源码 `tests/orderbook.rs:90`。 |
| Task4 327–440：校验、单边无成交时留簿 | **已实现，零价域存在旧文档歧义。** `orderbook.rs:351` 先校验，`orderbook.rs:456` 残量分配 seq 后插入。Task4 334 行允许 price>=0，现实现 358 行为 price>0；旧 R08/core 已留档，不能重复报成新增漏项或凭常识为旧文盖章。 |
| Task5 442–638：交叉、逐档、部分成交、价时、maker 价 | **已实现。** `orderbook.rs:377` 循环，387/393 行分别 buy>=ask/sell<=bid，424 行 min 数量，428 行 Money 成交额，438 行 maker 更新，440 行 Trade，456 行余单。`market.rs:316` 成交驱动 last_price，`continuous_matching.rs:517,1049` 投影和消费双方回执。测试源码 `tests/orderbook.rs:239,256,279,295,306`。 |
| Task6 640–714：撤单返回余单、未知 ID 显式错误 | **已实现并细化全成交反馈。** `orderbook.rs:489` → BookState；生产 `continuous_matching.rs:1330` 检查当前簿与 filled owner，1344 行所有权，1351 行生成资源释放 receipt，1355 行撤余量；347 行起集成测试覆盖全成交不能撤与部分余量可撤。 |
| Task7 716–847：深度聚合、导出、clippy/build/test | **实现存在；命令验收未执行。** `orderbook.rs:637,651,667` 深度为 u64，避免多笔 u32 聚合越界；`lib.rs:27` 导出。测试源码 `tests/orderbook.rs:385,399,411`。不能据此声称历史 clippy 或全量测试已通过。 |
| Self-Review 849–868：类型、算法、错误、矩阵、已知风险 | **现代码覆盖功能；旧代码样例有错误。** 计划 554/624/868 行把卖单交叉错误写成 >=，与同计划 449 行及 spec 的卖价<=买价冲突；当前 393 行使用 <=，不是遗漏实现。原字段签名中的 OrderNotFound 命名字段与元组示例冲突，同样是文档漂移。 |
| Spec §1–2，10–24 行：撮合内生价、单股纯模块、限价与 ID | **核心已实现；身份责任演进。** Market 一股一簿，`market.rs:279` 调撮合，316 行末笔成交改变价格。簿只分配时间序 `orderbook.rs:457`；跨股身份由 session validation driver 分配并 checked_add（`account_validation_driver.rs:101,108`）。旧“簿内部自增 ID”与带 id 的输入设计冲突，当前 caller 身份唯一性与簿重复保护存在。 |
| Spec §3，25–77 行：全部类型/API | **已实现并演进。** 与 Tasks1–7 逐项一致；new 返回 Result，与“非法 tick Err”一致，旧返回 Self 签名属文本矛盾。depth u32→u64 与内部 BookState 拆分保留原读写语义。 |
| Spec §4，78–89 行：撮合流程 | **已实现。** 见 Task5；seq 用于同价优先，owner/source/OrderId 不替代价格和时间键。 |
| Spec §5，90–99 行：价量/tick/重复/缺单显式错误 | **已实现。** `orderbook.rs:83` 错误类型与351行先验校验；`market.rs:302` 日涨跌停在进簿前拒绝；`continuous_matching.rs:492` 业务拒单转 receipt、502 行内部错误上抛。 |
| Spec §6，100–106 行：账户/涨跌停/行情边界、市价延后 | **分层保留，市价范围被后续规则细化。** 账户资源与费用见下表；日涨跌停在 Market，价格笼子在 session。市价意图用限价簿成交后撤余量 `continuous_matching.rs:510`，最新 trading-rules 42–43 行明确简化；IOC/FOK/各交易所市价细分类未承诺。 |
| Spec §7，107–121 行：11 项测试矩阵 | **测试源码存在；未运行。** 见 `tests/orderbook.rs:30,65,90,108,129,239,256,279,295,306,317,336,347,385,411`。tick=1 分的价格不能表达10.005元，旧设计示例由 plan 363 行 tick=5 分反例修正；不能按不可表示样例报缺测试。 |
| Spec §8–9，122–141 行及对应 DoD | **布局/导出存在，历史验收未证明。** Rust 编译/测试/告警和提交过程未重跑、未查询历史成功见证；不能把 checkbox 未勾等同产品未实现。 |

## 当前 trading-rules 全文逐段交叉核查

| 原文条款 | 状态及生产链 |
|---|---|
| 3–14 行：权威 session、Escrow envelope、卖费封顶、验证投影不是另一套规则 | **已接线/明确简化。** `pipeline/mod.rs:236` 私有 candidate 经 P0→P1；`account_validation.rs:740,758` 买预留现金，卖仅预留股份；`continuous_matching.rs:1162,1184` 成交进入 FeeTransition；`transition.rs:80` 校验累计 nominal 与 charged。官方材料最近核查范围和费用404声明属于证据边界，未升级为“全部官方验收”。 |
| 18–23 行：板块日涨跌幅、tick、100股、分类单笔上限、零股、T+1 | **生产校验存在。** `Market::place_inner`（`market.rs:300`）取得显式股票上下限；`account_validation.rs:710,718,728,731` 分别上限、买整手、可卖预算、零股余数；`account.rs:493` 买卖 settlement，日界 `auction_day_end.rs:2156` 解锁。未要求将尚不支持的科创板/北交所硬塞进这些分类。 |
| 24–29 行：开盘申报/撤单、09:25窗口、新受理余量可撤、同 tick 释放不可复用 | **存在实际 caller。** `session.rs:1989,2003` 把开盘窗口分 CallAuction/PreOpen；`stock_auction.rs:17,29,183` 显式撤单窗口，`continuous_matching.rs:1330,1355` 取当前簿而非旧快照决定能否撤。新委托预算从本轮资源快照建立（`account_validation.rs:1121,1133`），cancel 生成 release receipt 不回补该预算。 |
| 30–35 行：收盘阶段、一次清算/余单失效、交易所清算差异 | **存在实际 caller；深市参考明确已知简化。** `session.rs:1995` ClosingAuction，`auction_day_end.rs:968` 日末一次完成，`stock_auction.rs:308,330` 统一完成入口；`stock_auction.rs:817` 上海候选中间价、837行深圳最小差后距 previous_close。所有深圳竞价统一昨收、同距低价已在规则34–35行明确，不能重报成历史未实现承诺。日界 finalizer `auction_day_end.rs:2144` 检查余单/envelope已清。 |
| 36–40 行：价格笼子与可关闭配置 | **存在实际 caller。** `continuous_matching.rs:449,454,456` 受理时解析符号价与按 price_cage_enabled 检查，467行显式拒单。关闭仅跳笼子，不跳过 `market.rs:302` 日涨跌停。 |
| 41–49 行：真实费率口径、模拟市价、共享占用、卖费实收封顶顺序 | **实现/明确简化。** `config.rs:212,229,234` 佣金/税/过户费，`transition.rs:45,80,146,167` 累计 nominal 与费用差；`continuous_matching.rs:510` 市价残量撤销。买单 reservation 含费，卖单 ResVec现金为零 `account_validation.rs:740,758`；成交 receipt →账户 settlement 接线见 `continuous_matching.rs:517` 与 `settlement.rs:76`。本文不把卖费封顶当真实券商结算规则。 |
| 51–75 行：NPC 可集中/现金、主动与等价报价、符号价存档、机构经验策略 | **策略语义由对应 ADR 承接；本轮复核其交易边界。** `continuous_matching.rs:449` 受理时 resolve_draft，480行落簿保存实际 Money 价格；`orderbook.rs:156` 已受理单没有动态追价方式字段。资金股份继续走 account_validation；机构观察/经验模型完整性不在订单簿模块中重复定规则，需与其他 sweep 的 ADR-0021/0022/0026 明细合并，不能仅凭本表宣称机构全行为验收。 |
| 77–95 行：无固定容量配额、策略计划跨日、NPC/玩家下一tick、局部受理顺序及待提高性能 | **已有路径与明确尚未完整优化项。** `account_validation.rs:391` account-local arrival、`account_validation_driver.rs:101` 资源验证与订单身份分别推进；`continuous_matching.rs:480` 股票入口真正送簿后才产生 seq。规则93行已经保留等待范围和加速比不足，本轮不把已知工程优化重命名为新交易制度缺口。每日计划/注意力完整明细由其他 sweep 复核，本表只核对撮合/资金/T+1边界。 |
| 97–112 行：官方来源、核对日期、费用访问失败、规则修改需同步 | **文档与证据限制。** 本次没有官方联网或业务实现修改，无新增规则适用日期。 |
| 113–117 行：当前完整格式、交易事实/RNG/日K、无损 seq 和空簿 cursor | **订单簿事实链已实现。** `session.rs:2608` 每股保存 next_seq，2774行恢复；`persistence.rs:273,285` 股票键集一致校验；`orderbook.rs:548` 先按原 seq 排序且拒重复，559行 seq<next_seq 与 JS安全域，candidate成功后替换。cursor 是 u64 独立事实，不能由空簿倒推0。其他完整存档字段由 persistence 专题复核。 |
| 119–123 行：未模拟真实制度与投资建议边界 | **明确范围排除。** 新股无限幅、科创/北交所差异、停复牌、退市、临停、大宗、融资融券、个性化舍入不作为本模块漏实现。 |

## 候选与反证

1. **卖单 crossed 方向错误候选：否决。** Plan 554/624/868 行样例真的写错，实际 `orderbook.rs:393` 正确用 <=；这是历史文档代码样例错误，不是生产漏实现。当前源码第381行旧注释仍写“统一>=”与实际卖单不同，为文档/注释漂移，旧审计已核查实际算法，不升级为交易缺口。
2. **簿内 next_id 缺失候选：否决。** 生产 session caller 提供跨股单调唯一 OrderId，簿351行拒重复；旧设计又接受含id的Order。本模块没自增ID不是调用链断开。现行“会话身份、簿时间序”应在历史规格责任描述中收口。
3. **零价未接受候选：维持旧文档歧义。** 旧Plan>=0与现代码>0不一致，已在R08/core列明；本次未找到足够最新明文收口依据，不重复登记新项。
4. **深市收盘竞价错用昨收候选：归既有简化。** `stock_auction.rs:330,848` 确实使用 previous_close；trading-rules34–35行明确承认尚未修正，不作为未公开遗漏。
5. **撮合错误发生后公共簿部分变化候选：维持既有边界。** `orderbook.rs:438` 每腿更新maker，后腿可能Money溢出；core复核已解释公共place无任意错误强原子承诺。生产 `continuous_matching.rs:502` 将此上抛并丢弃 private tick candidate，不新增“已拒单仍扣款”结论。
6. **缺市价/IOC/FOK候选：否决。** trading-rules42–43行只承诺保护价立即成交与撤余量，现510行接线；独立交易所市价分类、IOC/FOK不在原模块范围。
7. **新受理部分成交订单不能撤候选：否决。** 当前1330行读取本股当前簿，并由1355行撤剩余量；全成交通过filled owner给业务反馈，测试源码347行覆盖区分。

本轮没有新增确认的订单簿生产实现遗漏。该结论限于逐项记录的静态路径，不代表所有策略/宿主/性能及历史DoD已经验收；上述交叉专题须与对应sweep合并，报告本身完整diff由主审独立复核。
