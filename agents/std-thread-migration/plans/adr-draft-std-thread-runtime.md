# ADR 0043：std::thread 所有权转移运行时（一股一交易部线程 + NPC 组线程）

> 草案（2026-10-17，批 10 文档批评审用；未入库）。名词与代码对齐锚点：
> `packages/engine/src/runtime/`（mod/protocol/entities/facts/group/desk/coordinator/
> watchdog/world/drive）。终态"已采纳"待批 11（全量回归 + s5 正式基准 + 独立复核）
> 通过后勾选。

- 状态：草案（Draft）——实现已落地、缺省未翻转（见"后果"第 6 条）
- 日期：2026-10-17（决策链 2026-10-11 ~ 2026-10-16）
- 决策者：用户拍板（两阶段模型判定 2026-10-11；std::thread 内联方案 2026-10-14；
  资源实体所有权转移模型 2026-10-15）；实现由 std-thread 迁移批 0~9 各任 agent 执行
- 编号说明：上一编号 0042；0037–0040 已被占用

## 背景

### 1. rayon 宽度税与串行脊（性能主线实测，16 核 server 路径 s5 负载）

- 优化后仅 ~220x~310x；多核不升反降：128w 档 **−43%~−63%**、每 tick CPU 12~21×；
  16w 仅用 42% CPU（串行脊限死真实并行度）。
- worker 曲线倒 U：4w 293x vs 16w 213x——池宽与真实可并行负载错配（**宽度税**），
  调参类方案（默认宽度/粒度治理/分级池）无法救。
- 根因画像：贵活在串行脊柱（克隆/编排/验证），并行道只剩便宜活；jemalloc 释放
  ~52% + 容器析构 ~20% 的"构建→拆毁→释放"churn。
- 用户判定（2026-10-11，两阶段循环模型）：tick 核心只有两件事——**NPC 决策（并行）→
  交易簿撮合（并行）**，顶多 commit 前对 view 做一次同步；多核倒退是**并行模型不对**
  的结果，不是参数问题。据此否决全部"现有模型内调参数"方案。

### 2. 原型三臂数据（`packages/std-thread-proto`，s5 负载=5 股 / 20007 NPC / seed 42 / 3000 tick）

| 臂 | 机制 | 实测 | 跨股语义 | 结论 |
|---|---|---|---|---|
| 按股私有臂（`worker.rs`） | 一股一持久线程，NPC 名册按股私有，外部槽恒空 | ~975–1000 t/s（inline 口径，多轮采样） | **不可承载**（retail 全市场选股/跨股持仓无处安放） | 语义否决 |
| 跨股中转臂（`xworker.rs`） | 家端账户+外部席位 escrow 预留+coordinator 三相位中转+回执记账 | x0（0% 跨股）~845–870 t/s；x30（30% 跨股）**~484 t/s** | 完整 | 性能与复杂度否决 |
| **所有权转移臂（`oworker.rs`/`orun.rs`）** | NPC 分组线程持账户，实体 move 直发交易部，零锁零共享 | **849 t/s@5 组 / 962 t/s@10 组**（用户确认口径）；纯机制税 ~8%（decide 等待 1.017 ms/tick、match 等待 0.114、dispatch 0.012；实体 26558 条/tick、消息 200+25 批/tick；现金/股份守恒、T+1、整手零违例） | 完整 | **采纳** |

对照：rayon 形态同负载 359 t/s（std-thread 原型 982 t/s 的 1/2.7）。

### 3. 决策链

2026-10-14：迁移到纯 std::thread 内联方案——一股一个持久线程、tick 内联决策、
统一 join → 多线程撮合 → 收束；**代码本体不出现线程数管理概念**（无 worker 数、
无池宽、无 RAYON_NUM_THREADS）；CPU 限制用 **OS 级**能力（cgroup cpu quota /
taskset affinity）。2026-10-15：确认最终形态为**资源实体所有权转移模型**
（NPC 分组线程 + 实体 move），即收敛架构 F（`.tmp/migration-design/
F-converged-architecture.md`）。

## 决策

以下条目名词全部与代码一致（括号为锚点）：

1. **拓扑**：G 个 NPC 组线程（`world.rs` 组线程 spawn，组数=世界构造参数，名册连续
   区间均匀切分、组与股无亲和）+ N 个交易部线程（一股一个，独占本股订单簿与在途
   实体）；协调者逻辑在驱动器调用线程上运行，不单独建线程（`world.rs` 模块注释）。
   线程数由组数与股数决定，**代码本体零线程数管理概念**。
2. **所有权转移（非 escrow 预留）**：资源实体=可 move 数值载体（`Entity` 五类：
   `CashProceeds`/`CashRefund`/`BoughtShares`/`RefundedShares`/`Closed`，
   `entities.rs`）；下单时现金/股票从组账户**实际扣出**随 `DeskIn::Place` 转移
   （买单 `purse`=最坏情况对价+佣金上限+过户费上限，卖单 `purse`=0、qty 股实扣）；
   成交时实体互换、终结/拒绝/到期原路退回。粒度=NPC 现金一个聚合额、每
   (NPC, 股票) 持仓一个聚合额。挂单在途期间该笔资金/股票**不在**组账户里。
3. **零锁零共享**：同步原语全 `std::sync::mpsc`；组↔交易部全互联直发
   （G×N 下行 `Ops` + N×G 上行 `DeskBatch` 实体批次）——**数据面（订单/实体）
   不经协调者**；无 Barrier、无 Condvar、无共享 Mutex。
4. **tick 协议（两阶段模型的落地形态）**：协调者 `broadcast_start`（携带
   `Arc<TickSnapshot>` 广播快照）→ 组内决策∥（复用 `decide_data` 纯函数）+ 实体
   扣除 + **分段 flush**（chunk 流水线，决策与撮合重叠）→ 交易部边收边撮∥
   （`begin_tick`：隐式转正→日界重置→行情快照→开盘清算→到期撤销）→
   `converge_round` 收齐 G 个 `GroupReport` + N 个 `DeskReport`（**commit 前唯一
   同步**）→ `TickEvidence` 过校验后单点安装，或广播 `Abort`。
5. **同 tick 多轮（计划续行）**：任一组上报续行 → `broadcast_continue(round+1)`；
   组先落账本轮实体（记逆操作）再续行决策；整个多轮 tick 是一个原子单元；终止=
   全部组续行 ops 为空（计划链有限步）。
6. **失败原子性：三层 working overlay + 单点裁决**：组账户（本 tick 实体扣除+已发
   `DeskIn` 日志+staging+组端可逆日志）/ 交易部簿（撤销日志 COW+实体出口缓冲）/
   协调者（私有 candidate）。转正由协调者的下一条消息唯一裁决：组=收到下一 `Start`
   （隐式），交易部=**下一 tick 首批 `Ops`**（tick 字段前进即隐式转正）；`Abort`
   逆序回放全部日志回到上一收敛态。等价于旧管线"整体 clone candidate、失败消费式
   丢弃、成功单点安装"。
7. **锁步不变量（正确性基石，`protocol.rs` 模块注释）**：交易部 flush 实体批次
   **先于** `DeskReport` 发送；协调者收齐 G+N 报告**之后**才广播下一 `Start` ⇒
   组线程收件箱中 tick N 的全部实体先于 `Start(N+1)` 入队（mpsc FIFO +
   happens-before）。驱动器侧读数=`world_committed_tick() == session.tick()`
   （`drive.rs`，失配=显式 InvariantViolation）。
8. **同拍交付双写**：`step()` 返回前 `apply_same_tick_fill_projection`
   （`drive.rs`）把本拍成交事实合入公开 session 账户（持仓/T+1/成本/现金/执行腿
   ——ADR-0017 完整结算同拍交付）；组内 staging 下一 `Start` 合并机制保留=**双写**
   （投影即时、组权威随后整值对齐，下一拍不双计）。
9. **panic 运输协议**：worker 单 tick 执行体 `catch_unwind`，payload 经
   `UpMsg::TaskDied` 上行；协调者广播 Abort+Shutdown → 收齐 `GroupFinal`/
   `DeskFinal` → join → `resume_unwind` 原 payload（不吞、不改写、不跳股继续）。
10. **事实流与实体分离**：`FillFact`（单腿视角，计划续行）/`DeskReceipt`（受理与
    撤单回执）/`DeskTradeFact`（双腿，`maker≡卖腿`、`taker≡买腿`——ADR-0042）/
    `DeskAuctionFact`/`DeskLifecycleFact` 只做信息回流，不做资源转移；
    `DeskBatch` 携带 `stock`/`tick`/`round` 防御字段，组端 `verify` 错配=显式失败
    （`DeskBatchMismatch`），不静默落账。
11. **CPU 资源限制走 OS 级**（cgroup cpu quota / taskset affinity），部署层负责；
    引擎代码、环境变量、配置面均不出现线程数旋钮。

## 被否方案

1. **B 桥接（session 分区桥接）**：账户留协调者+计算亲和下发、股域 working/
   committed 双份+epoch 转正（迁移设计 B）。否决理由：每 tick clone 双份正是一个
   被否决的往返形态；权威分布后整体克隆无对象。其全局域归属清单、lease 只读投影、
   存档重建、毒化传播结论被本模型吸收保留（F §8）。
2. **池宽封顶**：rayon 128 线程池封顶止血。用户否决——128=真实 128 线程池不许缩；
   宽度税治理止步 A+B，不做封顶（memory `feedback_no_pool_width_cap`）。
3. **rayon 补丁**：在 rayon 调度器上打补丁修宽度税/嵌套池边界。用户否决——直接换
   更简单的 std::thread，不再投资 rayon（架构文档旧文记录的
   `in_place_scope` 嵌套池已知边界随 rayon 退役整体消失）。
4. **原型按股私有臂**：跨股语义损失不可接受——A 股语义要求 retail 全市场均匀选股、
   inst/hot 全市场扫描、跨股持仓常态。
5. **原型跨股中转臂**：escrow 预留+席位镜像+三相位 coordinator 中转：x30 实测
   ~484 t/s，且共享回执记账重新引入协调点。
6. **NPC 分组+放松延迟**（决策→执行延迟 1~2 tick）：登记为备选，默认不启用
   （memory `npc_grouping_relaxed_latency`）；仅当续行轮/退款时序基准证明必要时按
   数据提请用户拍板。

## 后果

**正面**：

- 机制口径 962 t/s（10 组调优，s5 负载），约为 rayon 形态 359 t/s 的 2.7 倍；
  纯机制税 ~8%（批量消息+流水线重叠）。
- 零锁零共享：并发正确性由消息协议（FIFO+收束计数）承载，死锁面结构性消失；
  活性契约 std-thread 重述（任务独立/有限完成/协调者阻塞安全/等待目标无条件入队/
  看门狗逃逸——相位感知 `recv_timeout`，`watchdog.rs` `Phase`），liveness-contract
  v5 逐前提映射不弱于 v4。
- 代码本体无线程数概念：部署弹性全部交给 OS（cgroup/taskset），引擎不再携带
  worker/池宽/环境旋钮及其全部测试与台账负担。
- 等价性门禁：A 股语义映射表 30 项（F §7）逐项落地；Abort 回滚等价（tick 起终
  快照哈希一致）、现金/股份守恒、T+1、整手、费用完整口径均有测试锚定。

**负面/限制（如实登记）**：

1. **WASM 串行回退**：wasm 宿主无 OS 线程，`step_runtime_enabled` 恒 `false`
   走串行管线（`runtime/mod.rs`，Web 优先产品语义；已知限制，不做 wasm 线程化）。
2. **线程数随规模线性增长**（G+N，协调者复用调用线程）：测试进程线程数上升，
   `World` Drop 显式 Shutdown 防泄漏；50~100 股 / 128 核场景的组数缺省推导公式
   待基准标定后写入 setup 契约（不写经验魔数）。
3. **严格所有权的单轮资金不可复用**：本 tick 撤单释放的资金/可卖下一 tick 才可用
   （退款实体次拍落账）；组端 skip 计数显式暴露资金不足跳过，不静默。语义差登记
   `trading-rules.md`（对齐撤换单轮复用取消、reserved 口径改"实体在途"）。
4. **上行负载大于原型基准口径**：正式引擎 `FillFact`/`DeskReceipt` 逐条上行
   （原型仅 Entity 聚合批）——962 t/s 只能作机制口径参考，正式基准批 11 重标。
5. **同拍交付双写**引入投影与组权威两条写路径：由"下一拍整值回写"保证不双计，
   测试面已锚定（同拍全量投影/Abort 零泄漏）。
6. **翻转现状**：`step_runtime_enabled()` 缺省 `0`=旧管线；`ENGINE_STEP_RUNTIME=1`
   显式启用 runtime。终态=全局域迁移完成、缺省翻转、旧管线与开关退役（批 9 C4）
   ——本 ADR 落库时按当时状态如实更新本条。
7. 迁移期双轨（翻转前）旧管线测试与 runtime 测试并存；旧管线退役批按"死亡测试
   台账"逐条登记机制替代，不静默删。

## 关联

- 收敛架构全文：`.tmp/migration-design/F-converged-architecture.md`（实施期工件，
  结论以本 ADR 与代码为准）
- 批次计划：`.tmp/migration-design/G-batch-plan.md`（12 批；本 ADR=批 10 件）
- 相关 ADR：0017（escrow tick 与验证边界——语义等价基线）、0025（日终档边界）、
  0041（StockId 数值身份）、0042（maker/taker 腿方向语义）
- 后续文档：liveness-contract v5、`trading-rules.md` 三处登记、`architecture.md`
  更新清单（见本文附录）

---

# 附录：architecture.md 需要更新的段落清单（旧描述 → 新描述对照，不写全文）

> 共 8 个更新点。行号以当前 `docs/architecture.md`（194 行版）为准。

| # | 位置 | 旧描述 | 新描述（要点） |
|---|---|---|---|
| 1 | §1 核心设计理念（L27-31"关键点"） | "`game-engine` 是纯逻辑：输入状态+动作，输出新状态+事件。不碰 DOM、网络、定时器" | 保留；补一条：native 运行时（ADR-0043 std::thread 所有权转移模型）内部自管 OS 线程与 mpsc channel，但对外仍是纯函数契约（`step()`），不碰任何 I/O；wasm 宿主恒走串行管线 |
| 2 | §5 目标目录结构（L113-119 packages 树） | 树中只有 `engine`、`engine-gpu` | 补 `engine/src/runtime/`（组线程/交易部/协调者/协议/world/驱动器，ADR-0043）；补 `packages/std-thread-proto/`（三臂原型包，批 11 验收后退役） |
| 3 | "Escrow tick 与验证边界（ADR-0017）"节·管线段（L128-131） | 单候选脊相位流水：SealAllocationSnapshot→DecisionShadow→AccountValidation/StockProcessing→ReceiptAggregation→Settlement→Projection→PreCommitValidation→CommitTick；"线程预算为 1 不是另一套串行引擎" | runtime 消息协议描述：`broadcast_start`（`Arc<TickSnapshot>`）→ 组决策∥+分段 flush ∥ 交易部撮合（`begin_tick`）→ `converge_round` 收齐 G+N → 单点安装/Abort；等价语义（失败原子性/守恒/事件 seq 单调/同拍交付）保持，载体改三层 working overlay；串行仅 wasm 回退，native 恒并行拓扑 |
| 4 | 同节·rayon 段（L156-160） | "股票流使用 `rayon::in_place_scope`……不创建额外线程……嵌套 tick 协调者仍可能耗尽同一个池……" | 整段删除，替换为 std-thread 拓扑一句（G 组线程+N 交易部线程+协调者复用调用线程、数据面直发不经协调者、锁步不变量、相位感知看门狗），引 ADR-0043 与 liveness-contract v5 |
| 5 | 同节·panic 段（L138-140） | "panic 是进程级故障，不使用 catch_unwind 冒充可恢复 tick" | 分述：串行管线该语义保持；native runtime 的 worker panic 走运输协议（`catch_unwind` 捕获→`UpMsg::TaskDied` 上行→Abort 回滚→收齐 Final→join→`resume_unwind` 原 payload 重抛）——tick 仍判死、已收敛权威未被触碰，是显式失败运输，不是冒充可恢复 tick |
| 6 | §4 状态与持久化（L82-87） | SavedRuntimeState 投影/恢复描述（单线程权威隐含） | 补：恢复=日终静点重建线程拓扑（组/交易部线程重建，`initial_committed_tick` 锁步对齐 `session.tick()`）；活动委托不进档（日终清簿后天然为空）不变；日内回滚=Abort 三层 overlay 冲回 |
| 7 | §6 数据流（L180） | "核心层 `GameSession.step()` → Event[]" | 签名不变；内部经 `step_runtime_enabled` 分派（终态=runtime 唯一路径、开关删除，批 9 C4；wasm 恒串行） |
| 8 | §7 待定清单（L188-192） | 无并发模型条目 | 增补已敲定项：并发模型=std::thread 所有权转移运行时 ✅ ADR-0043；rayon 相关旧表述全部随第 3/4 点改写清除 |
