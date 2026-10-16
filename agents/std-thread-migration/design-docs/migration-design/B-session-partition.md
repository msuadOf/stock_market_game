# 任务B：Session 状态分片设计（std-thread 迁移）

> 输入：spine-simplify-01 增量冻结链终态（`/data1/baiyifan/workplace/stock_market_game/.worktree/spine-simplify-01/packages/engine/src/`，下称 engine）
> 原型：`.worktree/std-thread-model/packages/std-thread-proto/src/`（下称 proto）
> 用户决策（2026-10-14）：一股一持久主线程 + 内联 NPC 决策；代码本体无线程数管理概念，CPU 限制走 OS 级（cgroup/taskset）。

---

## 1. 现 GameSession 的字段分组

权威状态唯一载体 = `CommittableSessionState`（engine/session.rs:1379-1438），由 `GameSession`（engine/session.rs:1362-1376）持有。按所有权维度分组：

### 1.1 按股 keyed（天然可分片）

| 字段 | 定义处 | 内容 |
|---|---|---|
| `markets: BTreeMap<StockId, Market>` | session.rs:1389 | 每股订单簿 + last_price + 涨跌停带（market.rs:50-66） |
| `price_history: BTreeMap<StockId, VecDeque<Money>>` | session.rs:1392 | 逐股 tick 价历史 |
| `market_minute_closes: BTreeMap<StockId, Vec<MarketMinuteClose>>` | session.rs:1393 | 逐股分钟收盘 |
| `candle_book: SessionCandleBook` | session.rs:1394 | 内部即 `BTreeMap<StockId, _>`×2（candles.rs:92-95） |

关键事实：**tick 内的逐股私有状态已经存在**——`IncrementalContinuousStockShadow` 每股一个实例，拥有本股 `Market`、`PrivateStockLedger`（逐股 envelope/守恒行，private_stock_ledger.rs:7-22）、`market_prior`（行情先验）（incremental_continuous_stock_shadow.rs:128-143）。今天它们每 tick 从 candidate 克隆构建、收尾消费，这正是可以"常驻线程化"的现成切面。

### 1.2 按账户 keyed（全局，与股正交）

| 字段 | 定义处 | 备注 |
|---|---|---|
| `accounts: AccountBook` | session.rs:1390 | COW 分页（account_book.rs:22-49），跨股持仓 + 现金 + 策略 |
| `history_reads` / `npc_attention` / `retail_experience` / `belief_participants` | session.rs:1391/1397/1398/1425 | `AccountPagedMap`（account_paged_map.rs:15-18） |
| `npc_order_lifecycles` | session.rs:1399 | 账户 keyed 生命周期 Vec |
| `plans: PlanBook` + `urgency_policy` | session.rs:1422-1423 | 跨日个人计划（全账户） |
| `attention_scheduler` | session.rs:1408 | `BinaryHeap<(tick, AccountId)>`（attention.rs:265-270） |
| `last_retail_decisions` / `last_retail_order_events` | session.rs:1402-1403 | 逐账户诊断缓存（不入存档） |

**为什么账户不能按股切**（与 proto 的本质差异）：
- retail 每次决策从全股 `MarketView` 随机选一股（strategy/retail.rs:18-27）；
- inst/hot 遍历全部股票（strategy/institution.rs:18、strategy/hot.rs:13）；
- `SelfView` 明确跨股（strategy/mod.rs:107-109）；初始分配把每只股票的流通盘分给全部三类 NPC 账户（initial_allocation.rs:52-89）；
- 撮合本身**不直接写账户**——continuous_matching 产 envelope/receipt/fact，账户回写集中在 Settlement 相位（settlement.rs:146-148，已按账户 `into_par_iter`）。

### 1.3 全局只读 / 近不可变

`setup` / `validated_config` / `stocks`（StockCatalog 构造后不可变，session.rs:1380-1386）；公司域 `company_registry` / `operations` / `closing` / `library`（Arc，日结时 `Arc::make_mut`，session.rs:1411-1418）；`groups`、`disclosures`、`seed`。这些与 proto 的 `Arc<MarketSnapshot>` 下行同形（proto/worker.rs:59-78）。

### 1.4 全局共享可变（串行脊必须独占）

- 单调计数与身份：`next_receipt_base`、`next_order_id`、`tick`、`day`、`seq`（session.rs:1430-1435）；
- `rng`（会话随机流，seal 采样消费，session.rs:1387）；
- `envelope_ledger`（在簿回执账本，跨股 key 空间，session.rs:1427）、`retail_projection_seen`（session.rs:1429）；
- `civil_clock`（自然日双时钟，session.rs:1437）；
- 玩家通道：`pending_player`、`ingress_receipt_cursors`（session.rs:1395-1396）+ `GameSession::ingress`（外部宿主共享注入，session.rs:1364）；
- `GameSession` 顶层：`compute: Arc<ComputeContext>`（session.rs:1363）、`revision` 原子直改计数（session.rs:1368）、`poison`（session.rs:1369）。

---

## 2. 分片方案对比

### 2.1 (a) PerStockState 拥有本股全部状态 + 全局只读快照

即 proto 原样放大。**判定：不可直接用**。除 1.2 节的跨股账户障碍外：
- 玩家账户、计划链、公司域、日历、回执身份域全部留在协调者，"本股全部状态"不成立；
- 若按 proto §4 把 NPC 名册按股独占（design.md 已登记的退化），retail 选股分布、inst 跨股配置语义漂移——不是大 A 规则违背，但属于模拟形状变化，必须单独登记并经用户拍板，不应耦合在线程迁移里。
- 唯一无损可行的"按股独占"恰好是 1.1 的四域 + 逐股账本；其余仍需消息边界。

### 2.2 (b) 纯消息传递（各股线程发 patch 回协调者）

形态=现状 tick 内结构的线程化：协调者保留全部权威，股线程只算不持有。缺点：book 每 tick 在线程与权威间转移（转移即 clone 或所有权往返），丢失"持久线程私有无锁变更"的用户模型核心收益；且决策不下沉的话，最大计算量（NPC decide）仍在协调者侧，982 t/s 的内联收益拿不到。

### 2.3 (c) 混合（推荐）：按域分所有权 + 固定消息边界

```
协调者（tick 脊，单线程权威）
  拥有：1.2 账户域 + 1.3 只读域 + 1.4 计数/时钟/玩家通道
  每 tick：
    1) 组装 Arc<TickStart>（全局只读快照：全股 MarketView + 密封后的逐账户
       DecisionAccountInput + PhasePolicy + 玩家意图冻结）
    2) 广播给 N 个股线程
股线程 i（持久，一股一个，跨 tick 存活）
  拥有：Market_i（book）、price_history_i、minute_closes_i、candle_i、
        PrivateStockLedger_i（即把 IncrementalContinuousStockShadow 常驻化）
  每 tick：内联本股 NPC 决策计算（读共享快照，纯函数 decide_data——
        compute.rs:1237-1244 已约定"按输入顺序稳定输出、不按账户偷信息"）
        → 本股撮合（含 adaptive continuation 轮，本股候选对本股 book 的
          后续轮天然串行在本线程）
  上行：TickDone{ MarketDelta_i, facts, receipts, 行情 patch, 个人域 patch }
收束安装（协调者，单点）
  全部 TickDone 到齐 → 结算（按账户并行，读全局回执）→ 校验 →
  一次安装账户域/计数器/时钟 + 确认各股 epoch 转正
```

要点：
1. **决策内联不需要账户写权限**：现状决策已密封为只读 `DecisionSnapshot`（decision_snapshot.rs:13-38），产出是 patch/intent 消息（npc_tick_preparation.rs 交接件 `NpcDecisionHandover`，pipeline/mod.rs:242-245）。NPC 名册分给股线程只是**计算亲和**，不改语义——decide_data 是纯函数，输入（MarketView/SelfView/RNG `seed^tick^account`）与输出按账户身份对齐，在哪个线程算结果等价。这绕开了 2.1 的语义漂移。
2. **撮合无需碰账户**（1.2 末条），股域私有化零结算代价；Settlement 留在收束点按账户并行（现状 settlement.rs:146 形态保留，只换执行底座）。
3. **线程拓扑固定**：股线程数=股票数；CPU 竞争交 OS（taskset/cgroup），代码里没有 worker 数、准入台账（compute.rs:24-49 的 PoolAdmission/active_width 全删）、宽度收缩——用户明示"线程不进行管理"。跨股依赖被两相位边界消除：决策读的是**上一收束点**的快照，同 tick 撮合结果不回灌本 tick 决策（与现状 pop 视界=已提交 tick 的语义一致，pipeline/mod.rs:397-400）。
4. 活性契约四前提（stock_stream.rs:7-26）在新拓扑下的对应：任务独立=是（股间无等待）；有限完成=是；协调者阻塞等待=真 `mpsc::recv`（协调者不占 worker 槽，不存在池内自锁）；等待目标无条件入队=广播即全部派发。契约实质保持，措辞需按 std-thread 术语重写（任务背景已要求"不依赖 rayon 术语"）。

**结论：推荐 (c)。** 它是 proto 形状（持久 worker + 快照下行 + TickDone 上行 + 单点收束，proto/run.rs:171-206、proto/worker.rs:203-239）在真实引擎相位结构上的无损放大。

---

## 3. commit/install 路径重构

### 3.1 现状机制

- 每 tick `clone_for_tick_shadow`（session.rs:1742-1762）整体克隆 candidate（COW 分页共享未变页），所有相位在 candidate 上推进；
- 失败原子性 = **消费式丢弃 candidate**：`TickShadowPlan::take_session`（pipeline/mod.rs:296-311，注释明言 "A failure drops it; restoring a partially changed candidate would make a failed plan committable"），权威从未被失败路径触碰；
- 成功路径 `commit_tick_shadow` → `commit_from`（session.rs:4580-4674）：32 个字段逐 `defer_swap`（FieldId 台账 session.rs:1452-1568，两个 cfg 变体 30/32 项全集），旧值移交专用析构线程（deferred_drop.rs:5-7，池外 std::thread，与 rayon 无关，**保留**）；
- 自然日日结同机制（`with_prepared_civil_day`，session.rs:2619-2647）。

### 3.2 分线程后的失败原子性：两域两策

**股域（book/行情/逐股账本）——working/committed 双份 + epoch 转正**：
- 股线程常驻持有 `committed`（上一转正态）；tick 开始从 committed 构建 `working`（这条增量构建路径已存在：`continuous_shards`/`from_post_expiry` 初始化 IncrementalContinuousStockShadow，read_only_start.rs:36-38 的 stock_shards 即其产物）；
- tick 全链成功：协调者广播 `Commit(epoch)`，股线程 `committed = working`（swap，旧 committed 走 deferred_drop）；
- 任一处失败：广播 `Abort(epoch)`，各股丢弃 working——**与现状"丢弃 candidate"逐语义等价**，只是丢弃动作从单线程变成广播；
- 权威 `markets` 等四域从协调者 state 中移除，换成 `BTreeMap<StockId, StockLease>`（线程句柄 + epoch + 只读投影缓存）。`FieldId::Markets/CandleBook/PriceHistory/MarketMinuteCloses` 四项从 commit 全集退役，替换为逐股 epoch 转正的合并计量；`COMMIT_FIELD_EXPECTED` 两个 cfg 变体与注释 ①②③ 同步清单（session.rs:1450-1451）照做。

**全局域（账户/计数器/时钟/公司域）——维持单线程 candidate**：
- 收束安装仍在协调者单点：收齐全部 TickDone 后，协调者在其私有 candidate（或直接权威，因股域已隔离）上做 Settlement→校验→事件→安装。失败时协调者丢弃自己的候选 + 广播 Abort；成功则安装 + 广播 Commit。安装点唯一性不变（authoritative_tick.rs:1 "单批 EventFact 只在全部 callback 验证后安装" 的门禁语义保留）。
- 存档/查询（persistence.rs 2318 行、session.rs 只读 API）：需要股域一致性视图时走**收束点同步快照**——存档本来就发生在 tick 边界（宿主不并发 step），协调者广播 `SnapshotRequest(epoch)` 收集各股只读投影即可；非收束点的 `&self` 查询只暴露上一转正态（MarketLease 内缓存），与"step 后可见"的既有契约一致。

**poison/panic 传播**：现状 `poison: Option<StepFatal>`（session.rs:1369）+ `require_healthy`。分线程后股线程 panic 必须显式上抛不静默（铁律二）：proto 已给出范式——worker 主循环 Err 显式 break 上报（proto/worker.rs:226-238）、join 时 `resume`（proto/run.rs:224-229）、常驻池 Drop `panic_any`（proto/worker.rs:137-147）。协调者收到任一股 Err → poison 会话 + 广播 Shutdown + join 全部句柄后再返回错误。

**X1 新鲜度（revision 原子）**：直改计数（session.rs:1365-1368）留在协调者；股线程只读快照携带 revision，收束安装时校验未变（现状语义平移）。

### 3.3 存档恢复

`SavedRuntimeState` 恢复（session.rs:468 起、persistence.rs）需要新增一步：重建 GameSession 后按股重建常驻线程（每线程从恢复的 per-stock 状态初始化 committed）。worker 生命周期 = 会话生命周期（Drop 时 Shutdown+join，proto worker_main 收尾上交 FinalState 的形状 proto/worker.rs:240-264 可复用为期末审计）。

---

## 4. 文件面预估（engine src 共 431 个 .rs）

引用 `ComputeContext` 的非测试文件 = 21 个（grep 计数）。分级：

**重写/新建（6-8 个）**
| 文件 | 动作 | 规模 |
|---|---|---|
| compute.rs（1316 行） | 删除池/准入/宽度/scatter-gather/ComputeBackend trait；保留验证 observer、phase timing 绑定所需最小面 | −1200 行级 |
| session/stock_worker.rs | 新建：常驻股线程 + 消息循环 + working/committed + epoch（proto/worker.rs 放大） | ~600-900 行 |
| session/tick_coordinator.rs | 新建：tick 脊/快照组装/广播/收束安装/Abort 广播（proto/run.rs 放大） | ~400-600 行 |
| session.rs（5050 行） | CommittableSessionState 重组（markets→lease）、clone_for_tick_shadow/commit_from/FieldId/COMMIT_FIELD_EXPECTED 重写 | 核心改动 ~300 行 |
| session/pipeline/stock_stream.rs（2401 行） | 池内协调层（准入/容量/让位）退役；typed feedback 归并并入 coordinator | 删 ~2/3 |
| session/pipeline/market_tick_transaction.rs（589 行） | 相位编排改为 coordinator 调用股线程 | 大改 |

**中改（8-10 个）**：pipeline/mod.rs（plan_tick 尾段）、npc_tick_preparation.rs（1845 行，决策分发改内联派工）、npc_split.rs（SessionReadView 拆分重定义）、incremental_continuous_stock_shadow.rs + continuous_matching*.rs（shadow 常驻化接口）、settlement.rs / retail_projection.rs / npc_state_projection.rs / decision_chain.rs（并行原语替换）、persistence.rs（存档收集/恢复重建）、verify_metrics.rs（730 行，字段全集同步）、shared_ingress.rs（570 行，快照携带玩家冻结）。

**签名级（12-18 个）**：account_book.rs / account_paged_map.rs / decision_resources.rs / decision_snapshot_capture.rs / ready_ingress.rs / candidate_commit.rs / read_only_start.rs / notices.rs / self_views.rs / views.rs / plan_chain_candidates.rs / verification_evidence/phase_timing.rs 等 21 个 ComputeContext 引用文件中其余者。

**合计：核心改动约 25-35 个非测试文件；测试牵连 pipeline 下 40+ 个 `*_tests.rs`（多数仅夹具/探针适配，spine_v0_contract / t4_market_data_owner / order_single_lifecycle 等契约测试需按新拓扑重述）。**

## 5. 风险登记

1. **同 tick 跨股依赖**：adaptive plan chain 的 continuation 若引入"股 B 结果影响股 A 同 tick 候选"，违反任务独立前提——现状把这类依赖收敛在账户校验收束点（stock_stream 的"已完成股票推进依赖计划"），迁移时必须保持该边界，不得让股线程互相等待。
2. **快照成本**：每 tick 全股 MarketView + 逐账户密封输入的下行体积（20,007 NPC 的 DecisionAccountInput）。proto 用 `Arc` 共享 + index 对齐（proto/worker.rs:60-65）；决策输入密封本身已在现状热路径（queue 链），需基准确认广播不成为新串行脊。
3. **拍卖/日界相位**：集合竞价、收盘竞价、日结不走连续相位（phase.rs PhasePolicy），股线程消息循环需按相位分派；日结（end_civil_day）是全局事务，建议直接在协调者单线程执行（股线程空闲等待），不并行化。
4. **WASM**：`cfg(target_family = "wasm")` 路径（compute.rs:468-486、deferred_drop.rs:14-16）退化为单线程顺序执行——股线程模型整体 no-op 化（一股一"伪线程"串行循环），保留现有 WASM 顺序合同。
