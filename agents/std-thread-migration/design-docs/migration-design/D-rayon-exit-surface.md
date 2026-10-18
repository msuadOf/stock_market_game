# 任务D：rayon 退出面清单

> 分析对象：spine 树（`.worktree/spine-simplify-01/packages/engine/src/`，106 增量冻结链终态）。
> 目标模型（用户决策 2026-10-14）：纯 `std::thread`，一股一持久主线程 + 内联 NPC 决策，
> 代码本体无线程数管理概念，CPU 资源限制交给 OS（cgroup/taskset）。
> 参照原型：`.worktree/std-thread-model/packages/std-thread-proto/src/`（worker.rs 一股一线程 + ThreadGauge）。
> 本文只读分析，不改任何源码。所有行号以 spine 树当前文件为准。

## 0. 盘点总量

- `rg 'rayon|par_iter|par_bridge' packages/engine/src/` 命中 **108 处、42 个文件**（`rg -c` 汇总）。
  其中约 **19 个纯测试文件 / cfg(test) 模块**，**23 个生产文件**含 rayon API 或 par_iter 直呼。
- 另有 **约 15 个生产文件**不含 rayon 字样、但经 `ComputeContext`（字段/参数/`record`/`yield_work`/`install`）间接依赖，ComputeContext 退役时必然编译受牵连。
- crate 外：`apps/server`（6 文件）、`apps/web-wasm`（2）、`apps/web/vite.config.ts`（1）、`packages/engine/Cargo.toml`、`apps/server/Cargo.toml`、根 `Cargo.toml`（workspace `rayon = "1"`，根 Cargo.toml:22）。
- `packages/engine-gpu/` 无 rayon 引用（rg 零命中），不受影响。

## 1. 逐处分类

分类口径（按任务要求）：
- **(a) 可直接替换为 std::thread + channel**：控制流等价搬运（join/两点分支/后台任务）。
- **(b) 需重写算法逻辑**：par_iter 并行循环改手写分片线程组、或改消息分发到股 worker、或退化为串行（按测量定）。
- **(c) 可删除**：整套随池存在的机制（准入台账、宽度封顶、install 包装、探针、观察）。

### 1.1 (c) 整体退役——compute.rs 本体（1316 行，几乎全文）

| 退役项 | 源码依据 |
|---|---|
| `ComputeContext.pool: Arc<rayon::ThreadPool>` | compute.rs:23 |
| 池准入台账 `PoolAdmission{drives,spawned}` + `admission: Mutex` + `capacity_changed: Condvar` | compute.rs:38-40, 172-177 |
| `PoolDriveSlot` / `SpawnSlotGuard` 守卫 | compute.rs:181-211 |
| 入口条件准入 `try_enter_pool_drive_slot` | compute.rs:547-559 |
| 派发预留 `reserve_spawn_slot` / `release_spawn_slot` / `wait_for_spawn_capacity` / `has_spawn_capacity` | compute.rs:565-629 |
| 宽度封顶 `active_width` / `set_active_width` / `parallel_grain` | compute.rs:49, 754-784, 1159-1163 |
| scatter-gather 原语 `parallel_map_chunks`（准入耦合版） | compute.rs:800-972 |
| `install` / `parallel` 包装（池往返 + 绑定传播） | compute.rs:987-1027, 749-752 |
| `join` / `scope` / `in_place_scope` 池版 | compute.rs:693-736 |
| `is_worker`（`pool.current_thread_index()`） | compute.rs:738-747 |
| `yield_work`（`rayon::yield_now` 协作让位） | compute.rs:1165-1170 |
| `pub(crate) use rayon::Scope` 与 WASM `Scope` 垫片 | compute.rs:1124-1147 |
| `pub(crate) mod parallel`（native=re-export `rayon::prelude::*`；WASM=顺序 trait 垫片） | compute.rs:1172-1219 |
| 构造面 `with_workers`（`rayon::max_num_threads`/`ThreadPoolBuilder`）/ `from_pool` / `available` / `shared_default` | compute.rs:444-527 |
| 验证探针组：`observer`/`dispatch_gate`/`capacity_wait_probe`/`admission_snapshot`/`T1Probes` 及全部 take_*/begin_*/end_* | compute.rs:50-64, 70-120, 1029-1080 |
| `ComputeObservation`（含 `rayon::current_num_threads` 读取） | compute.rs:251-259, 1091-1097 |
| `ComputeWork` 枚举 + 生产代码里的 `compute.record(...)` 调用点 | compute.rs:230-249；调用点见 §1.4 |

注意：`CpuBackend::decide_all`（compute.rs:1261-1292）是**裸全局池 par_iter**（compute.rs:1279），属 (b)；`ComputeBackend/ComputeMode/create_backend`（compute.rs:1227-1316）接口本身与 rayon 无关，可保留。

### 1.2 (a) 可直接替换为 std::thread + channel

| 调用点 | 现状 | 替换 |
|---|---|---|
| read_only_start.rs:113 `crate::compute::join(...)` | 两分支并行 | `std::thread::scope` 两分支；或顺序内联（分支小） |
| indicators.rs:256 `context.join(macd, ‖ kdj, kdj_ohlc)`（≥2048 样本） | 嵌套 join | `std::thread::scope`；`Option<&ComputeContext>` 参数（indicators.rs:243）随之删除，`None` 分支即串行路径，全部统一为串行或 scope |
| market_tick_transaction.rs:392 `compute.join(...)`（ready/prepared 两分支） | 同上 | scope 或顺序 |
| compute.rs:635-690 `spawn` / `spawn_two_stage`（fire-and-forget，绑定传播） | 池 spawn | `std::thread::spawn` + mpsc；多数调用点随流式改造消失（stock_stream/npc_decisions 重写后不再有后台任务） |
| stock_stream.rs 派发路径 `in_place_scope(|scope| ... scope.spawn)`（stock_stream.rs:725 起） | 每股任务借 rayon scope 借用 | 模型主体替换：每股**持久** `std::thread` + `mpsc::{Sender<Msg>, Receiver<TickDone>}`（原型 worker.rs:81-98 的 `TickMsg/TickDone`），一次性创建、跨 tick 复用，不再是每 tick spawn |

### 1.3 (b) 需重写算法逻辑（par_iter → 手写分片循环 / 消息分发 / 串行）

分两子类：**股维度循环**（每股一 worker，改成向股线程发消息，由 worker 内联执行）与**非股维度批量**（临时分片线程组或串行，按数据拍板）。

股维度（→ 分发到股 worker）：
| 调用点 | 说明 |
|---|---|
| stock_stream.rs:130 `continuous_shards` 初始化 into_par_iter | 每股建 shard，改 worker 初始化路径 |
| stock_stream.rs:189 `finish_all` into_par_iter | 每股收尾，改 TickDone 汇合 |
| incremental_continuous_stock_shadow.rs:350 `apply_round` into_par_iter | 每股影子推进，同上 |
| continuous_matching_adapter.rs:73 `prepare` par_iter（包 compute.parallel） | 每股输入准备 |
| views.rs:15 / views.rs:125、self_views.rs:15 | 逐股观察/挂单收集 |
| decision_chain.rs:111 `build_technical_on` into_par_iter | 逐股技术观察 |

非股维度（→ 临时分片线程组或串行）：
| 调用点 | 说明 |
|---|---|
| notices.rs:163 逐 NPC 日终交付 into_par_iter | 账户维度 |
| decision_chain/retail_analysis.rs:65 逐账户个人分析 | 账户维度 |
| decision_snapshot_capture.rs:196 逐 NPC 快照 par_iter | 账户维度 |
| settlement.rs:146 逐账户结算 into_par_iter | 账户分组 |
| retail_projection.rs:416 逐账户投影 into_par_iter | 账户分组 |
| account_validation.rs:461 逐 lane 校验 par_iter | lane 分组 |
| account_paged_map.rs:90 / :127 逐分组页写入（**裸 par_iter，无 install 包装**） | Arc 分组 |
| account_validation_context.rs:58 逐股 facts（裸 par_iter） | 串行化最廉价 |
| live_reservations.rs:172 ≥512 行页投影 into_par_iter（<512 已串行） | 阈值分支保留、并行臂改写 |
| continuous_matching.rs:936 ≥512 挂单校验 par_iter + `map_init` | 保留阈值，并行臂改写 |
| ledger_conservation.rs:228 `install + rows.par_iter().with_min_len` （`Option<&ComputeContext>` 口径，None 分支已串行） | 退化为常串行或临时分片 |
| account_book.rs:104 `parallel_map_chunks` 页快照 | 串行或分片 |
| npc_state_projection.rs:578 `compute.parallel` | 内联 |
| market_tick_transaction.rs:248 `compute.parallel(validator.finish())`、:369 `compute.install` | 内联（去包装） |
| candidate_commit.rs:73 `rebase_compute.parallel` | 内联 |
| CpuBackend::decide_all compute.rs:1279 | 手写分片（lib 公共 API，tests/compute.rs 依赖其契约） |

去包装即删的 `compute.install/compute.parallel`（调用点原地执行）：ready_ingress.rs:202/206、decision_resources.rs:96、上述各 par_iter 包装点。

### 1.4 (c) 生产 `compute.record(ComputeWork::…)` 观察调用点（随 record 机制删除）

decision_chain.rs:113、retail_analysis.rs:67、notices.rs:161、continuous_matching_adapter.rs:71、settlement.rs:148、retail_projection.rs:418、ledger_conservation.rs:227、read_only_start.rs:107、plan_chain_candidates.rs:778、decision_snapshot_capture.rs:198、market_tick_transaction（install 内隐式）。apps/server/tests/actor.rs:182-191 的 `ComputeWork` 观察断言随之重设计或退役。

### 1.5 (c) 流式协调里的池准入分支（随重写删除）

- stock_stream.rs：`on_pool_worker`/`pool_slot`（:349-358）、`try_enter_pool_drive_slot`（:364）、`reserve_spawn_slot` 派发点（:473）、单 worker `try_recv+yield_work` 协作循环（:627 起）、`wait_for_spawn_capacity/has_spawn_capacity`（:656-657）、`workers()==1` 分支（:627, :740）。
- npc_decisions.rs（790 行）：`is_worker`/`try_enter_pool_drive_slot`（:422-423）、`in_place_scope`（:440）、`parallel_grain+active_width`（:448）、`reserve_spawn_slot`（:536）、`wait_for_spawn_capacity`（:653）、`is_worker` 收尾分支（:658）。目标模型下 NPC 决策**内联进股 worker**（原型 worker.rs 模块注释 1-5 行），整个账户任务流的大半（spawn/归并/准入）消失。
- plan_chain_candidates.rs：`is_worker`（:571, :1075）、`reserve_spawn_slot`（:743）、`wait_for_spawn_capacity`（:1190）、`yield_work`（:592, :1042, :1064）。
- pipeline/mod.rs:448-462：池内协作让位 `try_recv+yield_work` 循环。

### 1.6 仅注释/文档提及（顺手更新，无行为）

session.rs:4629（par drop 历史）、deferred_drop.rs:3（E7 教训）、npc_split.rs:4/:311（rayon::join 借用论证——测试用 rayon::join 验证不相交借用，生产仅注释）、indicators.rs:237/:576/:582（全局池惰性初始化护栏说明）、npc_decisions_quota_tests.rs:166。

## 2. ComputeContext / PoolAdmission / liveness 契约退役清单

**类型与字段**（compute.rs）：`ComputeContext` 全部字段（pool/admission/capacity_changed/active_width/observer/dispatch_gate/capacity_wait_probe/t1_probes）、`PoolAdmission`、`PoolDriveSlot`、`SpawnSlotGuard`、`ComputeObservation`、`T1Probes` 及见证记录（`FirstDeliveryRecord`/`RootHandoverRecord`/`AttentionCallRecord`，cfg(test)）、`ComputeInitError`（池构建错误变体 `PoolBuild/UnsupportedWorkerCount/ThreadsUnavailable` 失去对象；`InvalidWorkers/AvailableResources` 若保留 OS 探测则留）。

**API 面**：`with_workers/from_pool/available/shared_default/workers/is_worker/install/parallel/join/scope/in_place_scope/spawn/spawn_two_stage/parallel_map_chunks/active_width/set_active_width/try_enter_pool_drive_slot/reserve_spawn_slot/wait_for_spawn_capacity/has_spawn_capacity/admission_snapshot/set_observer/set_dispatch_gate/set_capacity_wait_probe/record`；自由函数 `join/scope/parallel_grain/yield_work`；`mod parallel`。

**持有与构造面**：
- session.rs:1363（字段）、:1778/:1820/:3224（shared_default）、:1786/:1804/:3230（带 compute 构造）、:1795 `compute_context()`、:1802 `rebind_compute_for_verification`。
- protocol/civil/session.rs:101/:153/:174（shared_default）。
- lib.rs:68-71 导出。
- server：apps/server/src/lib.rs:169-185（host_compute_context）、actor.rs:981-1007（compute 字段/compute_workers）、:1548（compute_context 共享）。

**liveness 契约**：现行契约（`.tmp/spine/round5-fix/liveness-contract.md` v4，四前提）以 rayon 术语表述——多 worker 台账不变式 `drives+spawned≤workers`、单 worker 池 `try_recv+yield_work` 协作自足、池外协调者容量等待、等待目标无条件入队。台账与协作让位机制删除后：
- 新模型活性来源 = 每股线程无共享等待点 + coordinator 每 tick 阻塞 `recv` 固定数量 TickDone（无偷取、无嵌套池内等待，死锁面结构性消失）。
- 契约文档必须**重写**（保留实质：无死锁/无饿死/等待目标必然推进；删除 workers/台账/让位术语），登记"TickStart 已同步送达才进入等待"对应第四前提。这是文档批次，不是代码批次。

## 3. WASM / cfg 分支影响

- compute.rs 现有 **17 处 `cfg(target_arch = "wasm32")`**（:358/:468/:497/:660/:686/:702/:717/:732/:743/:763/:780/:979/:1023/:1096/:1118/:1127-1147/:1177-1219），全部服务于"rayon 不存在于 wasm"。std::thread 在 wasm32-unknown-unknown 上同样不可用 → 新原语（若保留 `join/scope` 类自由函数）仍需 wasm 串行分支；但绝大多数分支随被删代码一起消失，净复杂度大幅下降。
- `apps/web-wasm`：Cargo.toml:26 `wasm-bindgen-rayon = "=1.2.1"`、src/lib.rs:25 `pub use wasm_bindgen_rayon::init_thread_pool;`（lib.rs:3 注释）——整体退役；前端对应 `init_thread_pool()` 调用点与 Worker 初始化需同步删除。
- `apps/web/vite.config.ts:8`：SharedArrayBuffer/COOP-COEP 头是为 wasm 多核 rayon 加的，可退役（实施时核对是否有其他功能依赖 COI 头）。
- `deferred_drop.rs` 已用 `cfg(target_family = "wasm")` 独立实现专用析构线程，**不受影响**（只更新注释里的 rayon 历史教训措辞）。
- 若一股一线程模型要求 WASM 也"一股一线程"，则 WASM 走串行退化（一股一线程顺序模拟）；需在 ADR 明确登记为已知限制。

## 4. 文件总数与增量批次预估

**engine/src 需改动生产文件：37 个**（§1.1-1.5 全列）+ 2 个含 cfg(test) 计算上下文 helper 的生产文件（account_settlement.rs:78/:153、stock_execution_transaction.rs:171）+ npc_tick_preparation.rs（近全测试）。
**engine/src 测试触碰：约 30 个文件**——纯 rayon 测试 19 个（npc_decisions_tests、authoritative_tick_tests、ledger_conservation_tests、ready_ingress_tests、continuous_matching_adapter_tests、receipt_aggregation_tests、decision_resources_tests、continuous_matching_tests、executor_perturbation_tests、ledger_state_tests、settlement_tests、retail_projection_tests、repair_lifecycle_receipt_tests、npc_decisions_quota_tests、initial_candidate_round_tests、decision_snapshot_capture_tests、saved_runtime_tests、phase_timing_tests、tests/step_skeleton.rs）+ ComputeContext/探针依赖约 11 个（account_settlement_tests、fill_fact_tests、plan_quota_dead_zone_tests、t3_boundary_tests、t4inc76_route_cross_batch_tests、plan_chain_candidates_tests、plan_root_quota_wave_tests、civil/{day_end_npc,decision_failure_witness,epoch_alignment}_tests）+ 7 个文件内 cfg(test) 模块（stock_stream/views/self_views/npc_split/local_admission/live_reservations/read_only_start）。
**crate 外**：Cargo 3（根 workspace:22、engine:54、server:28）+ server 5（lib.rs、actor.rs、examples/server_max_speed_probe.rs:14、tests/actor.rs:201、src/actor/fatal_tests.rs）+ web-wasm 2 + vite.config.ts 1 + engine examples 5（max_speed_probe、production_entry_performance、escrow_performance_endpoint、escrow_verification_harness/{runtime,committed}）+ engine tests 2（step_skeleton.rs、compute.rs 复核）。
**文档**：ADR-0008 修订/新 ADR、liveness-contract v4 重写。

**合计约 85 个文件**（生产 ~44、测试 ~32、构建/文档 ~9）。

**建议增量批次（6 批，每批可独立编译+测试）**：
1. **文档批**：新 ADR（std-thread 模型 + OS 级限流 + WASM 串行限制）+ liveness 契约重写。
2. **核心退役批**：compute.rs 重写（删池/台账/宽度/探针；保留或重定义最小原语）、lib.rs/session.rs/protocol 构造面、Cargo 删 rayon（engine 可先留到批 5）。约 8 文件。
3. **模型主体批**（最大风险）：stock_stream.rs、npc_decisions.rs、plan_chain_candidates.rs、pipeline/mod.rs、npc_split 配套——一股一持久线程 + 内联 NPC（移植原型 worker.rs/run.rs 骨架）。约 6 文件。
4. **散点替换批**：§1.3 其余 par_iter/join/install/record 调用点。约 25 生产文件。
5. **测试面批**：删/改 30 个测试文件的 ThreadPoolBuilder/台账断言/活性红测试，重写为新模型等价验证。约 32 文件。
6. **crate 外与清理批**：server/web-wasm/web/examples/Cargo 清理 + CI + 全量回归 + 独立 subagent 复核（大 A 语义门禁）。约 12 文件。

## 5. 高风险点（实施时须显式处理）

1. **绑定传播断点**：compute.rs 的 spawn/install 是 `phase_timing`/`executor_perturbation` thread-local 绑定的跨线程传播点（compute.rs:642-658、:1006-1020；executor_perturbation.rs:97-108）。删除后 verification-harness 特性的扰动/计时注入要么进入股线程入口，要么整体重设计——不能静默失效。
2. **裸 par_iter 的隐式全局池**：account_paged_map.rs:90/:127、account_validation_context.rs:58、CpuBackend（compute.rs:1279）目前不经 install，从非池线程调用会惰性初始化全局池（ledger_conservation.rs:234 登记的 132→260 线程元凶）。迁移后全局池消失，这些点必须显式选择串行/分片，不得遗留。
3. **语义合同保持**：parallel_map_chunks 承诺"结果与首错按索引序、每索引恰一次、panic 原 payload 重抛"（compute.rs:795-799）；手写分片替换必须保持该合同（首错按索引序是大 A 台账与错误展示链依赖）。
4. **`workers()==1` 分支承载的活性特例**（stock_stream.rs:627）删除后，需在批 3 用新模型的汇合结构证明等价活性（契约重写覆盖）。
5. **server 部署合同**：actor.rs `compute_workers()` 与 probe 报告字段（`rayon_threads`，server_max_speed_probe.rs:78）是外部可观察面，需换 OS 亲和/cgroup 报告口径，避免静默字段消失。
