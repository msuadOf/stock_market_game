# 任务G：所有权转移模型迁移——批次实施计划

> 产物日期：2026-10-15。设计者：任务G subagent（只读分析 + 本文件，未改任何 worktree 源码）。
> 输入：A（NPC 所有权）/ B（session 分片）/ C（协调者协议）/ D（rayon 退出面）四份设计产物 +
> 用户 2026-10-15 确认：**资源实体所有权转移模型**做正式引擎迁移。
> 原型依据：`.worktree/std-thread-model/packages/std-thread-proto/src/{oworker,orun}.rs`
> （所有权转移臂，962 t/s @ s5、10 组+5 交易部、完整跨股语义、零锁零共享）。
> 迁移对象：`.worktree/spine-simplify-01/packages/engine/src/`（HEAD `55b898c7`，431 个 .rs、165,077 行）。
> 模型要点（用户决策）：一股一交易部持久线程 + NPC 分组线程 + 实体 move（CashPurse/ShareLot
> 实扣实转，非 escrow 预留）+ 代码本体零线程数概念（组数按名册规模自适应，无魔数）+ CPU 限制走 OS 级。
> 修订（2026-10-15，Sol 终审最小修改集）：批0 补未点名文件归桶与 orderbook 去留（§1.4/§1.4.1）、
> §1.1 DeskBatch 加 tick/round 防御字段、批2 补 Abort 回滚等价判据、批3 补五项受理/相位判据、
> 批5 接 DayEnd/AdminBatch 及三项验收、风险清单补 12/13（RECV_TIMEOUT 相位感知、上行负载口径）。

## 0. 口径与实测基线

- **拓扑**：coordinator（tick 脊）+ G 个组线程（各持一片 NPC 账户：现金/持仓/附属状态）+
  N 个交易部线程（各持一股 book/行情/逐股账本）。数据面组↔交易部直发（DeskIn ops 下行、
  Entity 批次上行），coordinator 只广播 Start + 收束 G 个 GroupStats + N 个 DeskDone
  （oworker.rs:1-19 时序不变量：交易部 flush 实体先于 DeskDone，coordinator 收齐全部
  DeskDone 后才广播下一 Start ⇒ 组线程在处理 Start(N+1) 前已收齐 tick N 全部实体）。
- **测试实测**（spine HEAD，`grep -c '#\[test\]'`）：src 单测 **1441** + tests/ 集成 **1117**
  = **2558**。任务书口径 1474 与实测不符（疑为旧快照/部分口径）；本计划按实测规划，最终以
  批0 基线运行的 `cargo test` 计数为准。
- **文件面合计**：生产改动 ~44 + 新增 runtime ~6 + 测试触碰 ~32 + 构建/文档 ~9 ≈ **90 文件**
  （与 D §4 的 ~85 一致，差额为 runtime 新增）。
- **实施位置**：从 spine-simplify-01 HEAD 拉独立 worktree（worktree 独占纪律），原型代码从
  std-thread-model/packages/std-thread-proto 移植。

## 1. 逐文件对照（engine/src，五类：K 零改 / K+ 接缝级 / R 重写 / D 删除 / N 新增）

### 1.1 N 新增——runtime 模块（`src/session/runtime/`，批2 落地）

| 文件 | 内容 | 蓝本 |
|---|---|---|
| `runtime/mod.rs` | 模块声明与导出 | — |
| `runtime/protocol.rs` | `DeskIn{CancelAll,Place}`/`Entity{CashProceeds,CashRefund,BoughtShares,RefundedShares,Closed}`/`DeskBatch`（含 tick/round 冗余防御字段：组端核对批次属当前 tick/轮，错配显式失败——Sol 终审）/`GroupIn{Start,Batch,Shutdown}`/`DeskMsg{Ops,Drain}`/上行枚举（含 `Failed` 显式上抛） | oworker.rs:37-131 + F §1.3 |
| `runtime/entities.rs` | 现金/股票实体、严格所有权计数（`EntCounters`）、组端在途登记 | oworker.rs:167-238 |
| `runtime/desk.rs` | 交易部主循环 + 挂单携实体的簿（撮合/T+1/费用/涨跌停/整手/到期/撤单/收据与 typed facts 产出、订单号块预分配） | oworker.rs:203-1009 + book.rs |
| `runtime/group.rs` | 组主循环（Start 解冻→内联 decide→实体扣除→分批直发→收实体落账）+ 期末名册上交 | oworker.rs:1011-1250 |
| `runtime/coordinator.rs` | tick 广播/收束计数/公共态安装/Drain→Shutdown→Final 收尾/panic 运输 | orun.rs:99-613 |

引擎化改造点（相对原型，逐项对应 C §1.4 差异表）：T+1 按 civil-day（非 tick 近似）、
DeskDone 承载 receipts/facts/行情收尾（对齐 `IncrementalContinuousStockFinish`）、
阶段元数据分派（Continuous/集合竞价/PreOpen，phase.rs PhasePolicy）、收据身份从
coordinator 块预分配、守恒审计含 fee_sink 与公司域。

### 1.2 R 重写——核心编排与状态面

| 文件（行数） | 改动 | 依据 |
|---|---|---|
| `session.rs`（5050） | `CommittableSessionState` 重组：四股域 → lease/epoch、账户域 → 组线程、构造面/`clone_for_tick_shadow`/`commit_from`/FieldId 台账（30/32 项全集）/`COMMIT_FIELD_EXPECTED` 同步 | B §1/§3 |
| `pipeline/mod.rs` | `plan_tick` 五步脊改 coordinator 驱动；`in_place_scope` 重叠臂、`precomputed_stock_shards` 快速路径退役 | C §1.1① |
| `pipeline/stock_stream.rs`（2401） | 协调层（dispatch/poll_progress/准入/通知/workers()==1 让位）整体退役；`continuous_shards` 构造迁 desk 初始化 | D §1.5 |
| `pipeline/market_tick_transaction.rs`（589） | join/drive 段 → 广播+收束；`finish_market_execution` 收束侧保留 | C §5 |
| `pipeline/npc_tick_preparation.rs`（1845） | 决策生产重组：按组密封输入、内联派工 | C §1.1② |
| `session/decision_chain.rs`（5222） | 账户决策链内联组线程（纯计算逻辑保留，并行组装消失） | D §1.3 |
| `session/views.rs`/`self_views.rs` | 视图组装进组线程（广播快照 + 私有状态） | D §1.3 |
| `pipeline/incremental_continuous_stock_shadow.rs`（925） | shadow 常驻化 = desk 持有 committed/working | B §1.1 |
| `pipeline/continuous_matching.rs`（1446） | 撮合内核进 desk；≥512 par_iter 阈值分支消解 | D §1.3 |
| `session/initial_allocation.rs` | seed_float/split_float → 组名册构造；**全局池语义保留**（每股流通盘摊全体 NPC、跨股持仓常态） | A §1.3 |
| `pipeline/adaptive_plan_chain.rs`（1301）+ `session/plan_chain_candidates.rs`（1298）+ `pipeline/ready_stock_stream.rs`（221） | 计划续行 → 多轮协议（外股 ops 上报→定向续轮） | C §2.3 |
| `pipeline/account_validation.rs`（1237）+ `_driver` + `_context` | 校验收束面改从 desk/组报告聚合 | D §1.3 |
| `pipeline/account_settlement.rs` + `settlement.rs`（227） | 结算 → 组端实体落账 + 回执应用（费用/税计算逻辑保留） | A §4.5 |
| `pipeline/candidate_commit.rs`/`candidate_composition.rs`/`commit_evidence.rs` | commit 路径两域两策（全局 candidate + 股/组 epoch 转正） | B §3.2 |
| `session/persistence.rs`（2318） | 保存=收束点三方收集；恢复=重建线程拓扑 | B §3.3 |
| `session/account.rs`（761） | Account → 组名册条目（实体承载：现金标量+本股仓+跨股 secondary，T+1 冻结字段保留语义） | oworker npc.rs 形状 |
| `src/compute.rs`（1316） | 主体删除（见 §1.4）；期间作为过渡 seam 存活到批7 | D §1.1 |

### 1.3 D 删除——机制退役

| 文件/面 | 理由 |
|---|---|
| `pipeline/npc_decisions.rs`（790） | 账户任务流（chunk 派发/TaskDied 池运输/准入/让位）整体消失；`decide_npc_account_from_shared`（:180-286）保留复用进组线程 |
| `pipeline/npc_split.rs` | 读写分裂为所有权模型取代（borrow 纪律不再需要） |
| `pipeline/live_reservations.rs`（717）+ `pipeline/local_admission.rs`（705） | `AccountReservation` 预留映射 → 严格所有权实体扣除（在途资金不在 NPC 账上） |
| `session/account_book.rs`（583）+ `session/account_paged_map.rs`（381） | COW 分页 → 组线程私有存储（history_reads/npc_attention/retail_experience/belief_participants 同） |
| `pipeline/read_only_start.rs`（539） | 持久线程天然获得"预备"，快速路径机制无对象 |
| `compute.rs` 全部池面（批7）：ComputeContext/PoolAdmission/Condvar 容量/active_width/探针/install/record/parallel_map_chunks/WASM Scope 垫片 | D §1.1/§2 全清单 |

### 1.4 K+ 接缝级（签名/调用点，机械改动）

- **ComputeContext 引用 21 文件**（D §0）：account_book、account_paged_map、retail_analysis、
  decision_chain、notices、account_settlement、account_validation×3、candidate_commit、
  continuous_matching_adapter、continuous_matching、decision_snapshot_capture、
  incremental_continuous_stock_shadow、institutional_experience_projection、ledger_conservation、
  ledger、live_reservations、market_tick_transaction、mod、npc_split、npc_tick_preparation、
  ready_ingress、receipt_aggregation、retail_projection、settlement、stock_execution_transaction、
  stock_stream、plan_chain_candidates、protocol/civil/session、session.rs、self_views、views、
  phase_timing——`Option<&ComputeContext>` 参数与 `install/parallel/record` 包装逐点拆除。
- **散点 par_iter/join**（批1，D §1.2/§1.3 非股维度 ~25 文件）：notices、retail_analysis、
  decision_snapshot_capture、settlement、retail_projection、account_validation、
  account_paged_map、account_validation_context、live_reservations、continuous_matching（阈值臂）、
  ledger_conservation、account_book、npc_state_projection、read_only_start、indicators.rs:256、
  market_tick_transaction.rs:392 等 → 串行或 std::thread 临时分片（首错按索引序合同保持）。
- **desk/组归属重挂**：quote_expiry.rs（→desk expire_due）、private_stock_ledger.rs（→desk）、
  price_resolution.rs、envelope.rs/envelope_projection.rs（→coordinator，跨股 key 空间）、
  receipt_aggregation.rs、fill_fact.rs、order_receipts.rs、day_end.rs/book_auction_boundary.rs
  （→coordinator 串行全局事务）、attention.rs/history_reads.rs/experience（→组）、
  candles.rs（→desk）、hash.rs/verify_metrics.rs（聚合口径扩展）、shared_ingress.rs/ready_ingress.rs/
  player_candidates.rs（玩家冻结与路由）、executor_perturbation.rs/phase_timing.rs（绑定重设计，
  见 §4.3）、diagnostics.rs（2377 行，投影收集 seam）、lib.rs（导出清理）。
- **K 零改**：`company/`（89 文件）、`accounting/`（36）、`strategy/`（25，decide_data 原型已直用）、
  `plans/` 域逻辑（17）、`experience/`（10）、`information/`（8）、`calendar/`（6）、
  `behavior/`（3）、money/config/market/stock/intraday_average/observation/shared_history/
  civil_clock/deferred_drop（仅注释更新，D §1.6）。
- **orderbook/ 去留决定**（Sol 终审，批0 冻结清单）：desk 用原型自有簿（价格档 FIFO——
  价格优先已按 memory `concurrent_matching_policy` 取消，现 `BookState` 的价格-时间
  `RestingKey` 优先级不再成立）⇒ 簿实现退役（`book_state`/`resting_index`/
  `persistent_index`/`filled_orders` + `orderbook.rs` 撮合核心，随批3/批7 清退）；
  **仅留领域类型**：`Side`/`AccountId`/`OrderId`/`js_safe_u64`/`MatchingMode`/
  `AuctionClearingReject` 与 `auction_clearing` 清算价纯函数——实测被 plans/strategy/
  information/session::protocol/market.rs/book_auction_boundary/day_end 生产引用。
  批0 核对引用面后冻结最终清单。

#### 1.4.1 未点名 pipeline 文件归桶（Sol 终审补遗，20 文件；批0 逐条对源码核证后冻结）

| 文件（行数） | 桶 | 处置 |
|---|---|---|
| `conservation.rs`（174） | K | `ResVec`/`ConservationBasis` 守恒向量纯函数；收束侧守恒审计（ledger_conservation 群）继续用，聚合输入改三方报告 |
| `continuous_tick_finalizer.rs`（513） | R | candidate 收尾编排 → coordinator 收束面（收尾/投影/事件安装单点，F §4.1 ⑤） |
| `event_collection.rs`（93） | K+ | 事件 seq 分配与稳定键唯一性校验进 coordinator（输入改 DeskDone facts） |
| `event_key.rs`（177） | K | `EventSourceIndex`/事件稳定键纯类型，coordinator 继续用 |
| `execution_fact_producers.rs`（174） | K+ | 股票处理事实→EventFact adapter 保留；输入源改 DeskDone fills/receipts |
| `session_fact_producers.rs`（127） | K+ | 账户校验拒绝→事件 adapter 保留；输入改组端 skip 报告/DeskReceipt |
| `intent_candidates.rs`（316） | K+ | `IntentCandidate`/`CandidateSource`/稳定键身份保留（事件键依赖）；预留工作清单语义随批5 消失 |
| `decision_snapshot.rs`（310） | D | 密封 `DecisionSnapshot` 无对象（组自有账户，F §8 决策视界行）；等价输入=组私有状态+广播快照（§1.2 views 行，批4） |
| `decision_resources.rs`（327） | D | 依赖 `AccountBook`+`LiveReservations`（双退役，批5） |
| `ledger_paged_map.rs`（403） | D | candidate COW 分页无对象；envelope ledger 常驻 coordinator 后直接写（批5） |
| `ledger_candidate.rs`（56） | K | 回执 normalize/journal-rank 排序纯函数，coordinator 收据投影继续用 |
| `ledger_rows.rs`（112） | D | `SharedRows` Arc/Weak 写隔离随 COW 面退役（coordinator 单线程独占后无对象） |
| `ledger_validation.rs`（402） | K | envelope 回执审计纯函数继续用（输入 DeskDone 守恒行/回执） |
| `receipt_key.rs`（247） | K | `IndexedReceiptKey`/`JournalRank` 身份类型继续用 |
| `transition.rs`（356） | K | 买卖腿 `ReceiptDelta`/`ResVec` 增量纯函数；desk settle/收据组装继续调用 |
| `transaction_error.rs`（66） | K | typed fatal 透传 helper（`into_fatal`）新拓扑沿用 |
| `continuous_lifecycle_projection.rs`（779） | K+ | Continuous 生命周期诊断投影进 coordinator 收束面（输入改 facts/receipts） |
| `continuous_projection_transaction.rs`（29） | D | 唯一函数已 cfg(test)（生产并入 finalizer 单点）；测试等价断言随收束面重述（批3/5） |
| `session_execution_transaction.rs`（184） | D | apply 族已 cfg(test)、`Output/Error` 类型被 finalizer 生产引用——随 finalizer 重述并入 coordinator 收束面（批5） |
| `phase.rs`（274） | K+ | `TickPhase` 脊词汇瘦身：脊内相位随消息驱动消失，保留收束步/worker 轮次标识供 phase_timing/诊断（风险 3 绑定重设计） |

### 1.5 crate 外（批9）

server：`apps/server/src/lib.rs:169-185`（host_compute_context）、`actor.rs:981-1007/1548`
（compute 字段与 compute_workers）、`fatal_tests.rs`、`tests/actor.rs:182-201`、
`examples/server_max_speed_probe.rs`（rayon_threads 字段换 OS 口径）；web-wasm 两处 +
`apps/web/vite.config.ts:8`（COOP-COEP 核对后退役）；Cargo 三处（根 workspace:22、engine:54、
server:28）；engine examples 5 个。

## 2. 增量批次切分（12 批，每批独立编译+测试全绿）

| # | 批次 | 范围（核心文件） | 依赖 |
|---|---|---|---|
| 0 | **基线冻结与等价设施** | 无代码：spine HEAD 55b898c7 全量测试计数+结果、s5 负载基准（独占机器）、等价判据→现有测试簇映射表、语义差预登记（§4.4）、未点名 pipeline 文件归桶核证冻结（§1.4.1 表 20 文件逐条对源码）、orderbook/ 去留清单冻结（§1.4，仅留领域类型+清算价纯函数，引用面核对） | — |
| 1 | **非股维度并行原语替换** | §1.4 散点 ~25 生产文件 + 三处 join（read_only_start.rs:113、indicators.rs:256、market_tick_transaction.rs:392）+ record 调用点（D §1.4）；compute.rs 本体不动（池供股维度编排暂用） | 0 |
| 2 | **runtime 骨架（新增不接线）** | §1.1 六文件从原型移植 + 引擎化（§1.1 清单）；cfg(test) 单测；不接入 GameSession | 0 |
| 3 | **股域 desk 接线（E 阶段）** | stock_stream 协调层退役、shadow 常驻化、continuous_matching 进 desk、quote_expiry→desk、四股域→StockLease、身份块预分配；**决策仍留 coordinator**（npc_decisions 暂用） | 1,2 |
| 4 | **决策内联组线程（账户权威暂留）** | 组线程持密封输入内联 decide、intents 回 coordinator 走现有准入；npc_decisions 流 machinery + npc_split + mod.rs 重叠臂退役；decision_chain 股维度 par_iter 消解 | 3 |
| 5 | **账户所有权迁移（实体模型落地，最大批）** | 账户+附属状态→组私有、initial_allocation 组名册、live_reservations/local_admission/AccountBook/paged_map 删除、settlement→组端实体落账、views/self_views 重写；**DayEnd/组端日结/AdminBatch 接线**（Sol 终审：F §6.2 全序列——Drain 清簿回流、T+1 日界解冻、公司域权益 AdminBatch 下发组落账入组端可逆日志、day_end/book_auction_boundary→coordinator 串行全局事务） | 4 |
| 6 | **跨股计划续行多轮协议** | adaptive_plan_chain/plan_chain_candidates/ready_stock_stream/plan_execution；轮数上界显式登记 | 5 |
| 7 | **compute.rs 全退 + rayon 删除** | 池面全删、lib 导出、构造面清理、engine Cargo 删 rayon、wasm shim 收敛、tests/compute 契约迁移 | 6 |
| 8 | **持久化/恢复** | persistence 三方收集与线程重建；saved_runtime 契约更新（可破坏） | 7 |
| 9 | **crate 外接缝** | server/web-wasm/web/根 Cargo/examples | 7 |
| 10 | **文档与合同** | 新 ADR、liveness-contract v5（不弱于现版逐前提映射）、trading-rules.md 登记、architecture/roadmap | 8,9 |
| 11 | **全量回归+性能验收+复核收口** | 全量验收（多核并行、300s deadline）、s5 正式基准（独占全机）、独立 subagent 全 diff 复核+修复循环 | 10 |

拆分逻辑：先降 rayon 面（批1）→ 新骨架并行就位（批2）→ 按域横切（执行层批3 → 决策层批4 →
所有权批5），每批保留一个"旧机制仍在跑"的中间态使全量测试可判定；机制退役（批7）与对外
面（批8-9）最后收口。批5 若超载可再拆 5a（账户+名册+准入实体化）/5b（结算+投影+views）。

## 3. 每批验收判据

**通用门禁（每批强制）**：
1. `cargo test -p engine`（分片并行执行）全绿；单 case ≤10s、长验收 ≤300s 纪律。
2. 测试台账：新增/改写/删除逐条登记，删除必须有机制退役证明或替代表——不静默删。
3. 独立 subagent 复核该批完整 diff（大 A 语义三问），有效发现修复后复检。
4. **性能守卫**：固定 s5-lite fixture（10组+5股、~200 tick、独占机器）短基准；相对前一批
   回归 >5% 必须先归因（性能闭环）再合入；正式基准只在批11（独占全机）。

**批特定等价判据**：
- 批1：首错按索引序注入测试（新 chunked helper 合同：每索引恰一次、按索引序首错、panic
  原 payload 重抛，对齐 compute.rs:795-799）；全部既有断言数值不变。
- 批2：协议单测——收束计数（G+N）、时序因果（flush 先于 DeskDone/收齐才广播，T+1 解冻
  安全性）、通道断连显式失败、panic 运输（Failed→resume_unwind 原 payload）、
  Drain→Shutdown→Final 对账（对齐 orun.rs:373-453）、**Abort 回滚等价**（Sol 终审：
  含多轮 Continue 后失败——第 k 轮注入失败 → Abort → 组账户/交易部簿/协调者全局域
  三方快照哈希与 tick 起始一致，foreign_active 复原，F §5.2）。
- 批3：撮合逐项等价——价格档序/同价分层政策（tick 内任意已定；组间 FIFO 组=1~N tick 默认，
  迁移后由消息序承载，对照 memory 政策核证）、T+1（civil-day）、费用分项、涨跌停带、整手、
  部分成交/撤单/到期、**价格笼子**（±2% vs 十个最小价位取宽、参考价解析序
  对手一档→本方一档→最新成交/前收、可配置关）、**单笔上限主板/创业板差异**、
  **零股一次性**（组端校验+交易部防御复核）、**开盘/收盘集合竞价相位门禁与事件**
  （09:20 后/14:57 后不可撤窗口显式拒收回执、一次撮合、clearing_price 事件、余单转
  连续）、**PreOpen 拒收回执**（Sol 终审补）；现金+股份守恒；spine_v0 契约、
  order_single_lifecycle；hash 契约（新增三方聚合口径）。
- 批4：决策等价——decide_data 纯函数同输入同输出抽检、attention/belief 输入密封等价、
  决策配额（npc_decisions_quota 等价迁移）。
- 批5：期末审计全绿（负现金=0、T+1 违规=0、整手违规=0、现金守恒含 fee_sink、股份守恒
  含 remainder）、严格所有权 skip 计数显式非零可解释、存档往返、**group_skipped 率测量**
  （退款时序差量化，带数据交用户，见 §4.4）；**DayEnd 三项**（Sol 终审）：①权益落账
  等价（AdminBatch 现金股利/送转/扣税逐账户对账）、②清簿回流（Drain 后活动委托=0、
  实体守恒、fee_sink 日结上交）、③日结存档往返（完整持久化重建留批8，此处验内容
  等价；含 AdminBatch 落账入组端可逆日志的 Abort 冲回路径，F §6.2）。
- 批6：续行收敛（计划链有限步终止、无组间同 tick 循环等待、轮数上界测试）。
- 批7：`rg 'rayon|par_iter' packages/engine` 零命中；tests/compute.rs 5 项契约迁移。
- 批8：save→restore 重建线程拓扑后继续 tick + 审计等价。
- 批9：server actor 回归 + web-wasm 构建（串行退化合同）。
- 批11：全量绿 + **s5 正式基准 ≥ 原型同负载口径（962 t/s 级）**；负偏差走归因闭环
  （方案 vs 实现），不许直接降级目标。

## 4. 风险清单

1. **scope lifetime 不安全点**（任务指定）：现引擎依赖 rayon scope 的跨臂借用——
   `pipeline/mod.rs` plan_tick 的 `in_place_scope`（&view 与 &mut writes 分裂借用）、
   stock_stream.rs:725 起 `scope.spawn` 借用每股输入、npc_split 的读写分裂纪律、
   compute.rs:1124-1147 `pub(crate) use rayon::Scope` 再导出。新模型跨线程数据必须
   owned 或 Arc：**规则=出现借用冲突时禁用 unsafe、禁无声大克隆**；所有权转移（move）或
   收束点快照二选一，克隆路径必须过基准。批3/批4 是该风险集中区。
2. **首错按索引序合同**（任务指定）：parallel_map_chunks 承诺被台账/错误展示链依赖
   （D §5.3）。批1 的替换 helper 必须保持；批4/5 后并行面变为 G 组/N 部——**失败全序
   必须显式定义**（建议 (kind, index) 字典序：组按 group_idx、部按 stock_idx），收束侧
   仲裁选首错，其余失败如实并列上报不丢弃；配注入测试。
3. **绑定传播断点**（任务指定）：phase_timing/executor_perturbation 的 thread-local 绑定
   经 compute.spawn/install 跨线程传播（compute.rs:642-658、:1006-1020；D §5.1）。删除后
   verification-harness 特性要么随 Start 消息携带证据上下文、要么在各线程入口 init——
   不能静默失效；phase_timing_tests(17)/executor_perturbation_tests(13) 是守卫。
4. **退款/到期回流可见性时序差**（所有权模型固有语义差，非线程问题）：现引擎 quote_expiry
   在 tick 起点相位内完成、退款**当 tick 决策可见**；实体模型中到期实体的回流在 tick N 收束
   flush、**tick N+1 决策才可见**（oworker.rs:16-19 因果序）。严格所有权下在途回款期间
   NPC 可用余额更低 → group_skipped。处置：批0 预登记 → 批5 实测 group_skipped 率 →
   带数据交用户拍板（含可选的"到期实体提前 flush"改型）；trading-rules.md 登记，不许静默。
5. **commit/hash 聚合**：business_state_hash/session_state_hash（hash.rs:30/:190）与
   FieldId 台账（session.rs:1452-1568）建立在单 candidate 全集上；状态分裂后需三方聚合
   （组+部+coordinator）且 `COMMIT_FIELD_EXPECTED` 两 cfg 变体同步，否则 hash 契约静默漂移。
6. **全局身份分配**：next_order_id/next_receipt_base/envelope 跨股 key 空间在 coordinator；
   desk 本地发号必须走**块预分配**（coordinator 发段），避免每单同步；存档需记录已发段。
7. **拍卖/日界/公司域全局事务**：book_auction_boundary/day_end/end_civil_day/公司域
   Arc::make_mut 是全局事务——coordinator 串行执行、组/部空闲等待，不并行化（B §5.3）；
   相位分派错误会表现为收束计数挂死 → recv 超时+abort 显式失败兜底（RECV_TIMEOUT 模式）。
8. **WASM**：std::thread 在 wasm32 不可用 → 串行退化（组/部退化为顺序循环），ADR 登记
   已知限制；web-wasm 的 wasm-bindgen-rayon 与 COOP-COEP 头退役前核对无其他依赖。
9. **活性不弱于现版**：新契约 = 任务独立（worker 唯一等待点是下行 recv）+ 有限完成 +
   等待目标广播时刻已无条件入队 + mpsc FIFO 因果（§0 时序不变量）。死锁面结构性消失，
   但**收束计数错配/通道泄漏/recv 死等**是新失败形态：批2 的超时+abort+断连显式失败测试
   是硬门禁；liveness v5 逐前提映射现版四前提并附核证表。
10. **中间态双轨成本**：批3-5 期间快照广播可能成为新串行脊（20,007 账户密封输入下行，
    B §5.2）——性能守卫逐批捕获；若某中间批大幅回归，按负结果归因流程判断是否重排批次
    （如提前批5），不硬扛。
11. **deferred_drop 跨线程**：保留（D §3），但旧 committed 态的析构移交现在来自组/部
    线程——上交路径与顺序需在批3/批5 各自验证无阻塞（任务#35 教训）。
12. **RECV_TIMEOUT 相位感知**（Sol 终审）：recv_timeout 看门狗（原型 5s，§0/F §4.2）
    不分相位——DayEnd 公司域 civil 事务与批8 持久化期间 worker 空闲等待，大公司域/
    大存档可能超过 5s → 看门狗误报显式失败。处置：长相位按相位放宽阈值或协调者心跳
    保活；超时上报必须携带相位上下文（不许静默放大全局阈值）。批5（DayEnd 接线）与
    批8（持久化）各配守卫测试。
13. **FillFact/DeskReceipt 上行负载不在 962 基准内**（Sol 终审）：原型上行仅 Entity
    聚合批（25 批/tick）；正式引擎 FillFact（含费用分项）/DeskReceipt 逐条上行、
    DeskDone 携带 receipts/facts/守恒行，上行面显著大于原型——962 t/s 只能作机制口径
    参考。批11 正式基准按真实负载重标；上行批组织/对象池复用（F §1.3）为第一候选
    优化位，先测量再动。

## 5. 测试迁移策略（实测 2558 项）

| 桶 | 规模（估） | 处置 |
|---|---|---|
| A 零接触 | ~1700（~65%） | company/accounting/strategy/plans/experience/information/orderbook/calendar/behavior/money/config/market/civil 及经 GameSession 公共 API 的集成测试——公共 API（step_frame/存档/查询）不变 |
| B 构造面机械适配 | ~300（~12%） | GameSession 构造签名（compute 参数删除）、shared_default 调用点（session.rs:1778/1820/3224、protocol/civil/session.rs:101/153/174）、examples |
| C 夹具级适配 | ~250（~10%） | pipeline 测试中仅依赖 candidate/TickShadowPlan 结构的——夹具换新结构，断言保留 |
| D 重写 | ~180（~7%） | rayon 簇 318 项 + 探针簇 74 项中的拓扑断言部分（约六成）、compute.rs 内嵌测试、tests/compute.rs(5)、tests/step_skeleton.rs(6)。重写目标=同一语义在新拓扑下的等价断言（如 npc_decisions_quota→组线程配额、authoritative_tick→coordinator 收束安装、ledger_conservation→三方聚合守恒、saved_runtime(42)→恢复重建线程） |
| E 删除（强制登记） | ~70（~3%） | 池准入/宽度封顶/协作让位/workers()==1/ComputeWork 观测/T1Probes 断言（round3-5 台账红测试中机制绑定部分）+ server actor.rs:182-191 观察断言——逐条列"机制已退役，替代表=X 或无对象" |
| F 新增 | ~80-120 | 协议往返/收束计数/因果不变量/panic 运输/关停对账/严格所有权守恒/多轮续行/身份块分配/存档重建 |

执行纪律：每批的测试改动随批走（批N 代码+测试同 commit）；E 桶删除只在对应机制退役批
（主要批4/5/7）进行；A/B/C 桶在任何批不得顺手改动；批0 产出"测试→批次"分配表并随批勾销。

## 6. 与设计产物 A-D 的差异说明

- A 推荐 (a-strict)（按股名册、三处行为简化）**被用户 2026-10-15 决策覆盖**：走所有权
  转移模型（=A 的 (c) 血统 + 原型实测 962 t/s），**跨股语义完整保留**——retail 全市场选股、
  inst/hot 全市场扫描、watchlist/注意力发现机制全部保留（组线程持全市场快照广播）。
  A §2 的三处行为简化**不发生**；唯一语义差是 §4.4 的回流时序位移。
- C 的协调者协议（TickStart/TickContinue 经 coordinator 中转）在所有权臂下修正为**数据面
  直发**（组→部不过 coordinator，orun.rs:9）；C 的多轮续行设计保留用于批6 计划链。
- B 的分片结论（§2.3 混合方案）与所有权模型一致，仅"账户留 coordinator"修正为"账户
  所有权进组线程"；B 的 epoch/Abort 机制保留用于批3 股域、扩展到批5 组域。
- D 的退出面清单全量吸收进 §1.4/批1/批7。
