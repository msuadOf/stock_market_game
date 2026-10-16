# 将随旧管线消亡的等价测试清点（任务二·只读前置分析）

日期：2026-10-17。基线：分支 `exp/std-thread-model` 工作区（实现 agent 在清尾批失败，
本文档不改动任何源码/测试）。

## 0. 判定基准与术语

- **旧管线** = `GameSession::step()/step_frame()` 的 session 侧管线编排
  （`session/pipeline` 相位、`step_pipeline_contract` 契约入口、ComputeContext
  scatter-gather 池、staged_decision 桥、candidate 克隆、TickCommitEvidence）。
- **退役时序**（依据 `.tmp/batch9-execution-plan.md`）：默认翻转
  （`step_runtime_enabled()` 缺省 1）→ C4 删 `ENGINE_STEP_RUNTIME` 开关 →
  内部退役批（C2/C3/C5）删 `step_frame`/`step_pipeline_contract`/
  `ComputeContext` 家族与 `session/pipeline` 专有机制。
- **判定口径**：
  - **死亡**：文件主体或测试项的被测对象就是旧管线机制/双路径对照，旧管线删除后无被测对象；
  - **可保留（改锚）**：断言承载 A 股语义，换到 runtime 路径或手算 oracle 后语义仍成立；
  - **与旧管线无关**：经 `ProtocolSession` 包装层或纯类型面运行，随 step() 翻转自动换轨。
- 搜索标记：`ENGINE_STEP_RUNTIME`（42 处命中）、`step_pipeline_contract`（调用 13 处）、
  `pipeline_events`（flip_helpers）、`.step_frame()` 直调（16 文件）、
  `.tmp/flip-106-failures.txt`（2026-10-15 翻转实测失败清单，lib）。
- 注意：`flip-106-failures.txt` 是 D 批（D1-D5）**迁移面**（在修，保留为主）；
  本清点聚焦**等价对照面与已登记"旧管线专有机制"死亡面**。

## 1. 文件级判定总表（32 文件）

### A. 死亡（8 文件）——双路径等价对照/旧管线专有机制本体

| # | 文件 | 测试数 | 死因 | 断言去向 |
|---|---|---|---|---|
| 1 | `packages/engine/tests/runtime_flip_batch1.rs` | 10 | 双路径对照（pipeline_events vs runtime_events）；旧管线侧与"两路径一致"断言无对象 | runtime 单侧断言改锚独立（见 §2） |
| 2 | `packages/engine/tests/runtime_flip_batch2.rs` | 10 | 同上（竞价/相位门禁批） | 竞价域断言改锚（§2 竞价） |
| 3 | `packages/engine/tests/runtime_flip_batch3.rs` | 11 | 同上（撤单/到期/拒收批） | 生命周期断言改锚（§2 生命周期） |
| 4 | `packages/engine/tests/runtime_flip_batch4.rs` | 11 | 同上（T+1/日结批；其中 7 项为 runtime 单侧，可整段改锚保留） | T+1/日结/守恒断言改锚（§2） |
| 5 | `packages/engine/tests/runtime_flip_batch5.rs` | 12 | 同上（存档/恢复/公司域批）；`flip_opt_in_runtime_step_...` 为开关子进程实测，随 C4 死 | 存档往返断言改锚单侧 |
| 6 | `packages/engine/tests/pipeline_contract.rs` | 4 | TickPhase/EventSourceIndex/ReceiptLocalKey 纯排序契约，随 `session::pipeline` 类型退役消亡 | **无 A 股语义，不需再锚**（若 runtime 复用类型则原地保留） |
| 7 | `packages/engine/src/session/pipeline/executor_perturbation_tests.rs` | 13 | ComputeContext 执行器扰动机制（install/绑定/scoped 泄漏隔离）随 C5 整族删除；13 项全在 `with_executor_perturbation`+`step_pipeline_contract` 上 | 成交**执行序无关性**语义再锚到并发撮合内核（§2 生命周期/自成交交叉） |
| 8 | `packages/engine/src/session/pipeline/commit_evidence_tests.rs` | 8 | TickCommitEvidence/market_tick_transaction（prepare/commit 两段事务）=旧管线证据与事务结构 | 现金/股份不双计的结算原子性语义由 runtime Abort 回滚族承载（sol3/sol3b 已在 failure_tests） |

### B. 部分死亡/改锚（16 文件）——文件保留，删或改个别用例

| # | 文件 | 死亡用例（现名） | 死因/登记 | 处置 |
|---|---|---|---|---|
| 9 | `tests/common/flip_helpers.rs` | `pipeline_events()` 助手 | 旧管线驱动器本体 | 删；`assert_order_event_invariants`+3 项 closure 测试路径无关，保留并转 runtime 事件流专用 |
| 10 | `src/runtime/equivalence_tests.rs`（11 项） | `old_pipeline_and_runtime_driver_both_drive_without_divergence`、`envelope_conservation_equivalence_runtime_vs_pipeline`、`batch8_final_verdict_flip_requires_integration_migration_batch`（决策记录锚） | 双路径对照本体 | 删 3 项；其余 8 项 runtime 单侧（stable key/seq 通道/market_views/K 线回写/disclosure observer）保留 |
| 11 | `src/session/failure_tests.rs`（8 项） | `typed_private_plan_failure_is_poisoned_without_mutating_authority`（经 `pipeline::plan_tick` 直驱） | plan_tick=旧管线相位入口 | 溢出拒收+不改权威+poison 语义改锚 runtime 路径；其余 7 项（poison/回滚/锁步/hash）保留 |
| 12 | `src/session.rs`（测试模组） | `successful_step_transfers_one_complete_old_state_and_releases_it_once`、`failed_step_does_not_transfer_authority_to_drop_backend`（committable_state_tests，env-skip 已登记） | candidate 克隆/权威交接=旧管线机制；runtime 用 Abort/回滚 | 等价锚已由 `sol3_runtime_step_failure_stops_install`/`sol3_runtime_install_failure_rolls_back_advanced_state`/`sol3b` 承载（失败不留半安装态）；克隆计数类断言不再锚 |
| 13 | `src/session/decision_chain.rs`（chain_restructure_tests） | `institution_boundary_mixed_pause_latches_risk_pressure_until_recovery`、`urgency_paused_child_in_no_cancel_window_stays_pending_without_releasing_resources`、`filled_child_cancel_failure_reconsiders_the_live_plan`、`mid_day_restructure_in_a_non_cancellable_phase_keeps_plan_and_child`（4 项 env-skip） | 相位伪造+旧市场簿+staged 注入面=旧管线；机构边界/暂停恢复/撤单窗**语义未死** | **必须经 runtime 重述**（D1 域最大块）：冻结现金不复用、暂停闩锁至自审恢复、非撤窗（收盘竞价段）不撤单 |
| 14 | `src/session/plan_root_quota_wave_tests.rs` | `a1_stepfatal_form_atomicity_via_real_failing_tick`（env-skip） | staged 注入事务原子性=旧管线 staged handover 桥 | 等价锚=组端 Abort/undo 可逆日志（batch4 civil 回滚对账已载）；失败零部分提交断言可再锚 |
| 15 | `src/session/pipeline/quote_expiry_tests.rs`（13 项） | `quote_expiry_post_shadow_failure_discards_order_receipt_and_event`（env-skip）+ 其余 12 项锚定旧管线 expiry 相位/收据链/全局收据序 | post_shadow 注入面、收据链单源性=旧管线 shadow 机制；**TTL 到期释放资源语义未死** | 语义再锚（§2 生命周期 #4）；收据链排序/二次应用拒绝类机制断言随退役消亡 |
| 16 | `src/session/pipeline/npc_tick_preparation_tests.rs` | `failed_npc_completion_receipt_discards_the_entire_tick_shadow`、`next_tick_account_validation_rechecks_npc_orders_against_current_cash`（2 项 env-skip） | staged_decision 桥/ingress cursor 注入面 | 等价锚已登记（组端 purse 所有权+fault1004 类失败不安装副作用）；下一拍现金重验语义由 purse 模型天然承载，建议留一条 runtime 断言 |
| 17 | `src/session/pipeline/authoritative_tick_tests.rs`（12 项） | `npc_request_uses_available_cash_when_it_enters_the_next_tick`（env-skip） | staged 批"驱动中检视"形态 | 同上 purse 等价；其余 11 项（相位派发/证据提交/零量事件/预开拒收-连续买卖生命周期/统一簿拍卖）属 D2/D3 改锚保留面 |
| 18 | `src/session/pipeline/book_auction_boundary_tests.rs` | `same_tick_account_validation_envelope_can_be_canceled_without_refunding_the_batch_budget`（env-skip） | 密封批预算复用=旧管线 envelope 批装配 | 撤单即释放（组端 purse）语义建议再锚一条；竞价边界其余项 D 批改锚 |
| 19 | `src/session/pipeline/t3_boundary_tests.rs`（13 项） | 全文件锚"每 tick 生产恰一次/root 交接先于首交付/staged 不计生产/trace 可见性"旧管线生产契约；`staged_injection_is_not_counted_as_production`、`sealed_tick_is_not_backfilled_after_commit` 显式走 `step_pipeline_contract` | t3 生产时序=旧管线编排面；runtime 生产/收束结构不同 | 恰一次语义按 D2 计划翻译为 runtime 收束点断言；trace/attempt 计数类机制断言随退役消亡 |
| 20 | `src/session/pipeline/spine_v0_contract_tests.rs`（4 项） | 串行脊 v0 契约（失败保权威 RNG/id/游标、空 tick 仍交付合法帧）；2 处 `step_frame` 直调 | spine v0=旧管线骨架 | D2 改锚（在 flip-106 清单内）；空 tick 合法帧断言可再锚 |
| 21 | `src/session/protocol/civil/epoch_alignment_tests.rs`（8 项） | `epoch_alignment_accepted_account_personal_rng_terminal_is_exact` 内 twin 支路（`step_runtime_enabled()` 时 twin 走 `step_pipeline_contract` 保持同基） | 双轨同基依赖旧管线入口 | 改锚 runtime-native 双轨（同档 restore 后各自驱动逐位相等——"冻结输入确定函数"语义必须保留）；epoch 恰一次/RNG 边界断言保留改锚 |
| 22 | `src/session/shared_ingress.rs`（tests mod，10 项中 3 项） | `real_step_chain_delivers_each_external_input_exactly_once_across_forks`、`production_step_cash_winner_follows_cross_source_receive_order`、`production_step_same_price_stock_order_follows_cross_source_receipts`（3 处 `step_pipeline_contract`，"staged 注入面为旧管线专有"已登记） | staged handover 桥 | **同价赢家/跨源 FIFO 语义必须改锚 runtime**（对应已拍板政策：tick 内任意/组间 FIFO）；恰一次交付断言再锚 |
| 23 | `src/runtime/drive_tests.rs`（2446 行） | `sol5_from_session_restores_attention_state` 的前缀（旧管线推 5 tick 再交 runtime） | 混合驱动前缀 | 改为 runtime 自推后重取 attention；其余（sol6 history_len、day boundary 等）runtime 原生保留 |
| 24 | `src/session/persistence/saved_runtime_tests.rs`（1837 行） | 无整项死亡；3 项费用测试已**双轨分支**（1081/1233/1338 读 env 切 fixture） | else 分支=旧管线单账户种子卖 | 删 else 分支即成单轨独立锚；建议补手算数值（§2 费用） |

### C. 与旧管线无关/随翻转自动换轨（8 文件）

| # | 文件 | 依据 |
|---|---|---|
| 25 | `tests/tick_frame_commit.rs`（12 项） | 全经 `ProtocolSession.step_frame`（协议包装层随 step() 翻转）；被测面=TickFrame/CivilUpdate 契约（日界 barrier、周末 BeforeOpen/CivilAdvance 恰 kinds、暂停偏好、静默预开 tick 不丢、连续点与权威 PriceTick 一致） |
| 26 | `tests/protocol_host_restore.rs`（4 项） | ProtocolSession 层 tick_batch/恢复契约（帧保序、日内档拒绝公共恢复 fault3013 不改源） |
| 27 | `tests/protocol_civil.rs`（3+ 项） | civil 报告刷新/发布重连，协议层 |
| 28 | `tests/company_event_contract.rs` | 公司披露经协议层 |
| 29 | `tests/session.rs`（1 处 step_frame=ProtocolSession 周末蜡烛桥接；其余 step()） | 日历语义（362 根虚拟前置蜡烛） |
| 30 | `src/session/protocol/civil/decision_failure_witness_tests.rs`（5 项） | ProtocolSession 层失败见证（失败不 poison 的重试面、回执恰一次窗口、发布失败回滚业务）；协议入口语义与引擎路径无关 |
| 31 | `src/session/protocol/civil/single_owner_tests.rs`（9 项） | 协议单主面（wire 恰一次序列化、事实键唯一全覆盖、披露公私分离、**real_trade 双回执腿+T+1 交收**、失败 tick 零发布重试自原游标） |
| 32 | `src/session/protocol/civil/day_end_npc_tests.rs`（1 项） | 日结归档跨日续跑，协议层 |

**不在本表但同属翻转迁移面**（flip-106 清单内的 D 批改锚对象，保留为主，不属"等价测试死亡"）：
`t4_market_data_owner_tests`、`ready_ingress`、`attention_tests`、`phase_timing`、
`npc_lifecycle_projection`、`continuous_*` 系列、`saved_runtime` 认知档案/恢复项等
——由实现 agent 的 D1-D5 批逐项翻译到 runtime 路径。

## 2. 死亡测试中值得再锚定的语义断言清单（按域，共 27 条）

> 原则（呼应任务 #103"语义单测细化"）：再锚定 = 从"双路径一致"改锚为
> **runtime 单侧 + 手算 oracle/恒等式**，不再依赖旧管线输出做参照。

### T+1（2 条）
1. 【batch4_t1_unfreeze_clears_frozen_after_day_end（batch4 #3，runtime 单侧可整段保留）】
   首日有当日买入 ⇒ 日结前 frozen_today>0（注入单 100 股×1000 分=冻结 100 股，可手算）；
   `day_end` 后 world 侧全账户 frozen_today 全零（解冻恰在日结点）；账户现金非负。
2. 【single_owner_tests::real_trade_fact_retains_both_receipt_legs_and_t1_delivery（C 类保留，列出备查）】
   成交事实保留双腿回执且 T+1 交收腿可见——建议补"当日买入股份冻结不可卖"显式手算用例。

### 费用（3 条）
3. 【same_tick_routes_advance_one_seller_fee_debt_without_reusing_tick_start_audit】
   同拍两腿（100+1000 股）共享同一累计卖方费用审计：首腿毛额 100 分 < 最低佣金
   （`commission(100)>100` 最低佣金债务成立），第二腿补收首腿未付 nominal，
   `charged == saved_fee_components(累计 nominal)`，剩余量 200；无 SettlementError。
   （已有 runtime 分支——删 else 后补手算数值即独立锚。）
4. 【same_tick_new_seller_route_creates_and_advances_cumulative_fee_audit】
   同拍新建卖方路由创建并推进累计费用审计（同上口径）。
5. 【real_step_settles_and_persists_gross_capped_seller_without_cash_reservation】
   买方结算=毛额+佣金+过户费（`gross + commission(gross) + transfer_fee(gross)` 手算式
   已在断言中）；部分成交后卖方 envelope 保持 live、存档往返合法。

### 自成交（2 条）
6. 【batch1_trade_event_fields_wellformed #8】Trade 事件 maker≠taker（A 股自成交限制）；
   价格>0、数量>0。runtime 侧改锚为独立断言。
7. 【saved_runtime 双轨注释登记】旧管线**缺自成交限制**（已知缺口随退役消亡）；
   runtime desk 不撮同账户对手——建议新增显式单测：同账户买卖同价挂单不产生 Trade。

### 生命周期（5 条）
8. 【flip_helpers::assert_order_event_invariants（+3 项 closure 单测）】逐单局部不变量：
   撤销必先同键受理、余量不超；受理 id 全程不重复；同 id 撤单恰一条；
   账户聚合（受理−撤销）≤成交触碰量。路径无关，转 runtime 事件流专用。
9. 【batch3_exact_closure_on_no_trade_fixture #11】无成交 fixture 上精确闭环：
   每张受理单恰一条终结撤单（键集相等）、撤单余量==受理余量分毫不差。
   再锚时用确定性注入（arrival=0+玩家单）替代 seed 搜索。
10. 【batch3 #3/#5】OrderCanceled 余量>0、id 正（serial 从 1 起）；
    OrderAccepted 价格正/余量正——字段域断言，runtime 单侧改锚。
11. 【quote_expiry 域】TTL 到期语义：买单到期解冻现金、卖单到期零现金释放股份、
    到期不影响拍卖/玩家单、到期事件带唯一键并与诊断恰一次伴随。
    （收据链排序/二次应用拒绝等机制断言随旧管线消亡，不再锚。）
12. 【executor_perturbation 域】执行序扰动无关性：任意跨股/worker 输出重排下
    tick 不失败、tick 恰 +1、玩家每股成交量恒 300、无 poison——再锚到
    std-thread 并发撮合内核的"结果与线程完成序无关"门禁（呼应并发撮合政策）。

### 守恒（3 条）
13. 【batch1 #9 / batch4 #9】无外部注入时全账户现金总量不增。现界
    `base + 5_670_000_000` 过宽——再锚应收紧为恒等式：
    Σ现金 + fee_sink 累计 == 初始总量（派息/公司动作显式入账后另计）。
14. 【equivalence_tests::envelope_conservation_equivalence（死亡项）】envelope ledger
    守恒面随旧管线消亡；等价锚=runtime 侧"现金+fee_sink=初始"（DeskReport 批 3 起已有），
    再锚为独立恒等式断言（跨多 tick+跨日）。
15. 【equivalence_tests::runtime_driver_cash_conservation（保留项，列出备查）】
    回写通道不产生负值/溢出：全账户现金非负。

### 日结（5 条）
16. 【batch4 #1/#4】跨日驱动 day 恰 +1（240 tick+日结）；双路径 day parity 侧死，
    保留"恰 +1"单侧。
17. 【batch4 #7】seq 全局单调跨日结不重置、不重号（与 batch3 #8 合并）。
18. 【batch4 #8/batch3 #6】恰 240 tick（日终点）口径：日终清簿后无悬挂挂单
    （受理−撤销 ≤ 成交触碰，drained horizon 无合法豁免）。
19. 【batch4 #11（runtime 单侧可整段保留）】civil 日结失败**全量回滚**
    （World Abort、driver seq、事件缓冲、会话 day/seq 全回基线）+ 修复后重试恰一次
    （day 恰 +1、清簿撤单不重复）+ 重试后可续跑——旧管线 shadow 原子性等价锚全在此。
20. 【batch4 #10】第二日开盘竞价窗重新出现 AuctionTick（日结后相位重置）；
    另【tick_frame_commit（C 类保留）】civil barrier 恰 kinds：
    周六 CivilAdvance、周日 BeforeOpen、周五 AfterClose 无 BeforeOpen。

### 竞价（6 条）
21. 【batch2 #1/#2】开盘竞价窗（申报窗 tick<10）内每 tick 有 AuctionTick 且
    phase=CallAuction。
22. 【batch2 #3】开盘清算（AuctionCompleted）出现在申报窗（前 2/3=10 tick）后；
    clearing_price>0 域合法；无交叉时无价无量为合法形态。
23. 【batch2 #6】连续竞价窗（tick 17-236）无 AuctionTick；237+ 收盘竞价窗恢复。
24. 【batch2 #7】申报窗内新申报产生 OrderAccepted 而非 Trade（积累语义：零 Trade）。
25. 【batch1 #6/#10 + batch2 对照侧】20 tick 内必见竞价窗事件（对照侧死，单侧保留）；
    相位窗一致性（越窗后 CallAuction 事件不再出现）。
26. 【book_auction_boundary（死亡项）/authoritative_tick 统一簿拍卖（D 批改锚）】
    收盘竞价不复制连续簿在册子单、统一簿拍卖段不撮合只入簿、清算失败保簿/现金/股份
    ——竞价边界语义经 runtime 改锚保留（death 项仅"批预算复用"机制断言）。

### 附：seq/事件面（非 A 股域但等价面支柱，3 条）
27. 【batch1 #2、batch3 #2/#8、batch4 #7、batch2 #8】事件 seq 严格单调不重号（两路径各自
    +跨日）；事件类型集合落在引擎合法词表内；双路径事件总量同数量级（防静默吞事件）——
    "双路径"侧死，"runtime 单侧 seq 单调+词表+非空"改锚。

## 3. 统计与交接要点

- **文件总数 32**：死亡 8 / 部分死亡-改锚 16 / 与旧管线无关（随翻转换轨）8。
- 死亡用例（函数级，含部分死亡文件内）：env-skip 已登记 12 + 双路径对照
  （flip1-5 共 53 项中约 24 项为"对照/parity"性质）+ equivalence 3 + failure 1 +
  ingress 3 + t3/spine/quote_expiry/commit_evidence/perturbation 机制项 ≈ **90+ 项**。
- 再锚定断言：**27 条**（T+1 2 / 费用 3 / 自成交 2 / 生命周期 5 / 守恒 3 / 日结 5 / 竞价 6，
  另附 seq/事件面 1 条合并计）。
- 风险提示：① `tests/pipeline_contract.rs` 与 quote_expiry 收据键排序类断言若
  `session::pipeline` 类型被 runtime 复用则不死——退役批执行时按符号复检；
  ② flip-106 清单（D 批迁移面）与本清点互补：106 内文件**不因等价消亡**，
  仅其中已登记 env-skip 的 12 项随旧管线退役删除；③ saved_runtime 3 项费用测试
  已双轨，删 else 分支即最小改锚，但需按铁律一逐条登记裁剪依据。
