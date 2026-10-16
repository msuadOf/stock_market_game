# trading-rules.md 规则 ↔ 测试追溯矩阵（只读前置分析）

- 生成：2026-10-17，任务 #103（语义单测细化）前置分析。
- 范围：`docs/trading-rules.md` 全部已登记规则/简化声明，对照 `packages/engine` 测试。
- 本 worktree 仅含 `packages/engine`（无 `packages/web`），Web 层规则只标范围注记。
- 测试形态标记：`[旧]`=旧管线具名独立测试；`[rt]`=std-thread runtime 具名独立测试
  （desk_semantics_tests / skeleton_tests / day_end_tests / facts_tests / protocol_tests /
  saved_runtime_tests 等）；`[both]`=两侧均有；`[对账]`=runtime_flip_batch1-5 /
  drive_tests / equivalence_tests 等新旧对照类（按任务口径 drive_tests 归对账类；
  注：其中 sol*/d3a3 系实为 runtime 单侧世界测试，非逐事件对拍，升级为独立测试成本低）。
- 状态：独立=有具名独立测试；对账=仅对照类覆盖；零=无测试。
- 行号基于当前 worktree（exp/std-thread-model）。

## 矩阵

| ID | 规则（登记日期） | 状态 | 形态 | 测试锚点 |
|----|------------------|------|------|----------|
| R01a | 自成交限制·集合竞价双撤卖侧+公开量=实际量（2026-10-15） | 独立 | [rt] | src/runtime/desk_semantics_tests.rs:507 `p1_auction_self_trade_double_cancel_refunds_and_reports_actual_volume`；:549 `p1_3_undo_replay_clears_self_trade_index`；facts_tests.rs:141 成交量事实 |
| R01b | 自成交限制·连续竞价 taker 交叉档自单终止撮合（2026-10-15，游戏额外限制） | 对账 | [对账] | tests/runtime_flip_batch1.rs:219（`batch1_trade_event_fields_wellformed` 内一行 `assert_ne!(maker,taker,"自成交限制已修")`，无手算 oracle） |
| R02 | 日终未成交单失效+显式 OrderCanceled（2026-10-15，翻转批裁决2 有意分歧） | 独立 | [旧]（rt 仅对账） | tests/auction.rs:733 `closing_auction_accepts_limit_orders_then_expires_an_unmatched_remainder_at_day_end`（:788 断言 OrderCanceled remaining_qty）；runtime 面仅 tests/runtime_flip_batch3.rs:10 `batch3_runtime_emits_order_canceled`、:236 `batch3_exact_closure_on_no_trade_fixture` |
| R03 | 涨跌幅 10%/20%（主板/ST/创业板，逐股显式配置） | 独立 | [both] | tests/market/price_limits.rs:118/126/134 place 拒收/边界；tests/market/main.rs:38；tests/config.rs:324-473 极限参数校验；rt desk_semantics_tests.rs:173 `price_band_is_closed_interval_and_cage_only_continuous` |
| R04 | 最小价格变动 0.01 元 | 独立 | [旧] | tests/orderbook.rs:90 `orderbook_new_validates_tick`；:129 `place_rejects_invalid_price_and_qty`。rt 无直接"非整 tick 拒单"具名测试（笼子/带测试隐含） |
| R05a | 买入 100 股整数倍；一手不可改 | 独立 | [both] | tests/session.rs:3950 `a_share_buy_quantity_must_be_a_board_lot`；config.rs:474/493 lot 校验；rt skeleton_tests.rs:600 `desk_rejects_odd_lot_buy_and_refunds_via_entities`；facts_tests.rs:106 NotRoundLot 回执 |
| R05b | 单笔上限：主板/ST 1,000,000；创业板限价 300,000/市价 150,000 | 独立 | [both] | tests/session.rs:546 `chinext_enforces_limit_and_market_order_quantity_caps`；rt desk_semantics_tests.rs:231 `order_cap_differs_by_board`；facts_tests.rs:106 ExceedsOrderCap |
| R06 | 卖出≤持仓−T+1锁定−已挂卖单；零股须一次性卖出 | 独立 | [both] | tests/account.rs:118/329/353；tests/session.rs:3979 `a_share_odd_lot_sell_cannot_split_the_odd_lot_remainder`、:5080/:5123 restore 侧；rt desk_semantics_tests.rs:469 `odd_lot_sell_allowed_only_as_full_remainder` |
| R07a | T+1 当日买入不可卖 | 独立 | [both] | tests/account.rs:266 `apply_buy_t1_locks_sellable`；pipeline continuous_trade_acceptance_tests.rs:20；rt desk_semantics_tests.rs:284 `t1_frozen_shares_cannot_sell_same_day` |
| R07b | T+1 下一交易日日界解锁 | 独立 | [旧]（rt 仅对账） | tests/session.rs:4008 `a_share_t1_locked_shares_unlock_at_the_day_boundary`；pipeline continuous_tick_finalizer_tests.rs:371；rt 仅 runtime_flip_batch4.rs:27 `batch4_t1_unfreeze_clears_frozen_after_day_end` |
| R08 | 开盘竞价 09:15–09:25 窗口、前段可撤、一次撮合、余单入连续簿、09:25–09:30 拒新申报 | 独立 | [both] | tests/auction.rs:1139 `auction_only_trades_once_on_its_last_tick`、:1487（不可撤窗拒收）、:1512/:1578（前 1/3 可撤，游戏比例简化）、:1628 `orders_are_rejected_during_the_0925_to_0930_preopen_window`、:1663 余单入簿；rt desk_semantics_tests.rs:338 `preopen_rejects_and_auction_requires_limit_orders`、:367 `cancel_window_closes_after_first_third_of_opening_auction` |
| R09 | 撤单：部分成交只撤余量；已全成明确反馈失败 | 独立 | [旧]（rt 弱） | src/orderbook/state_contract_tests.rs:18 `partial_full_cancel_and_clone_keep_identity_and_fifo`；tests/plans.rs（已成交撤单业务失败）；rt 仅 facts_tests.rs:106 ReceiptKind 枚举相等性（无业务流测试） |
| R10 | 游戏简化：撤单释放资源下一 tick 才可占用 | 独立 | [both] | rt skeleton_tests.rs:153 `entity_batches_arrive_before_next_start`（:190 断言退款在下一 tick 决策前落账）；tests/session.rs:5469 `symbolic_buy_releases_excess_escrow_only_for_the_next_tick_across_stocks`（超额 envelope 面）。同 tick 拒绝复用的负向断言未见具名测试 |
| R11 | 收盘竞价 14:57–15:00 不可撤、一次撮合、余单日界失效、ClosingAuction 阶段+clearing_price 事件 | 独立 | [旧]（rt 部分对账） | tests/auction.rs:720 `closing_auction_has_a_distinct_phase_at_the_end_of_the_trading_day`、:733、:450；rt 面：desk_semantics 世界含 closing=3 相位 + batch2/batch4 窗口对账；rt clearing_price 事件面无具名测试 |
| R12 | 交易所归属显式配置；沪市同价取候选中间价（半进位）；深市统一昨收（登记简化）；完全同距取低 | 独立 | [旧] | tests/auction.rs:985/:1008/:1033/:1065/:1090（shanghai_clearing_price_* / shenzhen_final_tie_break_* 系列，本身就是手算 oracle 风格）。rt 侧未见同题具名测试 |
| R13 | 连续竞价价格笼子 102%/98%+十个最小价位较宽者；参考价顺序（对手一→本方一→最新/前收） | 独立 | [both] | tests/market/price_limits.rs:34/:71/:93；rt desk_semantics_tests.rs:203 `price_cage_rejects_beyond_wider_bound_in_continuous` |
| R14 | 可选简化（2026-09-27）`price_cage_enabled` 默认开、关闭仍走涨跌幅 | 独立 | [both] | tests/config.rs:63/:73（配置面+非布尔拒绝）；rt desk_semantics_tests.rs:173（cage=false 臂）；tests/session.rs:5441 笼子有无两态 |
| R15 | 过户费 0.01‰ 双边、印花税 0.5‰ 仅卖出、佣金+最低佣金可配 | 独立 | [both] | tests/config.rs:92/:583-652；tests/account.rs:169 `apply_buy_batch_charges_minimum_commission_once_across_fills`；rt facts_tests.rs:29-73（stamp tax 仅卖腿校验、费用非负等事实校验） |
| R16 | 游戏简化：市价意图按涨跌停价保护逐档立即成交，余量不进簿 | 独立 | [rt] | rt desk_semantics_tests.rs:418 `market_order_remainder_refunds_instead_of_resting`；旧 pipeline continuous_matching_tests.rs 连续市价系列。"按涨跌停价作保护"的命名断言未见（余量面已锚） |
| R17 | 共享资源占用：买单占成交额+费用、卖单不占现金、reserved_cash/reserved_sell_qty、卖费实收封顶按佣金→印花→过户顺序分配（ADR-0017 分歧#9 简化） | 独立 | [both] | tests/auction.rs:1360/:1400（envelope 预留）；rt desk_semantics_tests.rs:312 `seller_fee_capped_by_proceeds_in_priority_order`；src/session/persistence/saved_runtime_tests.rs:900 `seller_cumulative_nominal_and_charged_audit_survives_restore`、:603；rt SealAllocationSnapshot 密封预算面仅 equivalence_tests.rs:286（对账） |
| R18 | NPC 仓位选择（2026-09-26，ADR-0021）：不按固定单股/现金比例限制目标 | 独立 | [旧] | tests/strategy.rs:738 `value_strategy_can_add_to_a_concentrated_position_with_available_cash`（集中持股正向）；noise 报价面 strategy.rs:124、behavior.rs:1504。"无固定比例上限"的负向断言未见具名测试 |
| R19 | 最高/最低价格选项（2026-09-27，ADR-0022）：涨停价预留资金、释放差额不回补本 tick 预算、不自动追价 | 独立 | [both] | src/market.rs:588 单元；tests/session.rs:5346/:5397/:5441/:5469；tests/auction.rs:1698/:1735；rt protocol_tests.rs:124 `routed_price_covers_fixed_highest_lowest_market` |
| R20 | 机构风险暂停/不利选择/恢复需下一观察（ADR-0026，游戏策略假设） | 独立 | [旧] | src/session/institutional_behavior.rs:264/:295/:311/:357（内联测试：触发/恢复阈值分离、真实买入身份、事实不删只衰减） |
| R21 | 草稿容量约定（2026-09-25，ADR-0019）：不按固定条数拒绝合法请求 | 零 | — | 未定位任何引擎测试（负向契约，草稿阶段登记） |
| R22a | 计划 horizon/日内到期：积极型当日到期、跨日计划、到期显式终止 | 独立 | [旧] | tests/plans.rs:284 `horizon_expiry_at_day_end_terminates_with_explicit_reason`、:846/:1110/:1139 |
| R22b | 计划分类观察节奏（成长/均衡可跨日、深度价值/防御低频不因开盘复核） | 零 | — | 未定位具名测试（分类 cadence 面无锚） |
| R23 | 受理先后：NPC 上 tick 形成下 tick 受理、玩家下 tick 参与、来源标签非优先级、同账户按接收事实 | 对账 | [对账]+[rt 部分] | rt skeleton_tests.rs:153（批次下一 tick 到达，正向）；深层断言（来源标签非优先级/同账户接收事实裁决）散于 flip 批与 pipeline 集成测试，无独立具名锚 |
| R24 | 计划根逐个完成交付、execution leg 生命周期、day-end drain/abort 回滚 | 对账 | [对账]（实为 rt 单侧） | src/runtime/drive_tests.rs:900/:1013/:1205/:1275（d3a3/solmid/sol4 系列；按任务口径归对账类，改造为独立 oracle 测试成本低） |
| R25 | 存档：完整日结结构、无代际版本、额外/旧字段显式拒绝、缺失拒读档 | 独立 | [both] | tests/save_contract/failures.rs:93-324（obsolete_version_marker:154、obsolete_runtime_key:167 等）、restore_guards.rs、audit_restore_guards.rs；rt day_end_tests.rs:199 `day_end_candidate_roundtrip_rebuilds_equivalent_world`；对账 batch5 |
| R26 | 序号事实：seq/book_next_sequences 保留不重编号、严格递增、空簿保留已耗序号 | 独立 | [旧]（rt 对账） | tests/auction.rs:335/:377-446、tests/session.rs:347/:434、tests/extraction_replay.rs；rt 仅 runtime_flip_batch4.rs:151 `batch4_seq_continuous_across_day_end` + equivalence_tests.rs:231 |
| R27 | Money 分编码 wire（ADR-0031）：i64 十进制分字符串、拒绝数字金额 | 独立 | [旧] | tests/money.rs（29 个）；tests/config.rs:141/:166 |
| R28 | 图表周期展示简化（2026-10-05）引擎时钟面：observation_civil_instant 比例、日界/午休端点不冒充 | 独立 | [旧]（rt 对账） | tests/civil_clock.rs:132/:445/:553；rt 仅 batch5_civil_date_advances。Web 聚合/分桶面不在本 worktree（packages/web 缺席），范围外 |
| R29 | 快捷委托（2026-10-05）：手数×100 转换、可买预检扣费 | 独立（引擎原语） | [旧] | 引擎面复用 R05/R15 锚点；Web 预检面范围外 |
| R30 | 定时自动（2026-10-05，Web 会话功能）：到期执行、暂停无进度不执行、不写日终存档 | 零（engine 范围） | — | 引擎无对应物（by design）；"存档不含该域"未单列断言；Web 面范围外 |
| R31 | 散户成交事件面迁移期简化（2026-10-16 登记，D5 第11任复核 Major2）：runtime 不承载 consecutive_failed_buys/post_exit_cooldown/持仓峰值反馈 | 独立（旧）+ rt 零覆盖 | [旧] | 旧管线：tests/experience_feedback/main.rs:160/:195/:225、reads.rs:10-160、seam.rs:139/:185；src/session/pipeline/account_settlement_tests.rs record_fill 家族。runtime 侧无承载（登记盲区，"专项测试承载"待补；drive_tests.rs:1946 sol8th 仅首持初始化面） |
| R32 | desk 分段记账口径（2026-10-16）：每笔成交累计费用分项、purse 边际口径、分段≥累计、存档审计 fault2228 下界+宽松上界 | 独立 | [both] | src/session/persistence/saved_runtime_tests.rs:900（charged 分项断言 :912-914/:964-966）、:603；rt desk_semantics_tests.rs:312 |
| R33 | 尚未模拟负向范围（两融/大宗/临停/退整/新股无涨跌幅/科创板差异） | 零（登记性） | — | 无实现亦无拒绝路径测试；公司经营域简化由 company-accounting 测试族承载（industrial/bank/insurance/real_estate accounting 测试目录存在） |
| R34 | 同价同向申报时间优先（2026-09-25 复核沪 3.5.1/深 3.4.2：按主机接受时间） | 独立 | [both] | rt desk_semantics_tests.rs:124 `same_price_level_fills_fifo_by_arrival`；tests/auction.rs:335 跨 tick 同价 FIFO（含存档往返）；tests/orderbook.rs 系 |
| R35 | 公司经营收付不进投资者资金池；集团单层/移动加权简化（company-accounting §2.6/§8） | 独立 | [旧] | tests/industrial_accounting/、bank_accounting/、insurance_accounting/、real_estate_accounting/、consolidation/ 测试族（域内锚定，非交易撮合域） |

## 统计

- 规则测试面总数：**39 面**（R01/R05/R07/R22 各拆两面；R31 独立（旧）+runtime 零单列注记）。
- 有具名独立测试：**32 面**。其中 **9 面**（R02/R04/R07b/R09/R11/R12/R18/R26/R28）仅旧管线锚定、runtime 侧靠对账——迁移收官后旧管线测试若判死（任务 #105/#106），这 9 面即失锚，需优先在 runtime 重锚定；R20/R22a/R35 亦为旧管线侧模块内测试（strategy/plans/company 域，非撮合路径）。
- 仅对账覆盖：**3 面**（R01b 连续竞价自成交终止、R23 受理先后深层断言、R24 计划根逐个完成——注：R24 实为 runtime 单侧世界测试，重分类即可转独立）。
- 零覆盖：**4 面**（R21 容量约定、R22b 计划分类观察节奏、R30 定时自动 engine 面[by design]、R33 负向范围[登记性]）。
- 另：R31 散户成交事件面为登记在案的 runtime 结构性盲区（旧管线独立、runtime 零）。

## 需补独立测试清单（缺口，按优先级）

1. R01b 连续竞价自成交终止撮合——手算 oracle：构造 taker 交叉档含本人自单，断言终止撮合+被阻断的第三方不成交+资源闭合（现仅 flip_batch1:219 一行 assert_ne!）。
2. R31 runtime 散户成交事件面专项测试（登记的迁移期盲区：受挫计数/冷静期/持仓峰值反馈）。
3. R23 来源标签非优先级 / 同账户按接收事实裁决——独立可判定 fixture。
4. R24 计划根逐个完成系列从 drive_tests 重分类或复制为具名独立测试（已近 oracle 风格）。
5. R02/R07b/R11/R12/R26/R28 六面在 runtime 侧重锚定（旧管线锚定迁移后可能失锚）：日终 OrderCanceled、T+1 日界解锁、收盘竞价 clearing_price 事件、沪深清算价归属、seq 单调、civil 时钟比例。
6. R09 runtime 已全成撤单 AlreadyFilled 业务流回执（现仅枚举相等性）。
7. R10 同 tick 拒绝复用释放资源负向断言（现仅"下一 tick 前落账"正向）。
8. R16 市价意图"按涨跌停价保护"命名断言。
9. R17 runtime SealAllocationSnapshot 密封预算/卖单现金预留恒零独立测试（现 equivalence:286 对账）。
10. R21 容量约定负向契约、R22b 计划分类观察节奏、R18 无固定比例上限负向断言（三者均为登记承诺无锚）。

## 方法注记（诚实边界）

- 覆盖判定基于 rg 关键词（规则中文名/领域词/测试名模式）+ 逐文件测试名清单比对；src/session/pipeline/*_tests.rs 体量大，未逐行阅读全部断言，"未定位"指未找到具名锚，不排除有弱断言存在于邻近测试。
- R24 按任务口径归对账类，但其内容为 runtime 单侧行为测试；统计含义见注。
- packages/web 不在本 worktree，R28/R29/R30 的 Web 面未核查。
