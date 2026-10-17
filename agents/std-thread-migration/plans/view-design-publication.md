# Q1 版本化发布协议（跨线程一致读点）设计深化

> 日期 2026-10-18。工作区 `.worktree/std-thread-model`（只读分析，行号以当前分支为准）。
> 输入：`agents/std-thread-migration/plans/view-campaign-design-brief.md` §3 Q1、
> `agents/std-thread-migration/reviews/sol-view-model-verdict.txt`（终裁 §2/§5）。
> 性质：战役开工文件草案的 Q1 深化稿，供 ADR 草案与原型臂实验引用；非 ADR。

## 0. 问题重述与终裁约束

多组线程（账户/个人态/计划/attention）+ 多交易部（挂单簿/行情/K 线）+ 协调者（全局域）
各自维护权威分片。读者（`GameSession` 查询面、server actor、测试、存档）必须拿到
**一个固定提交版本 + 它的全部分片句柄**，否则分别读各 owner"最新值"会得到账户 N 拍 +
盘口 N+1 拍的撕裂视图。

终裁两条硬约束（设计不得违反）：

- C1：单加一个 epoch 数字解决不了**分片一致性**与**内存生命周期**——读者长期持有旧版本
  不能 use-after-free，也不能无限保留（保留内存或复制成本必须显式分析）。
- C2：working/旧 committed 可并存，但不得成为两套独立演进的业务权威——**唯一发布点**
  之后的数据才算"已发布版本"，candidate 分片在裁决前不外泄。

另一条代码事实约束：

- C3：engine 当前零并发外部依赖（`packages/engine/Cargo.toml` 只有 serde/thiserror/ts-rs；
  runtime 用 `std::sync::mpsc` + `Arc`）。引入 `arc-swap`/`crossbeam-epoch` 属新增核心依赖，
  按 CLAUDE.md 需先有 ADR。代码库已有先例证明 **Arc-句柄随消息走** 的模式可行：
  `broadcast_start`（`runtime/coordinator.rs:223`）每拍构造 `Arc<TickSnapshot>`、
  `stocks: Arc<Vec<StockPublic>>`（`runtime/protocol.rs:216`）逐组 clone 共享。

## 1. 候选方案对比

### 方案 A：发布快照对象（纪元双缓冲语义——不可变快照 + 原子交换，读旧写新）

**机制**：各 owner 在收尾时构建**不可变分片**（`Arc` 句柄），随现有
`GroupReport`/`DeskReport` 上行（不新增消息轮次）；协调者在现有单点安装
（`install_tick`，`runtime/world.rs:1615`）把全部分片句柄组装成一个
`Arc<PublishedTick>`，原子交换进发布槽。读者从槽里 clone 一次 `Arc` 即得一致读点，
包含全部分片 + 全局域 + 事件段。旧版本随最后一个读者释放。

**Rust 类型草图**（std-only 手写版，零新依赖）：

```rust
/// 一个已提交 tick 的不可变发布版本（零 String，标识全数值化）。
pub struct PublishedTick {
    /// 版本身份 = tick 编号（锁步不变量保证全局唯一递增）。
    pub tick: u64,
    /// 全局域（Copy 小结构：committed_tick/day/events_installed——
    /// 即 coordinator.rs:21 GlobalState，直接复用）。
    pub global: GlobalState,
    /// 各交易部分片（desk 线程收尾构建；含 StockPublic、挂单观察面、
    /// fee 累计、candle 段——替代 D4/D6 的会话重装）。
    pub desks: Vec<Arc<DeskShard>>,
    /// 各组分片（组线程收尾构建；含账户投影、计划进度、个人态观察面——
    /// 替代 D1 的 EchoAccounts/Belief/Attention/Plans 上行）。
    pub groups: Vec<Arc<GroupShard>>,
    /// 本拍事件段（连续 seq 的不可变前缀——R4；event_bridge 产物）。
    pub events: Arc<EventSegment>,
    /// 各 owner 提交态哈希（验证观察点，见 §4.3）。
    pub hashes: TickHashes,
}

/// 发布槽：一写多读。
/// 首选实现（零依赖、先测再升）：写者每 tick 一次 write-lock 换 Arc，读者 read-lock+clone。
pub struct ViewPublication {
    current: std::sync::RwLock<Arc<PublishedTick>>,
}
impl ViewPublication {
    /// 读者入口：一次 clone 得一致读点（持有期任意长，无 use-after-free）。
    pub fn load(&self) -> Arc<PublishedTick> {
        self.current.read().unwrap().clone()
    }
    /// 唯一写点（install_tick 内调用；写锁临界区=一次指针赋值）。
    pub fn publish(&self, v: Arc<PublishedTick>) {
        *self.current.write().unwrap() = v;
    }
}

/// 长持读者收窄保留面：按查询面克隆内层句柄，而不是整包。
impl PublishedTick {
    pub fn quotes(&self) -> &[Arc<DeskShard>] { &self.desks } // 只要行情面的读者
}
```

备选存储槽（若 P0 实验显示 RwLock 读侧有争用）：`arc_swap::ArcSwap<PublishedTick>`
（wait-free load，hybrid 策略内藏备份代避免分配churn）——需新增依赖 + ADR；
或手写 `AtomicPtr` + split-count（不推荐：unsafe 面）。

**内存生命周期分析**：

- 无 use-after-free：`Arc` 语义保证；发布槽换新不影响已 clone 的句柄。
- 保留上限=读者行为：慢读者持版本 N−k → 保留 k+1 代的**分片集合**。两种收窄：
  (i) 槽本身只持 1 代（current），保留完全由读者句柄决定——无隐藏多代；(ii) 长持读者
  按查询面克隆内层 `Arc<DeskShard>`/`Arc<EventSegment>`，保留量正比于实际引用的分片
  而非整包（PublishedTick 逐字段是独立 Arc，外层 drop 后内层仍可单独存活）。
- 无 grace-period 停滞：不存在"一个卡死读者阻塞全体 owner 回收"的全局耦合（与方案 B
  的本质区别）；每个版本独立计数，最坏情形=每个活跃读者各钉一代。
- 代价：每拍新分配 PublishedTick 外壳 + 各分片（分配成本进 §4.1 实测定标；外壳是
  固定小结构，分片构建本来就要做——与 D1-D6 删除的安装链是替代关系，非叠加）。
- 双写域（working/committed 并存）合规：owner 线程内 working 副本是私有可变态，收尾
  freeze 成不可变分片才上行；两态不是两套权威——freeze 时机由 Q2 的收尾确认约束。

### 方案 B：RCU 风格延迟回收（epoch pin + 每 owner 独立指针交换）

**机制**：免全局组装。每 owner 一个发布槽（`AtomicPtr`/`ArcSwap` 持自己的分片），
外加全局 epoch 计数器。读者进入临界区先 pin 当前 epoch，逐 owner load 分片，退出
unpin；owner 发布新代后等 grace period（全体读者退出旧 epoch）才回收旧分片。

```rust
pub struct OwnerSlot<T> { cur: arc_swap::ArcSwapAny<Arc<T>> }   // 每 owner 一个
pub struct EpochDomain { global: AtomicU64, per_thread: … }     // pin/unpin 记账
pub struct ReadGuard<'a> { _pin: EpochGuard<'a>, shards: Vec<Arc<dyn ShardView>> }
```

**内存生命周期分析**：

- 回收延迟到 grace period：**一个卡死读者会阻塞所有 owner 的旧代回收**（epoch 域全局
  耦合）——比方案 A 的"各版本独立计数"更差的无界保留风险，需要 epoch 泄漏检测兜底。
- 撕裂防护弱：读者 load owner₁ 分片后、load owner₂ 前，owner₂ 可能已发布新代——
  除非 (i) 每 owner 槽附 tick 字段、读者校验全齐同 tick 否则重试（读放大），或
  (ii) pin 期间禁止发布（退化成全局屏障）。终裁 C1 正是点名这个坑。
- 实现复杂度：手写 epoch 域 unsafe 面大；`crossbeam-epoch` 新增核心依赖（ADR）。
- 理论收益（owner 独立早发布、免组装轮）在锁步 tick 下不成立：tick N 的全部分片本来
  就在 converge 点才齐，"早发布"没有读者能用。

### 方案 C：单写者顺序提交日志（各 owner 视图追加，读者按 tick 序读）

**机制**：每 owner 一条 append-only 视图日志（ring buffer），收尾追加本拍分片；
协调者只发布一个全局 committed-tick 水位。读者取水位 t，再逐 owner 日志按下标
`[t − base]` 取第 t 代分片。

```rust
pub struct ViewLog<T> {
    ring: RwLock<VecDeque<Arc<T>>>,   // 容量 K 代
    base_tick: AtomicU64,
}
pub struct ReaderCursor { t: u64 }    // 读者自持游标；越界=落后于 base
```

**内存生命周期分析**：

- 保留由容量 K + 最慢读者水位管理（类 Kafka consumer offset）：需要每 owner 记录读者
  游标或引用计数才能安全覆写——把方案 A 由 `Arc` 自动做的事手工重做一遍（引用追踪
  逻辑更易错）。
- 慢读者落后超容量 K：要么阻塞写者（不可接受）、要么强制快照拷贝（复制成本显式化）、
  要么踢读者（违反"长期持有"容忍）。三个出口都比方案 A 差。
- 优势面：天然支持"读者按 tick 序回放历史版本"（诊断/回测面有用）——但该需求可由
  方案 A 的旁路（`report_log`/事件段归档）满足，不需要热路径承担。

### 方案 D（补充件，非独立方案）：极小热数据 seqlock 拷贝读

适用面仅限"标量级、极高频轮询"的读面（如最新价 `last_price`）：

```rust
pub struct SeqCell<T: Copy> { seq: AtomicU64, data: UnsafeCell<T> }
// 读：seq 偶数→copy→seq 复核一致；写：seq+=1(奇)→写→seq+=1(偶)
```

只解决单字段原子可见，不解决跨字段/跨 owner 一致——只能作为方案 A 之上的轮询加速件，
不能当发布协议主体。纲要候选 (c) 的定位不变。

### 对比总结

| 维度 | A 发布快照对象 | B RCU epoch | C 提交日志 |
|---|---|---|---|
| 分片一致性 | 构造性保证（单句柄含全部分片） | 需 epoch 屏障或 tick 校验重试 | 需水位+同 tick 校验 |
| 读路径成本 | 一次 clone（锁/原子读） | pin + N 次 load + unpin | 水位读 + N 次下标访问 |
| 慢读者内存 | 各版本独立计数，可按分片收窄 | 全局 grace period 耦合，可能全停 | 容量+游标手工管理，超界需踢/拷 |
| use-after-free | 无（Arc） | grace period 保证（易错） | 覆写前需引用检查（易漏） |
| 失败原子性 | 交换点=裁决后，candidate 不外泄 | 发布点分散，难统一 | 同 B |
| 新依赖 | 无（RwLock 版）/arc-swap(ADR) | crossbeam-epoch(ADR) 或大量 unsafe | 无 |
| 实现复杂度 | 低 | 高 | 中 |
| 与锁步 converge 的契合 | 复用现有收束+单点安装 | 收益无法兑现 | 需新增水位面 |

**推荐：方案 A**，理由：(1) 一致性是构造性的而非协议性的——终裁 C1 的两个坑
（分片一致、生命周期）分别被"单句柄含全部分片"和"Arc 独立计数"正面解决；
(2) 发布点天然落在现有单点安装（R3 必留结构），不新增同步轮次；(3) 零新依赖版本
可先行，ArcSwap 升级留作 P0 实验后的数据决策；(4) 事件段/日结候选（Q5）可复用同一
发布槽扩展。方案 C 的"按 tick 序回放"能力登记为诊断面备选，不在热路径承担。

## 2. 与 tick 边界的关系

### 2.1 发布点：全局提交裁决后（install_tick 单点），不采用各组收尾各自发布

- **发布点 = `install_tick`**（`runtime/world.rs:1615`，成功路径唯一触碰权威处）。
  收束（`converge_round`）已收齐 G+N 报告 → PreCommit 校验 → 单点安装。发布=
  在该函数末尾追加一次 `publication.publish(...)`。读者可见的版本序列与
  `report_log` 严格同序同身份。
- **否决"各组收尾各自发布"**：owner 分散发布 = 方案 B 的撕裂窗口 + 失败原子性破洞
  （见 2.3）；且锁步语义下"早到的分片"没有读者能消费——分散发布只增加协议复杂度
  不增加可用性。
- **与 Q2 的分工**：Q1 只定义**载体与交换点**；分片**内容就绪契约**（组账户已应用本拍
  最终 `DeskBatch`）由 Q2 的收尾确认解决。硬边界：无论 Q2 选报告后移还是两段确认，
  `PublishedTick.groups[g]` 的语义必须是"组 g 已完成本拍收尾后的读面"——方案 A 的
  组装点不变，变的只是分片何时随报告到达。Q2 若选 (c) 派生读面，则组分片=
  派生投影，同样走同一发布槽。
- 多轮（Continue）：只有终轮安装时发布一次；早轮事实已并入累积器
  （`runtime/world.rs:1455-1529`），发布版本含全轮事实——与 TickRecord 口径一致。

### 2.2 部分 tick 失败时读者看到什么

| 失败形态 | 读者视图 | 依据 |
|---|---|---|
| 收束期断连/看门狗超时/worker Failed | 仍是 tick N−1 版本；失败 tick 零外泄 | converge 返回 Failed，install_tick 未执行，无 publish |
| PreCommit 校验失败（`TickValidation`） | 同上；候选分片随 evidence drop 而释放 | `world.rs:1463-1469` abort_tick 分支 |
| worker panic（TaskDied） | 同上；world 死亡后发布槽保留最后 committed 版本可供诊断读取 | 撤退路径不触碰发布槽 |
| Abort 后重跑成功 | 跳到重跑成功的版本（tick 号可能重用），读者按 tick 单调性看到新版本 | Abort 回滚后权威未推进 |

补充保证（C2 的落实）：candidate 分片虽然物理上随 `GroupReport`/`DeskReport`
上行到了协调者，但只存在于 `TickEvidence` 局部变量中，从未进入发布槽——失败路径
消费式丢弃，读者侧不可见。`business_state_hash` 观察点：每版本携带
`hashes: TickHashes`（group_hashes/desk_hashes/GlobalState::state_hash 三族），
验证入口改为"对已发布版本计算/核对携带哈希"，与 `report_log` 逐位一致——
Abort 候选的哈希永不进入发布面。

### 2.3 日结等特殊版本

`DayEndCandidate`（`world.rs:245`）= 日结静点的特殊 PublishedTick 形态
（含组账户投影+fee_sink），同一发布槽在日结隐式提交点（下一 Start，F §6.2）
交换。Q5 复用本协议，不另建机制。WASM 串行回退：单 owner 退化发布=同线程
`publish()`（锁无争用），协议面不变——满足纲要风险表"wasm 显式建模"要求。

## 3. 实验 judging 标准（三指标）

> 全部实验按方法学 §1.1 净机窗 + 基线 worktree 配对；原型臂复用 `std-thread-proto`
> 三臂方法论。三指标 = 延迟、内存、一致性；任一不过即按 CLAUDE.md 负结果归因流程
> 处置，不静默关闭。

### 3.1 延迟（发布成本 + 读路径成本，两个子指标）

- **J1 发布开销**：B 探针（300/1000 tick，全语义链）上，"分片构建+组装+交换"替换
  D1-D6 安装链后的 ms/tick 净差与 t/s。判据：替换后 B 300 ≥ 500 t/s（战役门槛
  §4），且发布开销 ≤ 当前安装链开销（差分为正收益；若为负，按归因流程定位是
  分片构建还是组装/交换）。
- **J2 读路径延迟**：读者 `load()` 微基准（RwLock 版 vs ArcSwap 版 vs 现状
  `GameSession` 直接字段访问），含"写者每 tick 发布 1 次"背景负载下的 p50/p99。
  判据：读路径 p99 ≤ 微秒级（不劣化 server API 面感知），否则升级 ArcSwap 复测。

### 3.2 内存（慢读者保留曲线）

- **J3 保留内存曲线**：模拟外部慢读者持版本 N−k（k=1/10/100/1000），跑 1000 tick，
  记录峰值保留字节/版本 与 代数曲线；对比整包持有 vs 按分片收窄（§1 A 的 (ii)）。
  判据：每代保留量 ≈ 分片集合实际大小（无隐藏多代/无泄漏）；整包持有 1000 代的
  绝对值登记给用户拍板是否需要收窄策略（不预设阈值）。
- 附属：jemalloc/分配器每拍新分配次数定标（PublishedTick 外壳+分片），确认无
  分配风暴（方案 A 的固有代价，如实登记）。

### 3.3 一致性（违约计数必须为零）

- **J4 撕裂锤击测试**：并发读者循环 `load()` 后校验 (i) 版本内全部分片 `tick` 字段
  == `PublishedTick.tick` == `global.committed_tick`；(ii) 事件段 seq 连续且首 seq=
  上一版本尾 seq+1。引擎满速跑 1000 tick，违约计数必须 = 0（不是"很少"）。
- **J5 失败原子性**：注入 mid-tick 失败（validator 拒绝/看门狗），断言读者全程
  只见 N−1 版本、其携带哈希 == `report_log` 末条（失败 tick 零外泄）。
- **J6 等价锚**：发布版本驱动的查询面（snapshot/history_reads/活动委托/个人态）
  与现行会话镜像逐位等价（战役前置 §5.4 的查询面等价测试，作为 J4-J6 的行为面）。

指标总数：**3 个指标族（J1-J2 延迟、J3 内存、J4-J6 一致性），6 个判据项**。

## 4. 与现有代码的衔接面

| 现有结构 | 位置 | 复用方式 |
|---|---|---|
| `converge_round`/`TickEvidence` | `runtime/coordinator.rs:122`/`:65` | 分片句柄随报告上行：`GroupReport`/`DeskReport` 增 `shard: Arc<GroupShard>`/`Arc<DeskShard>` 字段（替代 D1 的 Echo 族独立上行轮）；`TickEvidence` 增 `group_shards`/`desk_shards` 字段——组装=收束时顺带 collect，零额外消息 |
| `TickEvidence.desk_views` | `runtime/coordinator.rs:81` | 现状 `view.clone()`（StockPublic 深拷贝，`protocol.rs:901`）改为 Arc 句柄后此 clone 消失——发布协议顺手消除一处每拍深拷贝 |
| `install_tick` 单点安装 | `runtime/world.rs:1615` | 追加 `publication.publish()` 唯一调用点；`public_views` 更新逻辑保留（下一拍广播输入）但源改为发布版本内分片 |
| `broadcast_start`/`Arc<TickSnapshot>` | `runtime/coordinator.rs:223`、`protocol.rs:216` | 既有 Arc-in-message 先例与模式参照；下一拍广播可直接引用发布版本内分片（`stocks: Arc<Vec<StockPublic>>` 聚合面可由 DeskShard 派生） |
| `GlobalState` | `runtime/coordinator.rs:20` | Copy 小结构，直接内嵌 `PublishedTick.global` |
| `report_log: Vec<TickRecord>` | `runtime/world.rs:312` | 审计/测试面保留；TickRecord 可持 `Arc<PublishedTick>` 弱引用或哈希族对照（J5 锚），避免事实双拷贝 |
| `World::spawn` | `runtime/world.rs:351` | 创建 `Arc<ViewPublication>`，句柄注入驱动器/会话——World 自身持写侧 |
| drive.rs Echo 消费段 | `runtime/drive.rs:532-732`、`world_committed_tick:1061` | 整段替换为"会话持发布句柄，`load()` 刷新"——D1-D6 删除的接入点；`account_dirty_prev`（:39）随 D2 消失 |
| `DayEndCandidate` | `runtime/world.rs:245` | 日结特殊版本复用同一发布槽（§2.3） |
| watchdog/Phase | `runtime/watchdog.rs` | 无新增等待相位——publish 是非阻塞交换，不进看门狗预算 |

分片类型新设：`DeskShard`（含 StockPublic+挂单观察面+fee/candle 段）、
`GroupShard`（账户投影+计划进度+个人态观察面）、`EventSegment`（连续 seq 前缀）——
零 String、全数值标识，与 engine 无字符串纪律一致。

## 5. 开放点（移交 ADR/实验裁定）

1. RwLock 槽 vs ArcSwap：J2 数据裁定；ArcSwap 走 ADR。
2. 分片粒度按 owner（本设计默认）vs 按查询面（纲要实验 3）——查询面清单盘点
   （snapshot.rs 消费面）先行，粒度影响的是分片内部布局不是协议形状。
3. `history_reads` 副作用路由（Q3）与发布版本只读性的交互：副作用不进版本、走命令
   面——本文件不展开，Q3 深化稿承接。
4. 保留策略是否需要硬上限：J3 曲线出来后带数据请用户拍板（不预设）。
