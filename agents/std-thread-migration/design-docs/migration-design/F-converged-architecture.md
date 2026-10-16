# F — 收敛架构：资源实体所有权转移模型（正式迁移统一架构）

日期：2026-10-15。作者：任务F subagent（只读分析，只写本文件）。
输入：原型 `oworker.rs`/`orun.rs`（实测形态，962 t/s@10 组 / 849 t/s@5 组，s5 负载）、
既有设计 A~D（本目录）、用户决策（2026-10-14/15，见 `memory/project_std_thread_migration.md`
等）。行号引用：`engine/` = spine-simplify-01 树，`proto/` = std-thread-proto 包。
诚实声明：工作区内未找到独立的"Sol 包审反馈"文件，本文以 A~D 与原型实测形态为收敛
依据；Sol 反馈若有补充点，落在本文的开放问题节再合并。
修订（2026-10-15）：按 Sol 终审最小修改集更新——多轮/AdminBatch 落账可逆化（§5/§6.2）、
Abort 恢复 foreign_active 起始值（§5.2）、desk 撤销日志覆盖面明确化（§3.2）、转正措辞统一为
"下一 tick 首批 Ops"（§3.3/§5.2）、DeskBatch 加 tick/round 防御字段（§1.3）、看门狗相位
感知（§4.2）。复核修订（同日，Sol diff 复核）：foreign_active 回滚不按被丢弃 Closed
递减、改逆序回放恢复起始值（§5.2）；§6.2 区分日结计算失败（Abort 冲回）与宿主写盘
失败（不可变候选已交出，显式报错保留旧档、不回滚市场）。

## 0. 定位与取代关系

本文是**唯一权威架构**，取代 A~D 的分歧部分；A~D 中与本文不冲突的盘点细节
（B 的字段分组、C 的 panic 运输协议、D 的 rayon 退出清单）继续有效并被本文引用。
分歧消解记录见 §8。

一句话架构：

```
协调者（单线程，tick 脊 + 全局域权威）
   │ Start(tick, phase, Arc<MarketSnapshot>, 玩家冻结, 行政实体)   ──下行──▶ G 个组线程
   │                                                            ──下行──▶ N 个交易部线程（只收 Ops/控制消息）
   ▲
   │ GroupStats / DeskDone / Failed / Final                     ──上行（单一 mpsc）──┐
   │                                                                            │
   │  数据面（订单/实体）不经协调者：                                              │
   └── 组线程 ══DeskIn 批次══▶ 目标股交易部；交易部 ══DeskBatch 实体══▶ 组线程 ═══════┘
```

- **G 个 NPC 组线程**：每组持有一片 NPC 账户（现金+跨股持仓+策略+注意力+计划+经历），
  全部为线程私有，零锁零共享。
- **N 个交易部线程**：一股一个，独占本股订单簿与本股在途实体（买单 purse、卖单余股）。
- **实体 move**：下单时现金/股票实体从组账户**实际扣出**随 `DeskIn::Place` 转移给交易部；
  成交时实体互换（买方得股票实体、卖方得现金净额实体）、终结/拒绝/到期时原路退回。
- 代码本体**零线程数管理概念**（无池、无准入、无宽度）；线程数=G+N+1，由世界构造参数
  （组数、股数）决定；CPU 限制走 OS 级（cgroup/taskset）。

原型证据（`own-inline-r5`，5 组 5 交易部，20007 NPC，3000 tick）：849 t/s，
decide 等待 1.017 ms/tick、match 等待 0.114 ms/tick、dispatch 0.012 ms/tick，
实体 26558 条/tick，消息 200+25 批/tick，现金/股份守恒、T+1、整手零违例；
10 组调优 962 t/s（用户确认口径）。纯机制税 ~8%（用户结论）。

## 1. 实体类型系统（Rust 类型设计）

设计原则：资源实体 = **可 move 的数值载体**，粒度按 memory `resource-entity-model`：
NPC 现金一个聚合额、每 (NPC,股票) 持仓一个聚合额，不做"一元一实体"。核心运行时零
String（`project_engine_string_free`）：身份全部数值化，字符串只留边界。

### 1.1 身份与基础量

```rust
/// 资源所有者身份：(组索引, 组内名册下标)。实体回流原样回带，组端零查找落账。
pub type Owner = (GroupIdx, LocalIdx);        // (u16, u32)，Copy
/// 股票数值索引（世界构造期建立 StockId(u16) ↔ StockCode 边界映射）。
pub type StockIdx = u16;
/// 挂单标识：交易部本地单调序号 + 股索引复合（全局唯一；替代 next_order_id 全局计数器，
/// 恢复时按各股已消耗序号重建）。存档/回执/Cancel 均用该复合标识。
pub type OrderRef = (StockIdx, u64);
/// 金额沿用 engine Money（i64 分，checked 运算显式上抛）。
```

### 1.2 组 → 交易部：操作与随单实体（`DeskIn`）

```rust
pub enum DeskIn {
    /// 下单。买单 `purse` = 现金实体（最坏情况对价 + 佣金 + 过户费上限，已从组账户
    /// 实际扣除，所有权随消息转移）；卖单 `purse`=0、qty 股已从持仓实际扣除。
    Place { owner: Owner, side: Side, price: RoutedPrice, qty: u32, purse: i64 },
    /// 对齐撤单：撤该 owner 在本交易部的全部挂单（实体原路回流）。
    CancelAll { owner: Owner },
    /// 定向撤单（引擎语义：部分成交撤余量、已全成回显式业务失败）。
    Cancel { owner: Owner, order: OrderRef },
}

/// 价格符号在交易部受理时解析（组端看不到活簿）：
/// Fixed(Money) | Highest | Lowest | Market（市价=涨跌停保护，未成交余量不进簿）。
pub enum RoutedPrice { Fixed(i64), Highest, Lowest, Market }
```

同一批次（及同一 channel 先后批次）FIFO：同 NPC 的 CancelAll/Cancel 恒先于其 Place
（对齐语义，`proto/oworker.rs:40-41`）。

### 1.3 交易部 → 组：资源实体（`Entity`）+ 事实流（`FillFact`）

原型五类实体保留，另加事实流承载引擎语义（计划续行/回执/事件需要价格与费用明细，
不能只有金额）：

```rust
pub enum Entity {
    CashProceeds  { local: LocalIdx, amount: i64 },  // 卖出成交净额（扣卖腿费用）
    CashRefund    { local: LocalIdx, amount: i64 },  // 买单终结 purse 余额 / 受理拒绝退款
    BoughtShares  { local: LocalIdx, qty: u32 },     // 买入成交（组端 T+1 冻结入账）
    RefundedShares{ local: LocalIdx, qty: u32 },     // 卖单终结余股（恢复可卖）
    Closed        { local: LocalIdx },               // 挂单终结标记（零额也发，闭合在途登记）
}

/// 成交事实（不做资源转移，只做信息回流）：驱动计划续行、retail/institutional
/// experience、receipt 聚合与 Event 流。
pub struct FillFact {
    pub order: OrderRef, pub owner: Owner, pub side: Side,
    pub price: i64, pub qty: u32,
    pub commission: i64, pub stamp_tax: i64, pub transfer_fee: i64, // 费用分项
}

/// 受理/撤单回执（业务失败显式可见，不静默）：带外价、非整手、超单笔上限、
/// 不可撤窗口撤单、"已成交无法撤单"等。
pub struct DeskReceipt { pub order: OrderRef, pub owner: Owner, pub kind: ReceiptKind }

/// tick/round 为冗余防御字段：组端核对批次属于当前 tick/轮，错配=显式失败（不静默落账）。
pub struct DeskBatch { pub stock: StockIdx, pub tick: u64, pub round: u32,
                        pub entities: Vec<Entity>,
                        pub fills: Vec<FillFact>, pub receipts: Vec<DeskReceipt> }
```

批次为聚合 Vec（每交易部每 tick 每组至多一个非空批次；原型实测 25 批/tick 上行），
可选对象池预分配（memory 建议），不逐条发消息。

### 1.4 组内账户（`Npc` 泛化的组端权威账户）

沿用 `proto/npc.rs:27-50` 形态并引擎化：主仓扁平字段 + `secondaries: Vec<XPos>` 跨股
次级仓 + `foreign_active: [u16; N]` 在途挂单登记。**严格所有权口径**：挂单在途期间
该笔资金/股票不在账户里（与现引擎 escrow 预留的差异见 §8 映射第 15 行）。
不变量（期末审计断言，原型已验）：`qty == sellable + reserved_shares + frozen_today`
逐仓成立；现金守恒 = Σ组账户 + Σ交易部 purse/fee_sink。

### 1.5 purse 口径

买单实体化金额 = 最坏情况对价 × 数量 + 佣金上限 + 过户费上限：
- Fixed 价：固定价计算；
- Highest/Lowest/Market 及符号价：按目标股涨停价（组端只有 band 表，`proto/oworker.rs:1432-1449`）。
实际成交较低时差额（slack）在挂单终结时以 `CashRefund` 退回（对齐引擎"实际报价较低时
释放差额；释放额不回补本 tick 预算"）。

## 2. NPC 组线程

### 2.1 分组策略

- 名册构造沿用现引擎（`seed_float`/`split_float` 全体 NPC 池 + 初始分配跨股持仓），
  拼接后按**连续区间均匀切分**成 G 组（`proto/orun.rs:160-167`）：组内混多个 home 股，
  组与股无亲和——订单按意图路由任意交易部，负载天然均衡。
- **组数 G 是世界构造参数**（随 setup 一起确定，运行期不变），不是运行时线程管理。
  缺省按负载推导（如 每组目标 2000~4000 账户，且 ≥ 股数），具体公式由基准定
  （memory `no_hardcoded_parallelism`：不写经验魔数；50~100 股/128 核场景另行标定）。
  原型数据：20007 NPC → 5 组 849 t/s、10 组 962 t/s。
- 账户 keyed 附属状态全部下沉组线程（B §1.2 清单）：attention 堆（每组自有）、
  belief/watchlist/价格记忆、retail_experience、history_reads、plans 链、
  npc_order_lifecycles。

### 2.2 组内决策流程（`GroupIn::Start` 处理）

1. **应用暂存实体**（staging，见 §5）：先于 Start 入队的上一 tick 实体批次落账
   （mpsc FIFO + 收束因果序保证，`proto/oworker.rs:16-19` 时序不变量）；Start 即
   上一 tick 的隐式转正，落账完成后丢弃组端可逆日志（§5）。
2. **日界解冻**：仅 DayEnd 相位做 T+1 解冻（`unfreeze_t1_all`），**不是**原型的每 tick
   解冻——原型无日结构属简化，正式版对齐"当日买入下一交易日日界解锁"
   （`docs/trading-rules.md`）。
3. **组装决策输入**：全市场 `Arc<MarketView>`（协调者广播，上一收束点口径）+ 逐 NPC
   `SelfView`（单持仓走 home 模板快路径零分配、多持仓逐 NPC 重建，`proto/oworker.rs:1347-1383`
   实测优化）。**视图=全市场**，保持 retail 全市场均匀选股与 inst/hot 全市场扫描
   （原型的 mask 视图是性能原型简化，不迁移；差异登记 §11）。
4. **到期性筛选**：attention/observation 概率在组内判定（读自有堆与广播 tick）。
5. **decide**：复用引擎 `decide_data`（纯函数，`seed^tick^account` 独立 RNG），含
   计划根/续行的账户侧评估（读 FillFact 暂存）。
6. **意图→操作+实体扣除**：对每个 intent，先对齐撤单（对该 owner 有在途的每个交易部
   发 `CancelAll`），再实体化扣出 purse/股票，装 `DeskIn` 进对应股的批缓冲
   （`proto/oworker.rs:1394-1512`）；资金/可卖不足=显式 skip 计数并丢弃该单
   （不静默、不部分执行）。
7. **分段 flush**：每 `chunk`（setup 参数，原型 512）个 NPC 把各股批缓冲发出一次
   （`final_flush=false`），决策与撮合流水线重叠；名册末尾发 `final_flush=true`
   空批作为本组本 tick 提交标记（`proto/oworker.rs:1317-1335`）。
8. 上报 `GroupStats`。

### 2.3 组线程消息面

```rust
pub enum GroupIn {
    Start(Arc<MarketSnapshot>),      // 快照含 tick/market_minute/全股公共态/PhasePolicy
    Continue(Arc<MarketSnapshot>),   // 同 tick 续行轮（§4.3），组先落账本轮实体再续行
                                     // （落账记逆操作进组端可逆日志，Abort 可冲回，§5）
    Batch(DeskBatch),                // 交易部实体批次（落 staging）
    Admin(AdminBatch),               // 协调者行政实体（公司域权益，§7.3）
    Abort(u64),                      // 撤销 working overlay，回到上一收敛态（§5）
    DayEnd(DayEndDirective),         // 日结相位（§7.2）
    Shutdown,
}
```

## 3. 交易部线程（一股一线程，独占撮合）

### 3.1 私有状态

`Desk`（`proto/oworker.rs:251-274` 引擎化）：双边 `BTreeMap<价, VecDeque<DeskOrder>>`
价格档 × FIFO、`index: HashMap<OrderRef,_>`、`by_owner: HashMap<Owner, Vec<OrderRef>>`、
到期队列、每组实体出口缓冲、fee_sink、逐股 `PrivateStockLedger`（envelope/守恒行，
现 `IncrementalContinuousStockShadow` 内逐股账本的常驻化，B §1.1）、行情产物
（last_price/最高最低/best bid/ask/失衡/minute close）。挂单携带资源实体：
买单 purse=剩余可花（含费用上限）、卖单 qty=剩余股数。

### 3.2 受理与撮合

- **到期撤销**先于本 tick 新意图（首批 Ops 到达时 `expire_due`，`proto/oworker.rs:851-860`，
  对齐现引擎 ExpiryShadow 相位时点）。
- **受理校验**（对齐 trading-rules 全表）：价 ∈ 涨跌停带；价笼子（连续竞价，参考价按
  对手一档→本方一档→最新成交/前收解析，`price_cage_enabled` 可关）；买入整手、单笔上限
  （主板/创业板差异化）；卖出零股一次性（组端已校验，交易部防御复核）；相位门禁
  （PreOpen 拒收、收盘竞价窗口拒撤）。拒绝= `reject_entities` 原路退回 + `DeskReceipt`。
- **撮合**：`match_incoming` 逐档吃对面最优，`settle_desk` 实体互换：卖方得
  `CashProceeds`（净额），买方 purse 扣实际 charge、得 `BoughtShares`；费用按引擎
  完整费则（佣金双边/最低佣金、印花税仅卖、过户费双边 0.01‰；卖腿费用从所得封顶、
  佣金→印花→过户费顺序，`engine/config.rs:97-100`），fee_sink 守恒桶累计。
- **同价先后**：价格档内 FIFO 队列 + 交易部单线程受理 ⇒ 跨 tick 老单先成交（满足
  用户默认偏好"组间 FIFO/组=1~N tick"，memory `same-price_winner_policy` 第 2 层）；
  同 tick 内跨组到达序任意（第 1 层已获准）。价格优先已按用户决策取消
  （memory `concurrent_matching_policy`），撮合按到达序吃档。
- **终结**：全成/撤/到期/受理拒绝 → `close_entities`（买单 slack 退款、卖单余股回流、
  `Closed` 恒发）。
- **working overlay（撤销日志）**：本 tick 内对交易部全部可变状态的每次变更记 COW 粒度
  撤销记录，覆盖面显式枚举：挂单簿（新增/移除/字段旧值）、到期队列、fee_sink 累计、
  行情产物（last_price/日内最高最低/累计成交量等收束统计）、订单号 next_id、
  book_next_sequences、逐股 PrivateStockLedger（envelope/守恒行）。跨续行轮累计，
  tick 收敛后按 §5 提交/回滚。

### 3.3 交易部消息面

```rust
pub enum DeskMsg {
    Ops { tick: u64, round: u32, group: GroupIdx, ops: Vec<DeskIn>, final_flush: bool },
    Commit(u64),   // tick 转正：丢弃撤销日志。隐式触发点=**下一 tick 首批 Ops**（Ops 携带
                    // tick 字段，tick 前进即上一 tick 转正；同 tick 续行轮 Ops 不触发）；
                    // 显式 Commit 消息保留便于对账
    Abort(u64),    // 回滚撤销日志、清实体出口缓冲
    Drain,         // 日终清簿：强撤全部挂单、实体回流、上交清簿后状态
    Shutdown,
}
```

收束计数：收到全部 G 个组的 `final_flush` 即本 tick（本轮）撮合输入完毕 → flush 实体
批次（**先于** `DeskDone` 上发，因果序是正确性基石）→ 上报
`DeskDone { TickDone 行情统计, fills, receipts, 守恒行, book_next_sequences }`。

## 4. 协调者与 tick 协议

### 4.1 正常时序（连续竞价 tick）

```
协调者（持有全局域权威：时钟/计数器/日历/公司域/玩家通道/事件 seq）
 ① 串行准备：tick+1 判定 PhasePolicy（含相位/日界）；冻结玩家 ingress 为玩家组输入；
    组装 Arc<MarketSnapshot>（全股上一收束点公共态：last_price/bid/ask/recent/
    minute/relative_volume/imbalance）
 ② 广播 Start 给 G 个组（每股/每组的下行 channel 逐个 send，无锁无惊群）
 ③ 组：落账 staging → 决策 → 实体扣除 → 分段 Ops 直发交易部 → GroupStats
 ④ 交易部：到期撤销 → 边收边撮合 → 收齐 G 个 final_flush → flush 实体批次 → DeskDone
 ⑤ 协调者收束（固定计数 recv：G+N）：
    全局域工作——receipt/envelope 投影、Event seq 统一分配与 EventFact 批组装、
    公共行情/蜡烛安装、计数器推进、PreCommitValidation（哈希与守恒核验）
 ⑥ 若存在续行需求（组上报"计划待续"）→ 广播 Continue 进入下一轮（§4.3）；否则 tick 完成
 ⑦ 成功 → 下一 tick 回 ①（Start 即隐式 commit，§5）；失败 → 广播 Abort（§5）
```

数据面（订单/实体）全程不经协调者——这是相对 C 设计（coordinator 组装逐股输入包并
路由续行 ops）的结构性简化，也是 962 t/s 的实测形态。

### 4.2 同步原语

全程 `std::sync::mpsc`（C §3 结论照搬）：组↔交易部全互联直发（G×N 下行 + N×G 上行
实体通道）、每股/每组各一条协调者下行、单一上行收束 channel。无 Barrier、无 Condvar、
无共享 Mutex 广播。防挂死：worker 收件循环带 `recv_timeout` 看门狗（原型 5s）+
全局 `abort: AtomicBool`，超时且无中止标记=显式失败（`proto/oworker.rs:827-843`）。
看门狗必须**相位感知**：DayEnd 公司域事务与持久化窗口期间 worker 长时空闲等待，
固定 5s 会误报——长相位按相位放宽阈值或由协调者心跳保活，超时上报携带相位上下文
（G 批计划风险清单）。

### 4.3 同 tick 多轮（计划续行）

现引擎计划链在收到命令结果后同 tick 续行（C §2.3）。本协议以轮次承载：
- 轮末组上报"本组有账户的计划已收到结果且产生续行 ops"；协调者收束后若任一组非空，
  广播 `Continue(round+1)`；组先落账本轮实体/FillFact 再跑续行决策（同 §2.2 流程，
  跳过普通 due 筛选）；交易部对 `Ops{round+1}` 继续撮合（簿含本轮未成交挂单）。
- 终止：一轮收束后全部组续行 ops 为空。轮数上界=计划链有限步（与现引擎 drive 循环
  同一前提；异常形状显式登记，不静默）。
- 整个多轮 tick 是一个原子单元（§5 的 overlay 跨轮累计；Continue 轮的组端落账同样
  记逆操作进可逆日志——第 k 轮失败时前 k-1 轮已落账实体一并冲回，不存在"半 tick"）。
- 若基准证明续行轮成本显著，"续行延迟 1 tick"是已登记的备选放松
  （memory `npc-grouping-relaxed_latency` ②，需用户按数据拍板），默认不启用。

## 5. 失败原子性：与现 candidate 模型的等价性论证

### 5.1 现状机制（对照）

现引擎每 tick `clone_for_tick_shadow` 整体克隆 candidate，全部相位在 candidate 上推进；
失败 = **消费式丢弃 candidate**（`TickShadowPlan::take_session`，pipeline/mod.rs:296-311），
权威从未被失败路径触碰；成功 = `CommitTick` 单点安装（32 字段 defer_swap）。
"单批 EventFact 只在全部 callback 验证后安装"（authoritative_tick.rs 门禁）。

### 5.2 新模型等价机制：working overlay + 单点裁决 + 消息驱动转正

权威分布在 worker，无法整体克隆，改用**三层 working overlay**，转正/回滚由协调者的
下一条消息唯一裁决：

| 层 | working 载体 | 转正（commit） | 回滚（abort） |
|---|---|---|---|
| 组账户 | 本 tick 实体扣除（已随 Ops 移出）+ 已发 DeskIn 日志（Copy，24B/条）+ staging 实体缓冲 + **组端可逆日志**（本 tick 内每条落账与每次 foreign_active 增减记逆操作；Continue 轮落账与 §6.2 AdminBatch 落账均在内） | 收到下一 `Start`（隐式：上一 tick 已成功），落账 staging 后丢弃全部本 tick 日志；`Continue` 只触发落账、**不是**转正点（落账进可逆日志） | 收到 `Abort`：①按已发 DeskIn 日志逆向恢复（买单退 purse、卖单还 qty/sellable）；②**foreign_active 恢复 tick 起始值**——本 tick 内登记的每次增减（下单 +1、闭合落账 −1）均在可逆日志内，Abort 逆序回放；**不得按被丢弃的 `Closed` 实体递减**（反例：起始遗留挂单本轮撤单后 Abort——desk 复原该挂单，登记须回到起始值而非 −1，staging 中被丢弃/出口缓冲被清空的 Closed 不构成递减依据）；③清 staging 并按落账逆操作日志冲回本 tick 全部已落账实体（含 Continue 轮与 AdminBatch） |
| 交易部簿 | 撤销日志（COW，覆盖面=§3.2 显式枚举：挂单/到期队列/fee_sink/行情产物/next_id/book_next_sequences/逐股账本）+ 实体出口缓冲 | 收到 `Commit` 或**下一 tick 首批 `Ops`**（tick 字段前进即隐式转正；同 tick 续行轮 Ops 不触发） | 收到 `Abort`：按日志逆向恢复簿（含 fee_sink/行情产物/计数器）、清出口缓冲 |
| 协调者全局域 | 私有 candidate（计数器/事件批/公共行情/投影）——本就是单线程内存态 | 安装（唯一安装点语义保持） | 丢弃（与现状逐语义相同） |

**多轮原子性统一（组端落账可逆化）**：Continue 轮与 DayEnd 的 AdminBatch 落账都发生在
tick 裁决之前，统一纳入组端可逆日志——每条落账对续行决策立即可见，但权威提升仍以下一
`Start` 为唯一转正点。备选方案"Continue 轮落账进 tick 级 overlay、下一 Start 才并账"
被放弃：组内续行决策需要本轮 FillFact/退款立即可读，双层缓冲徒增复杂度；逆操作日志与
已发 DeskIn 日志同构（Copy 小记录），单一机制覆盖单轮/多轮/日结全部路径。

**等价性论证**：
1. **权威零污染**：任一 worker 的已收敛状态（组账户、交易部簿）在 tick 裁决前只被
   overlay 触碰；overlay 的外溢（实体批次）要么停留在组 staging 未被提升，要么经
   可逆日志落账（Continue 轮/AdminBatch——每条记逆操作，Abort 完全冲回）。"已收敛"
   与"working"的边界从"是否落账"移到"是否转正"，裁决语义不变。
2. **单点裁决**：能否转正只由协调者在收齐 G+N 报告、完成 PreCommitValidation 后的
   下一次广播决定；没有任何 worker 能自主提升 overlay（组只在收到 `Start` 时提升
   staging——`Continue` 触发的落账在可逆日志内、**不是**提升；Start 只在成功后
   发出）。对应现状"commit 单点安装"。
3. **时序无竞态**：实体批次先于 DeskDone 发送、DeskDone 先于裁决、Start/Continue/Abort
   后于裁决（mpsc FIFO + 收束计数）。多轮下早轮实体在 Abort 前确已落账，但落账在
   可逆日志内、冲回无残留——"Abort 时 staging 已被应用"从竞态降级为被逆操作覆盖的
   受控中间态；"Commit 时实体未达"仍结构性不存在（原型时序不变量的推广，
   `proto/oworker.rs:16-19`）。
4. **失败后可继续**：Abort 后全体 worker 回到上一收敛态、继续存活等待下一 Start——
   与现状"丢弃 candidate 后宿主可再 step"等价。
5. **业务失败不中断 tick**：意图被拒/撤单失败/竞价拒绝等以 `DeskReceipt` + 事件流
   呈现（现引擎同语义，session.rs:258-260 注释），不触发 Abort。Abort 仅用于
   结构性失败（不变量破坏、守恒破坏、通道断连、PreCommit 失败）。

### 5.3 panic 协议（C §4 全盘保留）

- worker 单 tick 执行体包 `catch_unwind`：panic payload 经 `OwnUp::Failed/TaskDied`
  运输（对齐 `DecideMessage::TaskDied` 先例），worker 随即**回滚自身 overlay**、
  上交 `Final(已收敛态)`、退出——已收敛权威随 Final 存活，可重组/存档。
- 协调者收到 Failed/TaskDied/Disconnected：本 tick 判死 → 广播 Abort+Shutdown →
  收齐 Final（按存活数对账，缺口以 Disconnected 显式暴露）→ join 全部 →
  `resume_unwind` 原 payload。不吞、不改写、不跳股继续（守恒审计要求全股在场）。
- 锁中毒：目标态无共享读写锁（组/交易部全私有），此条自然退化；过渡期残留共享读
  的中毒必须显式报错。

## 6. 全局域（留协调者）的处理方案

### 6.1 归属清单（B §1.3/1.4 照搬）

- 单调身份与计数：tick/day/seq/next_receipt_base（`next_order_id` 退役，见 §1.1）；
  `revision` 原子直改计数（X1 新鲜度）。
- `civil_clock`（自然日双时钟）+ 日历 + `rng`（会话随机流，seal 采样）。
- 公司域：`company_registry/operations/closing/library`（Arc，日结 `Arc::make_mut`）、
  `groups/disclosures`。
- 玩家通道：`pending_player`、ingress 游标；X1 冻结语义在 Start 组装时执行。
- 投影与台账：`envelope_ledger`（由 DeskDone receipts 喂）、`retail_projection_seen`、
  verify_metrics 计量汇聚。
- 事件 seq 分配与 EventFact 批组装（安装点唯一性保持，§4.1 ⑤）。

### 6.2 日界/日结相位（全局事务，协调者单线程执行，worker 空闲等待）

```
DayEnd(tick, date):
 1) 广播 Drain → 交易部强撤全部挂单（日终清簿语义：收盘竞价余单随日界失效）、
    实体批次回流、上交清簿后簿态与守恒行 → DeskDrained
 2) 组：落账全部 staging（清空在途）→ T+1 日界解冻 → 计划/经历日翻转 → GroupDayDone
 3) 协调者跑公司域 civil 事务（operations/closing/披露）→ 产出账户 keyed 权益
    （现金股利、送转股、扣税）→ 打包 AdminBatch 下发组 → 组落账并回执
    GroupAck（权益入账后的投影随之带回）。**AdminBatch 落账纳入组端可逆日志**
    （推荐方案，§5，保持与现引擎等价）：DayEnd 相位内组端全部状态变更（Drain 回流
    落账、T+1 解冻、计划/经历日翻转、AdminBatch 权益）均记逆操作——**日结计算/
    候选投影失败**（公司域事务 panic、落账校验失败、收束对账缺口）时广播 Abort，
    组按逆操作日志整体冲回、交易部按撤销日志恢复清簿前簿态，全体回到上一交易日
    收敛态（日历不推进），与现引擎"日结 candidate 失败即丢弃、权威未被触碰"等价
 4) 协调者组装 SavedRuntimeState（组投影 + 交易部清簿态 + 全局域）并跑全部可失败
    校验（投影对账/守恒/哈希）——**此阶段仍在 Abort 窗口内**：第 3 步对账通过
    不足以关闭窗口，候选组装与校验自身可失败，失败仍广播 Abort 按可逆日志冲回；
    **候选组装及全部校验成功后才提交日结**（此刻窗口关闭、可逆日志退役），
    不可变候选随即交出、宿主写盘（ADR-0025 日终档边界不变；活动委托不进档——
    清簿后天然为空）。**宿主写盘失败 ≠ 市场回滚**：显式报错、保留上一有效档、
    不广播 Abort（内存态已日结，回滚反而制造"档-态"矛盾）；与现引擎"日结提交
    并交出不可变候选后，宿主写盘失败不触碰权威"同构
 5) 广播下一交易日 Start
```

恢复：重建组/交易部线程，`committed` 从恢复态初始化（B §3.3）。

### 6.3 玩家与只读查询

- 玩家账户与 NPC 账户同构，归属一个**玩家组**（专用组线程，或并 入首组；基准定）。
  玩家意图由协调者从 ingress 冻结后随 Start 下发该组，组端跳过策略 decide、直接走
  实体扣除与派发；撤单（Cancel by OrderRef）路由目标交易部；结果同实体/回执流回流。
- 查询：热路径（玩家组合、行情）走**收束点原子换装的只读投影槽**（组端/交易部端各
  维护 `Arc<Projection>`，收敛时 swap；非收束点读到上一收敛态——与"step 后可见"
  契约一致，对齐 B 的 MarketLease 思路）；存档与全量查询走日界/收束点同步收集。

## 7. A 股语义映射表（现引擎语义点 → 新模型承载）

| # | 现引擎语义点（trading-rules.md / 源码） | 新模型承载 | 备注 |
|---|---|---|---|
| 1 | 涨跌幅 10%/20% 按股显式配置 | 交易部受理校验（band 表随 shared 分发）；组端 purse 最坏情况按目标 band 上限 | 原型同构 |
| 2 | 最小价位 0.01 元 | Money 类型既有约束，全链共用 | 不变 |
| 3 | 价格笼子（±2% vs 十个最小价位取宽；可配置关） | 交易部受理时解析（活簿参考价在交易部：对手一档→本方一档→最新成交/前收）；仅连续竞价相位 | 需扩展 resolve_rp（原型只解析 Highest/Lowest/Market 与 band） |
| 4 | 买入整手 100、主板/创业板单笔上限差异 | 交易部受理校验，拒绝实体原路退回+回执 | 原型同构（qty%100 检查） |
| 5 | 零股须随一笔卖单一次性卖出 | 组端校验（持仓知识在本组）+ 交易部防御复核 | 新增于原型 |
| 6 | 卖出 ≤ 持仓−T+1−已挂卖 | 组端 sellable 字段维护（实体扣除时校验） | 原型同构 |
| 7 | T+1：当日买入不可卖，**日界**解锁 | BoughtShares 入 frozen_today；DayEnd 相位组端统一解冻 | **不采用**原型每 tick 解冻（原型简化） |
| 8 | 佣金双边+最低佣金、印花税卖 0.5‰、过户费双边 0.01‰；卖腿费用所得封顶、佣金→印花→过户顺序 | 交易部 settle 扩展为引擎费则；fee_sink 守恒桶日结上交协调者 | 原型缺过户费，需补 |
| 9 | 市价意图=涨跌停保护逐档即成、余量不进簿 | RoutedPrice::Market 在交易部解析 band 边界；未成交余量 close_entities 退回不挂簿 | 原型同构 |
| 10 | 开盘集合竞价 09:15–09:25（09:20 后不可撤）、09:25 一次撮合、余单转连续 | PhasePolicy 随广播；拍卖窗口交易部只积累不撮合；边界轮统一撮合（统一交易簿撮合模式 #53 复用，含沪同价取中间价/深距离规则的既有简化）；不可撤窗口 Cancel 显式拒收回执 | 交易部新增拍卖模式相位 |
| 11 | PreOpen 09:25–09:30 不接受申报 | 相位门禁（组端不产单/交易部拒收+回执） | 新增 |
| 12 | 收盘集合竞价 14:57–15:00 不可撤、一次撮合、clearing_price 事件显式携带 | 同 10；DeskDone 携带 ClosingAuction 事件与 clearing_price，协调者收束点入事件批 | 新增 |
| 13 | 撤单语义：部分成交撤余量、已全成显式反馈 | DeskIn::Cancel(OrderRef)；结果 DeskReceipt（含"已成交无法撤单"） | 原型只有 CancelAll，需补 |
| 14 | 本 tick 撤单释放的资金/可卖下一 tick 才可用 | 严格所有权天然等价：退款实体 tick N 收束 flush、Start(N+1) 前落账 | 原型同构，游戏简化语义保持 |
| 15 | SelfView.cash：**可撤换**工作单冻结额同一轮可复用（原子撤换） | 严格所有权下单轮不可复用（退款未到账），skip 计数显式暴露；下一轮可用 | **语义差异，登记 trading-rules.md**；用户已确认原型形态（group_skipped=0@s5）；若实际负载 skip 率非平凡，备选=实体流携带 purse 余额回执 |
| 16 | reserved_cash/reserved_sell_qty 公开占用预算 | 被"实体在途"取代：账户余额即严格余额；在途登记 foreign_active 可作诊断公开；UI 契约随破坏性修改更新 | 无版本概念 |
| 17 | retail 全市场均匀选股、inst/hot 全市场扫描 | 广播全市场 Arc<MarketView>（上一收束点） | **不采用**原型 mask 视图；962 数字含 mask 简化，全视图成本需基准复核 |
| 18 | NPC 请求上一 tick 已提交态形成、下一 tick 受理 | 组决策读上一收束快照+已落账账户 | 同构 |
| 19 | 计划链：根命令、结果后续行（同 tick 多轮） | 账户 keyed plans 下沉组；续行由 FillFact 驱动；同 tick 轮次协议 §4.3 | 轮数成本待基准；1-tick 放松为登记备选 |
| 20 | 受理先后=入口实际先后；来源标签非优先级；同账户资源分别校验 | 组→交易部 channel 到达序；同组同 NPC Cancel 先于 Place（FIFO）；跨组到达序任意（不要求确定性） | 符合已获准的并发撮合政策 |
| 21 | 同价老单先后：跨 tick 大致 FIFO（默认偏好） | 价格档 VecDeque FIFO + 单线程受理 ⇒ 老 tick 单先成交 | 满足第 2 层偏好，无需额外机制 |
| 22 | 现金/股份守恒 + 逐股 envelope 守恒行 | 交易部 fee_sink + PrivateStockLedger 常驻 + 期末/日结审计（原型审计器引擎化） | 原型已验（0 违例） |
| 23 | 事件流：seq 单调、单批验证后安装 | DeskDone 携带逐股事件；协调者收束点统一分配 seq、组装、PreCommit 后一次安装 | 安装点唯一性保持 |
| 24 | 回执聚合→按账户结算相位 | 实体批次即结算载体（成交所得/退款直接落组账户）；receipts 上行只做投影/事件；settlement.rs 并行结算相位退役 | 结构性替代 |
| 25 | 公司域：分红/送转/公告/扣税（含送转个税、持有期） | 协调者日结事务产出权益 → AdminBatch 实体下发组落账（入组端可逆日志，§6.2） | 新通道，复用实体落账路径 |
| 26 | 自然日双时钟/日历/日结档 | 协调者独占 + §6.2 DayEnd 相位 | |
| 27 | 玩家 ingress：验收入队下一 tick 参与 | §6.3 玩家组 | |
| 28 | 内存验证投影 book_next_sequences / 挂单 seq | 交易部上报（OrderRef 复合序号即序号事实） | |
| 29 | 决策不偷信息、按输入顺序稳定输出 | decide_data 纯函数 + 组内单线程顺序调用 | 语义保持（线程位置不改变等价性，B §2.3 论证） |
| 30 | poison/StepFatal 显式失败 | §5.3 panic 运输 + Abort + Final(已收敛态) 上交 + resume_unwind | |

## 8. 与 A~D 的分歧消解记录

| 分歧 | A/B/C 立场 | 收敛结论 |
|---|---|---|
| NPC 账户归属 | A：(a) 按股名册（三处行为简化）vs (c) 分片+3 相位路由；B：账户留协调者+计算亲和下发；C：决策随主股派发+外股 ops 通道 | **全部废弃**：组线程**拥有**账户，实体 move 到交易部。A 的两难被消解——零协调数据面 **且** 完整跨股语义（全市场视图+跨股持仓）同时成立，这正是 962 t/s 实测形态 |
| escrow vs 所有权 | A(c)/现引擎：预留+释放；C §2.6：v1 保持 Escrow、簿每 tick 往返 | **所有权转移**（用户 2026-10-15 确认）：C 的"持久的是线程不是所有权"表述作废；簿/账户均常驻 worker |
| coordinator 数据面 | C：coordinator 组装逐股输入包、续行 ops 经 coordinator 路由 | **数据面直发**（组→交易部），coordinator 只广播快照/控制与收束报告；C 的 TickRound/StockReport 通道拓扑被 OwnShared/GroupIn/DeskMsg 拓扑取代 |
| 失败原子性 | B：股域 working/committed 双份+epoch 转正、账户域留单线程 candidate | **三层 overlay + 单点裁决**（§5）：不做整域克隆（B 方案的每 tick clone 正是被否决的往返形态）；B 的 lease/只读投影/存档重建/毒化传播结论保留 |
| 同 tick 续行 | C：多轮协议（coordinator 路由） | 保留多轮语义但换通道：组直发+Continue 广播（§4.3） |
| 决策视界 | B：密封 DecisionAccountInput 广播 | 组自有账户 ⇒ 无需密封下发；唯一共享输入=Arc<MarketSnapshot>（B §2.3 的"快照成本"风险随之消解大半） |
| 撮合账户回写 | A(c)：成交回写 home 相位 | 实体批次回流（数据面），无回写相位 |

保留不变：B 的全局域归属清单与 WASM 串行退化；C 的 mpsc 选型、panic 运输协议、
活性重述方向；D 的 rayon 退出清单与 6 批增量计划（批 3 的"模型主体"按本架构实施）。

## 9. 活性契约（std-thread 重述，替代 liveness-contract v4 的 rayon 术语）

1. **任务独立**：worker 的每轮执行只依赖下行消息 + 本线程私有状态，不等待任何其他
   worker 的信号（组等 Start/Continue/Batch、交易部等 Ops——全是收件驱动）。
2. **有限完成**：每轮工作量有限（名册/ops 有限）；多轮续行由计划链有限步保证，
   异常无界形状显式登记为不支持。
3. **协调者阻塞安全**：协调者只在上行 mpsc `recv` 固定计数（G+N），不占任何 worker
   的执行路径；不存在池内嵌套等待（死锁面结构性消失）。
4. **等待目标已无条件入队**：被等待的报告在协调者进入等待前已由广播触发（send 完成
   即入队），不存在"目标入不了队"的饱和形态。
5. **逃逸路径**：recv_timeout 看门狗 + abort 标记 + Disconnected 显式失败，任何一侧
   挂死都在有限时间内变成显式错误（不弱于现版契约的实质）。

## 10. WASM、测试与迁移批次

- **WASM**：无 std::thread ⇒ 退化串行驱动：组/交易部的消息处理体实现为纯 step 函数
  （对消息&状态），串行 driver 按"全部组步 → 全部交易部步 → 收束"顺序直接调用，
  不建线程不建 channel（B §2.3(4)、D §3）。wasm 保持单线程顺序合同，登记为已知限制。
  step 函数化同时服务于单测与确定性测试路径。
- **测试面**（TDD 顺序）：①实体协议往返守恒（红：purse 不足/实体丢失/Closed 缺发）；
  ②Abort 回滚等价（tick 前后状态快照哈希一致）；③多轮续行终止；④拍卖/PreOpen/收盘
  相位门禁与事件；⑤费用完整口径 oracle；⑥玩家组路径；⑦DayEnd 权益落账+存档往返；
  ⑧panic 运输与 Final 重组。既有 spine 契约测试（spine_v0_contract、t4、
  order_single_lifecycle 等）按新拓扑重述（D 批 5）。
- **迁移批次**：沿用 D 的 6 批骨架，批 3"模型主体"= 本文 §1~§7 的实施
  （protocol/group/desk/coordinator 四文件族 + compute.rs/stock_stream 协调层退役），
  其余批次不变。文件面预估：新增 ~6-8 个生产文件（组/交易部/协议/协调者/审计/
  串行 driver），改造与退役面按 B §4 / C §5 / D §4 合计约 40 生产文件 + 30 测试文件。

## 11. 登记项与开放问题（实施前须闭环）

1. **trading-rules.md 登记三处**：①对齐撤换单轮资金复用取消（§7 第 15 行，用户已按
   原型形态确认）；②reserved 公开口径改为"实体在途"（第 16 行）；③组间到达序任意
   （第 20 行，已有政策援引）。
2. **全市场视图成本复核**：962 t/s 含 mask 视图简化；全 MarketView 下 retail/inst/hot
   读面变大，需基准重测并登记差值（预期 5~50 股量级影响小，未验证不宣称）。
3. **组数缺省推导公式**：按账户数/股数推导的公式需基准标定后写入 setup 契约
   （不做经验魔数）。
4. **玩家组落位**（专用组 vs 并入首组）与 OrderRef 复合序号 vs 号段分配：实现期定，
   须保持存档/回执/Cancel 契约一致。
5. **续行轮成本**：s5 负载计划占比低，多轮轮次的空转开销未测；超标时按 §4.3 备选
   提请用户拍板。
6. **skip 率监控**：严格所有权下组端 skip 计数必须进诊断面板（资金不足被跳过的单量），
   非 0 时优先排查对齐退款时序而非加锁。
7. Sol 包审反馈未见独立工件；终审修改集已于 2026-10-15 经任务书到达并按最小修订并入
   本文：多轮/AdminBatch 落账可逆化（§5/§6.2）、Abort 恢复 foreign_active 起始值
   （§5.2）、desk 撤销日志覆盖面明确化（§3.2）、转正措辞统一"下一 tick 首批 Ops"
   （§3.3/§5.2）、DeskBatch 加 tick/round 防御字段（§1.3）、看门狗相位感知（§4.2）、
   DayEnd 计算失败与宿主写盘失败分治（§6.2）。
