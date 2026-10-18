# 任务C：协调者协议设计（rayon 池 → 纯 std::thread，一股一持久线程）

> 设计分析产物（2026-10-14）。只读分析，未改任何 worktree 源码。
> 源码依据前缀约定：
> - `engine/` = `.worktree/spine-simplify-01/packages/engine/src/`（106 增量冻结链终态，迁移对象）
> - `proto/` = `.worktree/std-thread-model/packages/std-thread-proto/src/`（std::thread 原型，982 t/s）
> 行号为当前工作区快照行号。

---

## 0. 结论摘要

1. 现有引擎执行期（E 阶段）**已经**是"每股独立任务 + 单协调者收束"的形态（T5 改造后执行期 root-feed 唤醒通道已删除，`engine/session/pipeline/stock_stream.rs:50-52`），只是任务被撒进共享 rayon 池、簿的所有权每 tick 在协调者与池之间往返。std::thread 迁移的本质 = **把"池 + 台账 + 往返所有权"替换为"每股一个持久线程 + 每股一条下行 channel + 一条上行收束 channel"**，协调循环从事件驱动简化为计数收束。
2. 协议推荐：**全 mpsc、无 Barrier、无 Condvar**。下行 = 每股独立 `channel<TickMsg>`（`proto/run.rs:117-118` 每股建 channel、`proto/run.rs:177-180` 逐个 send）；上行 = 单一 `channel<StockResult>`、coordinator 固定 N 次 `recv()` 收束（`proto/run.rs:182-204`）。
3. 同 tick 计划续行（跨股 ops）必须保留：协议支持 **tick 内多轮**（TickStart → 收束 → 若有外股续行 ops → TickContinue → 收束 → … → TickEnd 安装），把现有 `drive_stock_stream` 的 `on_progress` 路由循环去线程化为按轮批同步。
4. panic 协议：worker 单 tick 执行体用 `catch_unwind` 包住、panic payload 经 StockResult::TaskDied 回传（对齐现有 NPC 流的 `DecideMessage::TaskDied` 运输模式，`engine/session/pipeline/npc_decisions.rs:455-460` 与 `engine/compute.rs:883/958`），coordinator 收到后全局关停、join 全部线程、`resume_unwind` 原 payload。一股 panic 不死锁、不静默、不冒充可恢复 tick。
5. 活性保证（契约四前提）在 std::thread 模型下的实质：**每股 worker 的单 tick 执行只依赖广播输入 + 本股私有状态，不等待任何其他线程的信号**（任务独立 + 有限完成）；coordinator 的等待目标（N 个在飞 worker）在广播时刻已全部无条件"入队"。旧契约中为池饱和设计的台账/容量等待/协作让位整套机制随之消失，活性论证反而更短。
6. 文件面：新增 3 个协议文件（约 700-900 行含测试），删除 `compute.rs`（1316 行，WASM shim 可按需保留）与 `stock_stream.rs` 协调层（约 600 行）及配套台账测试簇，改造 `market_tick_transaction.rs` / `npc_decisions.rs` 等 6-8 个文件。

---

## 1. 现有 tick 编排（五步脊）及其 std::thread 映射

### 1.1 现有编排结构（源码依据）

宿主入口 `step_frame`（`engine/session/protocol/commit.rs:62`）→ 唯一生产 dispatcher
`execute_authoritative_tick_with_projection`（`engine/session/pipeline/authoritative_tick.rs:26-33`）→
`prepare_market_tick_with_evidence`（`engine/session/pipeline/market_tick_transaction.rs:106-128`）。每 tick 的串行脊五步：

| # | 脊步 | 主要源码 | rayon 用法 |
|---|------|---------|-----------|
| ① | **准备**：捕获可丢弃 candidate（clone + freeze ingress）、npc_split 读写分裂、决策生产与只读快速路径准备**并行重叠**、expiry / 资源密封 | `engine/session/pipeline/mod.rs:385`（`plan_tick`）、`:431`（`npc_split`）、`:436-470`（`in_place_scope` 内一臂 spawn 只读准备、协调线程跑决策生产） | `in_place_scope` + `scope.spawn`（1 个辅助任务） |
| ② | **D 决策生产**：普通 NPC 按账户并行完整计算（attention→观察→分析→decide→个人投影），产物 `NpcDecisionHandover` 移交；计划 root 在 post-expiry 观察基上**串行同步闭环**（`capture_roots`→`drain_ready_accounts_at_capture`） | `engine/session/pipeline/mod.rs:443`（`produce_decision_output_at_entry`）；`engine/session/pipeline/market_tick_transaction.rs:366`（`capture_roots` 串行）；NPC 账户任务流 `engine/session/pipeline/npc_decisions.rs:387`（`stream_npc_account_tasks_with`，chunk 化派发 + TaskDied 运输） | 池内 chunk 任务 + 台账预留 |
| ③ | **E 执行**：账户授权（AccountValidatorDriver）与股票 shard 构造并行；`drive_stock_stream_with_feedback` 派发每股撮合任务、收完成、`on_progress` 路由续行 | `engine/session/pipeline/market_tick_transaction.rs:392-409`（`compute.join` 两臂）、`:423-432`（drive）；协调循环 `engine/session/pipeline/stock_stream.rs:705-783`（`drive`） | `compute.join` + `in_place_scope` + `scope.spawn` + 台账 `reserve_spawn_slot` |
| ④ | **收尾 + publication 一次汇合**：计划链排空、账户校验收口、股票 owner 行情收尾（三处收敛为 `finish_market_execution` 单入口）、收据聚合→按账户结算→frame 构造 | `engine/session/pipeline/market_tick_transaction.rs:236-309`（`finish_market_execution`）、`:460`（`finalize_continuous_tick`） | `compute.parallel`（validator.finish / finish_market_shards） |
| ⑤ | **commit 单点安装**：PreCommitValidation 哈希核查后 `CommitTick` 唯一安装点整体生效；失败丢弃整个 candidate | `engine/session/pipeline/market_tick_transaction.rs:117-122`；`engine/docs/architecture.md:128-141`（ADR-0042 四阶段模型） | 无（纯串行） |

外围设施（迁移后整体退役）：
- `ComputeContext`（`engine/compute.rs:21-72`）：共享 rayon 池 + 池准入台账 `drives+spawned≤workers` + `capacity_changed: Condvar` + `active_width` + 测试探针组（T1Probes 等）。
- 活性契约四前提（`engine/session/pipeline/stock_stream.rs:3-33` 模块头；全文 `.tmp/spine/round5-fix/liveness-contract.md`）：任务独立、有限完成、回调协作让位、等待目标已无条件入队。

### 1.2 逐步映射到 std::thread 协议

| 脊步 | std::thread 映射 | 说明 |
|------|-----------------|------|
| ① 准备 | **coordinator 线程串行执行**（candidate 捕获、expiry、密封）；"只读快速路径并行重叠"退役——其收益（读准备与决策生产重叠）在每股 worker 自持输入后自然消失 | 决策生产迁入各股 worker（见任务B边界），coordinator 只构造广播输入 |
| ② D 决策生产 | **广播**：coordinator 把每股输入（含该股相关账户的决策密封输入）发给对应 worker；各 worker **内联**跑本股 NPC 决策（内联方案=无子线程，对齐 `proto/worker.rs:290-295` width=0 内联臂） | 账户级决策产出跨股 intents 的问题见 §2.4 |
| ③ E 执行 | worker 在同一线程**连续执行**决策→撮合→本股收尾（`proto/worker.rs:268-399` `run_tick` 四相位：决策→到期→意图应用→T+1），簿不再往返 coordinator；`StockStreamCoordinator` 的 pending/in_flight/notification 三层结构整体消失 | 跨股续行 ops 走 §2.3 多轮协议 |
| ④ 收尾+publication | **收束安装点**：coordinator 固定 N 次 `recv()` 收齐 StockResult 后单线程执行（对齐 `proto/run.rs:182-204` 收束更新公共历史；引擎版换成收据聚合→结算→frame） | 各股的"owner 行情收尾"已前移到 worker 内（typed facts 随 StockResult 直载，对齐 T4 owner 迁移方向 `engine/session/pipeline/stock_stream.rs:263-266`） |
| ⑤ commit | coordinator 线程串行，不变 | 单点安装语义零改动 |

### 1.3 关键结构性发现：执行期已无跨股依赖

spine-simplify-01 的 T5 改造已把计划 root 的生产与闭环全部移到 **capture 期（tick 起始、串行）**：`engine/session/pipeline/stock_stream.rs:50-52` 注释明确"执行期旧 root-feed 唤醒通道已删除——root 在 capture 期闭环"；`engine/session/pipeline/market_tick_transaction.rs:362-366` 在输入准备前串行执行 `capture_roots` 完整排空。执行期 `on_progress` 的 `StockCompleted`/`PlanRootReady` 路径是**纯路由**（活性契约生产核证表第 4 行，`.tmp/spine/round5-fix/liveness-contract.md`）。

含义：**"每股任务独立、有限完成、不等待其他股票"在现引擎中已是事实**（活性契约第 1/2 条对生产路径的核证结论）。一股一持久线程模型因此天然成立，唯一剩余的跨股信息流是：

1. 广播输入里的**跨股公共视图**（市场快照：所有股票的上一 tick 价/量，`proto/worker.rs:59-78` `MarketSnapshot`/`StockPublicState`，Arc 共享只读广播）。
2. 执行期**计划续行**可能产出针对其他股票的 ops（账户计划链跨股）——这是唯一需要 mid-tick 协调的信息流，由 §2.3 多轮协议承载。

### 1.4 与原型的差异清单（协议必须补齐的语义）

原型 `proto/run.rs` 是简化负载，迁移版 coordinator 协议相对原型需补：

| 原型简化 | 引擎语义要求（迁移版协议） |
|---------|--------------------------|
| StockResult 只有价格/量统计（`proto/worker.rs:87-98` `TickDone`） | StockResult 须承载本股 typed facts / receipts / auction finalizers / 行情收尾产物（对齐 `IncrementalContinuousStockFinish` 各字段，`engine/session/pipeline/continuous_matching.rs`） |
| 收束=更新公共历史（`proto/run.rs:186-204`） | 收束=收据聚合→账户结算→publication→PreCommitValidation→CommitTick；**tick 失败丢弃整个 candidate**（Escrow 语义，`engine/docs/architecture.md:148-151`） |
| 单轮 lockstep（send all → recv all） | 同 tick 多轮（计划续行，§2.3） |
| 3000 tick 不跨日、无交易阶段切换 | 协议消息须区分 Continuous / 集合竞价 / PreOpen 日的阶段边界（worker 内相位逻辑按广播的阶段元数据走） |
| 无 panic 运输（worker err 直接 break，`proto/worker.rs:234-238`） | TaskDied 运输 + 全局关停 + resume_unwind（§4） |

---

## 2. 协议设计

### 2.1 消息类型（`protocol.rs`）

```rust
// coordinator → worker（每股一条 channel）
pub enum TickMsg {
    /// tick 开始/新续行轮：Arc 共享公共快照 + 本股私有输入/ops + 轮次元数据
    Start(Arc<TickRound>),
    /// 本 tick 结束，worker 交回期末状态后退出（或转入空闲等待下一 tick）
    Shutdown,
}

// worker → coordinator（单一收束 channel，N 个 sender）
pub enum StockReport {
    /// 本轮完成：本股结果（facts/receipts/行情）+ 产生的跨股续行 ops（若有）
    Round(StockRoundResult),
    /// worker 内 panic：原 payload 运输（§4）
    TaskDied(StockId, Box<dyn Any + Send>),
    /// worker 退出时上交的期末状态（审计/持久化用，对齐 proto FinalState）
    Final(StockFinalState),
}
```

`TickRound` 内容：tick 号、交易阶段、`Arc` 公共市场快照（全股上一 tick 收束结果，对齐 `proto/worker.rs:60-65`）、本股 ops（受理过的 ValidatedOperation + expected outcomes，对齐 T4 成对派发 `engine/session/pipeline/stock_stream.rs:334-340`）、以及本股决策密封输入（任务B 边界）。

### 2.2 tick 正常时序

```
coordinator（单线程，持有权威 GameSession）
  ① 串行准备：capture candidate → expiry → 密封 → 按股拆分输入
  ② 广播：for tx in &tick_txs { tx.send(TickMsg::Start(round))? }   // N 次 send
  ③ 收束：for _ in 0..N { match done_rx.recv()? { Round(r) => 安装槽位,
       TaskDied(..) => 走 §4 关停路径 } }
  ④ 若所有 StockRoundResult.foreign_ops 为空 → ⑤；否则组装 TickContinue
     （按股分组的续行 ops）→ 再次广播 → 收束（§2.3）
  ⑤ publication（收据聚合/结算/frame）→ PreCommitValidation → CommitTick 安装
  → 下一 tick 回 ①
```

- 广播输入构造在 coordinator 线程，是把现有 `prepare_incremental_continuous_inputs`（`engine/session/pipeline/continuous_matching_adapter.rs`）的产物按 `StockId` 拆分 send；BTreeMap 拆分 O(N) 无竞争。
- 各 worker 收到 Start 后**连续执行**：内联 NPC 决策 → 本股受理复核（T4 的 typed feedback 匹配留在本股完成点，`engine/session/pipeline/market_tick_transaction.rs:316-344` `match_continuous_feedback` 迁入 worker）→ 撮合 → 本股行情/收据收尾 → `Round(StockRoundResult)` 回传 → 阻塞 `recv` 下一轮。
- worker 的等待只有一处：下行 channel `recv()`。**无任何 worker→worker 信号**（活性契约第 1 条在新模型下的直接满足）。

### 2.3 同 tick 多轮（计划续行，跨股 ops）

- worker 本地先消化**本股**续行（结果触发同股计划链 → 本股新 ops → 本地再来一轮，直到本股无新 ops）；**外股** ops 打包进 `StockRoundResult.foreign_ops` 上报。
- coordinator 收束后若有外股 ops：按股分组、追加到对应股的下一轮 `TickRound`，发 `TickContinue`（复用 `TickMsg::Start`，轮次号+1）。仅向涉及的股票发送。
- 终止条件：一轮收束后全部 `foreign_ops` 为空（现有 drive 循环的 `FeedOutcome::Complete` 对应物，`engine/session/pipeline/stock_stream.rs:691-703`）。有限轮次由计划链有限步保证（活性契约第 2 条同前提）。
- 轮次内每股至多一个在飞任务、且在广播时刻已无条件发出——等待目标无条件入队（契约第 4 条）自动成立；不存在旧模型的"池饱和导致目标入不了队"形状，故无需容量等待。

### 2.4 跨股账户决策的归属

账户级决策（一个 NPC 账户可产多股 intents）在"一股一 worker"下的拆分：

- **方案（推荐，随任务B 内联方案配套）**：决策按账户在** coordinator 侧或按账户主导股分组**——用户已拍板"内联 NPC 决策"：决策在 worker 内联执行。推荐按"账户→其当 tick 涉及的股票集合"把账户决策任务广播给**每股都算一遍自己相关的账户**是浪费；正确做法是把账户决策输入随其涉及的**主股票**派发，产出的跨股 intents 走与 §2.3 相同的外股 ops 通道（第一轮即完成决策期→执行期的过渡）。这样决策与执行共用一条"外股 ops 上报→定向续轮"机制，协议不增加第二种跨股通道。
- 该拆分不改变 A 股语义（决策仍是账户原子：一个账户的全部 intents 在同一轮内产出后整批派发）；顺序语义按"无确定性要求"的用户决策放松（memory: 并发下不要求可复现）。

### 2.5 关停协议（对齐 `proto/run.rs:211-229`）

1. coordinator 发 `TickMsg::Shutdown` 给全部 worker；
2. 各 worker 走出 recv 循环，交回 `Final(StockFinalState)`（期末审计/持久化输入）后线程退出；
3. coordinator `for _ in 0..N { final_rx.recv() }` 收齐 → 逐个 `handle.join()`；join 返回 Err = worker 以未运输的 panic 退出 → 显式上抛（`proto/run.rs:224-229` 把 join Err 映射为显式错误；引擎版应转 `StepFatal::InvariantViolation` 带"worker panic 未运输"位置信息，不允许静默）；
4. Drop 全部 sender 使任何滞留 recv 以 Disconnected 显式失败（防御式：断连必须报错，对齐 `engine/session/pipeline/stock_stream.rs:665-669` 的既有纪律）。

### 2.6 线程生命周期与状态所有权的澄清

- **持久的是线程，不是权威状态所有权**：v1 迁移保持 Escrow 语义——每股簿/shard 仍每 tick 从权威 candidate 输入构造（现 `continuous_shards` 从 stock_inputs 构造，`engine/session/pipeline/stock_stream.rs:125`），worker 线程只是执行载体持久化；tick 失败时整 tick 丢弃、下一 tick 从已提交权威态重建输入。资源实体模型（NPC→交易部→NPC 所有权流动）是另一条线，不在本协议内耦合。
- 现有 `precomputed_stock_shards` 快速路径（`engine/session/pipeline/mod.rs:255-265`、`read_only_start`）在迁移后自然退役：worker 常驻后"提前构造 shard"= worker 在上一 tick 收束后即可预备，收益由持久线程天然获得，无需独立机制。

---

## 3. 同步原语选择：mpsc vs Barrier vs Condvar

**结论：全程 std::sync::mpsc；不引入 Barrier、不引入 Condvar。**

| 原语 | 评估 | 判定 |
|------|------|------|
| **mpsc 下行（每股一条 channel）** | coordinator 顺序 send N 次，各 worker 独立 futex 唤醒，无共享锁、无惊群；唤醒延迟 ~1-2 μs/股。5-100 股时 <0.2 ms/tick，远小于每股毫秒级工作量。原型实证 `proto/run.rs:117-118,177-180` | ✅ 采用 |
| **mpsc 上行（单一收束 channel）** | 主从收束语义（等 N 个结果再统一推进）与固定计数 recv 完全同构（`proto/run.rs:182-185`）；各 worker 完成即回，天然支持**早到早收**（收束循环可先处理先到结果做轻量预处理） | ✅ 采用 |
| **Barrier(N+1)** | 面向对等全员同步，需两阶段 generation 翻转；会让"最快 worker 等 最慢 worker"的语义显式化但其实 mpsc 计数收束已经是这样；Barrier 无法承载 payload（结果数据还得另走 channel），纯增开销。仅在"全员必须在安装点前对齐再继续"且无数据传递时才有优势——本协议每轮都有数据回传 | ❌ 不用 |
| **Condvar** | 旧引擎中唯一合法用途是池容量等待 `wait_for_spawn_capacity`（`engine/compute.rs:39-40` 及台账，`engine/compute.rs:172-177` PoolAdmission）——std::thread 模型无线程数管理（用户决策：代码本体无线程数概念，CPU 限制走 OS cgroup/taskset），容量等待整体不存在。多轮协议中 worker 等下一轮指令复用下行 mpsc，无需条件变量 | ❌ 不用（随池面整体删除） |
| 共享 Mutex<Vec<Msg>> 广播 | 单一锁集中竞争 + N 唤醒风暴；劣于每股 channel | ❌ 不用 |

补充：若未来扩到 ~1000 股，逐 send 的 O(N) 唤醒可再评估"共享快照 + 版本号 + 单 Condvar 批量唤醒"，但当前负载（50-100 股）实测原型 982 t/s 已证明 mpsc 足够，不预引入（对齐 memory：并行参数不写死、不过早优化）。

---

## 4. 失败 / panic 传播协议（一股 panic 不挂全局）

原则（继承引擎既有纪律，`engine/docs/architecture.md:148-151`）：panic 是**进程级故障**，不用 catch_unwind 冒充可恢复 tick；catch_unwind 只用于**运输**，最终在 coordinator 侧 `resume_unwind` 原 payload。

### 4.1 worker 侧

- worker 单 tick 执行体（`run_tick` 等价物）包 `catch_unwind(AssertUnwindSafe(...))`：Ok → `Round(result)`；Err(payload) → `TaskDied(stock_id, payload)` 并**立即转入 Shutdown 退出路径**（该 worker 不再参与后续轮）。
- 对齐先例：NPC 决策流 `DecideMessage::TaskDied`（enum 定义 `engine/session/pipeline/npc_decisions.rs:412-416`；处理=停止派发+排空在飞+重抛原 payload，`:495-510`）、`spawn_two_stage` 的 catch_unwind/resume_unwind 对（`engine/compute.rs:883,958`）。
- 内联决策的 panic 天然在同一线程栈展开，被 worker 层同一个 catch_unwind 覆盖——比原型 spawn 模式（scope join 后 `resume_unwind`，`proto/worker.rs:362`）更简单，这是内联方案在失败协议上的额外收益。

### 4.2 coordinator 侧

收到 `TaskDied` 或 recv 到 `Err(Disconnected)`（某 worker sender 全 drop 且未收齐 N 个结果）：

1. **本 tick 判死**：丢弃整个 candidate（Escrow 语义，权威状态零污染）；
2. **全局关停**：向所有仍存活的 worker 发 `Shutdown`，收齐 `Final`（已 TaskDied 的 worker 不再有 Final，收束计数按存活数校验，缺口以 Disconnected 显式对账）；
3. **join 全部线程**（防止泄漏常驻线程）；
4. **`std::panic::resume_unwind(payload)`** 原 panic payload 上抛——不吞、不改写、不折算成普通错误后继续跑（继续跑=静默吞错+守恒审计失去意义，违反铁律二）。

### 4.3 明确不做的

- ❌ 一股 panic 后跳过该股继续模拟（破坏股份/现金守恒审计，`proto/run.rs:282-310` 的守恒审计要求全股在场）。
- ❌ 空 catch / 默认值兜底。
- ⚠️ "一股 panic 不挂全局"的准确边界 = **不挂死（无死锁）、不静默、可诊断**；进程仍以显式失败终止本局。若未来要求"单股故障隔离降级运行"，属新需求，须另立 ADR（涉及守恒与 A 股语义），本协议不预留暗门。

### 4.4 锁中毒

若 v1 过渡期仍有共享 RwLock 读（如决策名册共享读，`proto/worker.rs:179` `Arc<RwLock<Vec<Npc>>>`），poison 必须显式报错（`proto/worker.rs:571-582` 的读/写锁中毒都映射为显式错误）；资源实体模型落地后共享读写锁消失，此条自然退化。

---

## 5. 文件面预估

### 新增（建议落 `packages/engine/src/session/pipeline/stock_workers/`，与 proto 对应命名）

| 文件 | 内容 | 预估行数 |
|------|------|---------|
| `protocol.rs` | `TickMsg` / `StockReport` / `StockRoundResult` / `StockFinalState` / `TickRound` 消息与构造校验 | ~150（+测试 ~120） |
| `coordinator.rs` | tick 广播/收束/多轮/安装循环、关停、panic 收口（对齐 `proto/run.rs` 骨架 + 引擎收束安装语义） | ~350（+测试 ~250） |
| `worker.rs` | 每股持久线程主循环：recv→run_tick（决策内联+撮合+本股收尾）→report；panic 运输；期末上交（对齐 `proto/worker.rs:164-264`） | ~300（+测试 ~200） |

### 删除（随迁移退役）

| 文件/面 | 行数（约） | 依据 |
|--------|-----------|------|
| `engine/compute.rs` 主体（全文 1316 行）：`ComputeContext`/`PoolAdmission`/Condvar 容量等待/`active_width`/`spawn_two_stage`/`parallel_gather`/全部测试探针/内联 `parallel` 模块 | ~1316（除 WASM 直通遍历 shim 可按需保留外整体退役） | `engine/compute.rs:21-177`（上下文与台账）、`:883/958`（panic 运输）、`:1173-1178`（parallel 模块门控） |
| `engine/session/pipeline/stock_stream.rs` 协调层：`StockStreamCoordinator`/dispatch/poll_progress/admission/notifications | ~600（保留 `continuous_shards` 构造与 finish 归并部分迁入 worker/coordinator） | `engine/session/pipeline/stock_stream.rs:263-856` |
| 活性契约的台账/容量/让位测试簇（round3-5 红绿测试） | 数百行测试 | 契约本身重述为 §4/§2 的两条简单前提 |

### 改造

| 文件 | 改动 |
|------|------|
| `engine/session/pipeline/market_tick_transaction.rs` | `apply_session_market_transaction` 的 join/drive 段（`:369-432`）替换为广播+收束；`finish_market_execution` 收束侧保留 |
| `engine/session/pipeline/npc_decisions.rs` | `stream_npc_account_tasks_with`（`:387-703`）的池派发/容量/协作让位退化为 worker 内联顺序调用（chunk 化删除） |
| `engine/session/pipeline/mod.rs` | `plan_tick` 的并行重叠（`:436-470`）改串行/迁 worker；`precomputed_stock_shards` 快速路径退役 |
| `engine/session/protocol/commit.rs` 等 GameSession 构造面 | `Arc<ComputeContext>` 字段与构造移除（另需 `GameSession` 持有 worker 句柄或由外层 Runner 持有——建议 Runner 持有，GameSession 保持纯数据权威，避免权威态背线程资源） |
| `ready_stock_stream.rs` / `ready_ingress.rs` | `continuous_progress` 的路由逻辑并入多轮协议的 foreign_ops 组装 |

净估计：新增 ~1400（含测试） / 删除 ~2200+（compute.rs 1316 + stock_stream 协调层 ~600 + 台账测试簇）/ 触及文件 ~12-15 个。测试面需新增：协议往返正确性、多轮续行、TaskDied 运输、Shutdown 对账、断连显式失败（对齐 `engine/session/pipeline/stock_stream.rs:548-661` 现有错误语义测试的迁移版）。

---

## 6. 风险与开放问题

1. **多轮协议的轮数上界**：依赖计划链有限步（与现有 drive 循环同一前提）；若存在理论无界续行形状，需在设计中显式登记轮次上限并按"契约外形状"处理（对齐活性契约的显式不支持清单纪律）。
2. **跨股账户决策分派**（§2.4）：账户→主股票的分派规则若涉及"账户当 tick 涉及多股"的公平性/语义问题，需与任务B（NPC 决策内联）联合定稿；本报告给出的是协议通道，不锁定分派策略。
3. **GameSession 与 worker 句柄的持有关系**：建议 Runner/宿主持有 worker 线程与 channel，GameSession 不变重（保持 Escrow 权威纯数据）；持久化/恢复时 worker 池重建即可（无线程状态需要保存）。
4. **拍卖/PreOpen 阶段**：spine-simplify-01 已统一交易簿（拍卖走撮合模式，任务#53），协议消息天然覆盖；但 auction 边界的收束安装（`AuctionTickBoundary`）在多轮协议下的轮次表达需在实现时按 `auction_day_end` 语义核对。
5. **CPU 资源限制**：按用户决策走 OS 级（cgroup/taskset），协议与代码本体不含线程数管理概念、无 worker 上限参数（`per_stock_threads`/width 等原型对照臂参数不进入引擎本体）。
