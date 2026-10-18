# D5 判型台账（第 10 任，2026-10-16，HEAD=本提交）

env=1 剩余 27 例逐域判型（进程级对照=红形态+代码面核对）。已修 1 例
（zero_quantity）。**判型≠结论**——每域标注修法方向与建议下轮顺序。

## 已修（本批）

- `public_step_reports_zero_quantity_in_event_and_retail_diagnostic`：
  组端 qty=0 校验（route 层拒收+事件缓冲+Rejected 诊断派生）。
- **runtime step 日界安装**（ends_day 拍）：session 侧交易日终迁移
  （TradingDayEnd 复用——day 推进/candle 提交/T+1/DayBoundary；先于
  publish install 后于行情安装——序错则 save fault2010/2076 交替显红，
  price_history expected_len 改 tick 推导解耦）。history_reads 推进两
  层 fault（civil 失配→2064 日 K 滚日）——**残余**：desk 侧滚日
  （roll_day）不在此（world 日结编排=driver.day_end，step 内不自动），
  save 的日 K 长度多 1——world 日结编排域登记。

## 旧管线注入/读面绑定域（测试改造型——通道或读面映射）

1. `two_npc_requests_and_player_request_share_stock_price_time_rules`：
   staged_decision 注入面 + 断言读旧市场簿（bid_depth/resting_orders）。
   修向：staged 注入已有首步消费面（take_staged_intents）；断言读面换
   world_public_views（depth/best_bid）。
2. `npc_request_uses_available_cash_when_it_enters_the_next_tick`（:271
   unwrap None）：同 staged 域。
3. `cross_stock_plans_keep_independent_children_and_escrow_after_a_cancel`：
   计划注入（open_buy_plan+push_execution+shadow 事务）打镜像簿组不感知。
   修向：MutatePlanBook 通道造计划+external 通道挂子单+组内撤单断言。
4. `ordinary_adoption_rejects_exceeding_remaining_quantity` /
   `ordinary_adoption_within_remaining_passes`：计划委托采纳机制
   （AdoptionExceedsRemaining 入口防御）——runtime 下普通单不走计划采纳
   （组内路由无采纳面）。修向：采纳入口组内等价（adoption 在
   record_child_submitted——外来单认领计划腿——组内外来成交已有身份核
   跳过路径，采纳面本身未迁移）。
5. attention 3 例（opening/closing 公开簿）：断言读旧市场簿
   （index out of bounds len=0——runtime 下单在 desk）。修向：读面换
   world_public_views；或公开簿读面投影（install_market_views 的簿面）。
6. `public_step_routes_pre_open_rejection_and_continuous_place_cancel_
   lifecycle`：竞价前拒收回执事件缺失（event_bridge 有 IntentRejected——
   竞价相位拒收路径未到回执）。修向：desk 竞价拒收回执核对。

## 存档/纪元/审计域（runtime 权威面未对齐——真实差异）

7. persistence/saved_runtime 3 例：
   - `real_step_settles_and_persists_gross_capped_seller_without_cash_
     reservation`：卖家实收费用封顶（低额腿费用 cap=0）+三层现金语义
     （escrow vs purse）——组内 purse 扣未体现到回写现金读面。**费用
     封顶与 purse 回写口径双面**，建议独立实验定基准。
   - `same_tick_new_seller_route_creates_and_advances_cumulative_fee_
     audit` / `same_tick_routes_advance_one_seller_fee_debt_without_
     reusing_tick_start_audit`（left=0 right=2）：费用审计计数——审计
     面 runtime 未推进。
8. `history_reads_survive_civil_day_save_restore...`：存档恢复后
   CivilClock MarketSessionOutOfSync——恢复面与 civil 推进时序。
9. `public_step_commits_phase_boundaries_once_and_leaves_a_saveable_
   quiet_point`：fault[2018] save 校验失败——存档 quiet point 面。
10. `live_plan_partial_fill_survives_restore_and_second_real_tick_fill`：
    restore 后二拍 Trade 缺失——存档恢复域。
11. `epoch_alignment_accepted_account_personal_rng_terminal_is_exact`
    （left=(129, huge) right=(4, 3)）：组内 attention/rng 自演进与 A 类
    冻结输入确定函数的模型差异——**判型关键**：确认组内 rng 终态语义
    （可能真差异：受理账户的 rng 消费次数组内远多于 A 类预期）。

## 旧管线专有机制域（等价面不存在——登记+跳过型）

12. t3_boundary 3 例（retail traces）：retail belief 链未迁移（已证
    归因——本轮交接背景）。修向：retail 观察链组内迁移（大件）。
13. committable_state 2 例（defer_swap/Job 计数）：旧管线关键路径旧
    状态转移机制——runtime 无此机制。
14. npc_tick_preparation 2 例（completion receipt/shadow）：旧管线
    NPC 准备域（receipt 溢出失败路径/次日校验拒收）。
15. `same_tick_account_validation_envelope_can_be_canceled_without_
    refunding_the_batch_budget`：账户校验信封批预算（staged/envelope）。
16. `a1_stepfatal_form_atomicity_via_real_failing_tick`：注入事务
    原子性（staged 注入 pre-mutation 形态）。

## 建议下轮顺序

1. 存档域（7-10 共 6 例——restore/save/civil 同族，可能一因多果）。
2. epoch（11——判型后定模型差异 or bug）。
3. 读面绑定域（1-6——读面映射/通道改造，机械性强）。
4. 旧管线专有（12-16——跳过登记 or 大件迁移 retail 链）。

数字（第 10 任末，HEAD=1d9fd529）：默认 lib 1603/0/11（d0_world_drop
已知 flake 复跑绿）；env=1=27（39→28 件4 批→27 qty0）；翻转批
10/13/14/14/15 全绿；d3b4/d3b3/solmid 三关键测试单跑绿。


# D5 二轮进展（第 10 任续，HEAD=d755dfb1）

## 已修（二轮）

- 存档族 2 例（phase_boundaries+history_reads）：desk 分钟收盘段钳满
  （min(elapsed, 连续窗)）+ ends_day 拍 world 日结编排（install 前
  run_day_end_public）+ 日 K 归档三修（time 日历重写/active 空窗同式
  fault2076/多日显式拒）。
- adopt 直入 desk 初始簿（到簿序对齐——live_plan_partial_fill 的
  maker/taker 颠倒部分修复；desk 外来买腿 purse=0 例外+组端 Closed
  幂等）。

env=1：27→26（sol6th 回归已修）。默认 1605/0/11；翻转批全绿。

## 剩余 26 例拍板清单（协调者决策）

1. **Trade 事件 maker/taker 契约分叉**（1 例：live_plan_partial_fill）：
   runtime desk 产出 maker≡卖腿/taker≡买腿（Sol 六审统一——费用分账
   按腿，印花恒卖腿 ✓A 股）；旧管线事件 maker=先在簿（被动）/taker=
   来单（主动）。**事件契约**（ts_rs/前端）+费用模型二选一。建议：
   接受 runtime 语义（A 股无 maker/taker 费差，腿方向更稳）——旧管线
   测试断言改腿方向+前端契约核对；或 desk fill 事实带到簿序、事件层
   映射旧语义（工作量：前者小后者中）。
2. **epoch rng 调度公式**（1 例）：组内 reschedule_due=独立 rng+gap
   均匀（F §2.2 设计近似）；旧管线 evaluate=signal-based 概率+observe
   门（**组内每 due 必决策——决策频率也高**）。逐位等价需组内迁
   evaluate 公式（observe 门+wait 采样+共享 rng 流）。建议：迁
   （等价性核心——翻转终态迟早要过）。
3. **envelope 费用/审计域**（4 例：real_step_settles+fee_audit×2+
   book_auction_boundary）：旧管线信封账本（最低佣金债/费用封顶/累计
   审计）vs runtime purse 所有权模型。迁移 envelope 审计投影 or 接受
   purse 模型登记差异。建议：审计投影（save 的 live_envelopes 面从
   purse+desk 费用事实重建——费用实收数据组内全有）。
4. **retail belief 链**（5 例：retail_analysis+t3×3+npc_working_quote）：
   组内 retail 位无 belief 消费（decide_data 直走）。迁移=retail 位
   组内 belief 态+disclosure 投递+个人分析（与 #2 调度纠缠）。大件。
5. **旧管线专有机制**（5 例狠判型结论）：committable_state 2（defer_
   swap/Job——旧管线关键路径旧状态转移，runtime 无对应机制，**pin
   +登记等价锚**）；npc_tick_preparation 2（completion receipt/
   shadow——旧管线 npc 准备域，runtime 组内调度无此面，**pin+登记**）；
   quota_wave 1（staged 注入原子性——staged 桥专有，**pin+登记**）；
   npc_request_cash 1（staged 桥+ADR-0022 断言，**pin+登记**（组内
   purse 校验另有面））。
6. **可改造**（2 例）：two_npc（staged 注入面 runtime 有首步消费——
   断言读面换 world_public_views）；cross_stock（计划注入走 Mutate
   PlanBook+挂单走 adopt/external——**前置 #1 拍板**）。
7. **attention 3 例**：公开可见性门禁（竞价隐藏簿/开盘揭示截点）——
   desk 公开投影语义迁移（中件）。建议：下轮（与披露/公开面一起）。
8. **ordinary_adoption 2 例**：计划委托采纳机制（外来单认领计划腿）——
   组内无采纳面（外来单走独立路径）。登记 or 小迁移（record_child_
   submitted 的外来触发）。


# D5 二轮·裁决执行段（第 10 任续，HEAD=本轮末）

## 已落地（按裁决顺序）

- #2 attention evaluate 真迁（fb39814e）：signal 概率+observe 门+wait
  采样+共享 rng 续传（GroupAttention 三元组回写 rng_state）+ForceDue
  ensure_observe；decide_account 纯决策化（due 门归调用方）。
- #1 Trade 契约统一（ed3bacc3，ADR-0042）：旧管线构造腿方向化
  （连续/拍卖）+continuous_matching buyer/seller 解耦+20 处断言适配。
  复核阻断（causal/aggregate 主动腿校验漏改）已修+3 处文档残留+
  evaluate saturating_add 补齐。
- #3 envelope 审计投影（本轮）：DeskMsg::EchoResting+RestingFact
  （desk 权威费用累计/余量）→runtime_step 尾信封账本重建（nominal
  重算+charged 直取；OrderId=serial 域）。
- 翻转批 fixture 适配 observe 门（c2f5bc0f：候选序搜索）+batch4
  搜索条件强化（末 8 拍受理）。

## 遗留与拍板项（更新）

- **#3 测试阻断**：saved_runtime 3 例红因=desk A 股自成交限制
  （同账户买卖不撮——desk.rs:944 索引路径）vs 旧管线 fixture 同账户
  自成交设计——envelope 投影已就位但 fixture 无成交可验证。需拍板：
  A) fixture 改双账户（测试侧）；B) 旧管线补自成交限制（引擎对齐）
  ——A 股实盘自成交限制存在（上交所/深交所盘中间隔规则——旧管线
  无限制是缺口，建议 B 或双账户+登记）。
- epoch：遗留（seam 时序+快照一拍差深因——D5 #4 retail 专项收口）。
- live_plan：fault2010（DeskStockSpec 无 recent_prices 种子——restore
  价格历史投影，下轮 #6 域）。
- cross_stock：#6 改造（计划注入组通道+断言已腿方向化）。

数字：默认 1605/0/11；env=1=31（26 基线−0+fee 族红移（envelope 投影
落地后 fee_audit 断言从"账本空"推进为"无成交"——同自成交限制阻断）；
翻转批 5/5 全绿。


## #5 pin+登记落地（本轮末）

- 6 例 pin（committable×2/npc_prep×2/quota/npc_request）——env=1 分支
  跳过+等价锚注记（翻转批对拍/组端 Abort 可逆日志/purse 校验）。
- 复验残留 A/B/C 修复（nominal 回滚/batch4 注释勘正/original_qty 删）；
  D 登记：simulation-diagnostics 面自 StockId 数值化（c79f051c）起
  编译失修 17 错（先于本批）——causal 选腿修复暂无测试覆盖，逻辑审读
  背书，登记待修。
- protocol.rs taker_side 注释歧义残留（复验指）待 #4 retail 专项顺带。


## 自成交拍板 A 执行（本轮末，含 restore 链五层深修）

- fee_audit ×2 清零（双轨 fixture：runtime=retail 位双账户种子卖/默认=
  原玩家单账户——旧管线结算 epoch 面对种子卖不适用）。
- envelope/lifecycle 投影链落地：project_live_envelopes runtime 分支
  （ledger 投影）；restore 侧 build_ledger 的 saved-only 重建路径
  （runtime 形态——旧簿空）+2226 放行+2220 证明空窗登记（下一
  runtime_step 由 desk 权威重建）。
- restore 等价三件：DeskStockSpec.initial_recent_prices（save fault2010
  长度面——desk recent 种子+from_session 投影）；adopt 后清旧簿
  （fault2226 双域一致）；DeskRestingSeed.purse（买腿 live 资源）+
  seed 源扩 ledger（restore 形态——desk 单从账本键恢复）。
- real_step 推进五层（fault2010→2226→2218→2220→事件对拍）——续拍
  持仓下穿（restore 后 desk 种子扣减与组持仓投影的时序双计）登记
  下轮（restore 后首拍的组初始持仓与 desk 种子成交回写的扣减次序）。
- install 段回归修复（sol3/sol3b——envelope/诊断段序在 install 后）。

数字：默认 1605/0/11；env=1=27（29−fee_audit×2）；翻转批 5/5。
