# 批 10 文档同步包（diff-ready，批 10 直接落库用）

> **状态更新（2026-10-18，Sol 终审 §134）**：批 10 已由 `7209fdfd`
> （docs(std-thread): 批 10 文档同步——ADR-0043 落库与迁移差异登记）
> **实际执行**——A6–A10 及 B4 已入正式文档、A12 已进
> architecture/ADR-0043。本文件转为**历史编制包**（不再是待做清单）；
> 下方文本与计数保留编制时原貌，落库差异以 7209fdfd 的 diff 为准。
>
> 2026-10-17 编制。输入：`agents/std-thread-migration/plans/intentional-divergences.md`
> （批 10 待同步 9 条：A6/A7/A8/A9/A10/A11/A12/A13/B4，其中直接受领 trading-rules 6 条）、
> `agents/std-thread-migration/plans/adr-draft-std-thread-runtime.md`、
> `docs/trading-rules.md` 与 `docs/architecture.md` 现状（HEAD=5c1f4cd7）。
> 只读编制，未动任何入库文件。所有代码名词已对照源码核验（锚点见各条）。

## 0. 落库前核对结论

- **ADR 编号 0043 未被占用，确认可用。** 当前分支 `docs/decisions/` 最高为 0042
  （0037–0040 不在本分支历史）；main 已于 659cf978（2026-10-06）登记 ADR-0037~0040。
  两侧视角下 0043 均为下一空号。文件名：`0043-std-thread-ownership-runtime.md`。
- 行号基准：`docs/trading-rules.md`（189 行版）与 `docs/architecture.md`（194 行版），
  均为 HEAD=5c1f4cd7。落库时以**锚点文本**为准复核，行号仅供定位。
- 同步计数（对齐总账 §四）：trading-rules 直接受领 6 条（A6/A7/A8/A9/A10/B4）+
  可选 A1 指针 1 条（不计入 9）；ADR 0043 落库 1 件；architecture.md 更新 8 处。
  A11（留台账）、A12（ADR-0005/0027 补记候选，非本包范围）、A13（并入迁移收官记录，
  非本包范围）按总账裁定为低优先另办，不在本包强行夹带。
- A9 依据已于 2026-10-17 网检复核：沪市交易规则（2026 年修订）行情揭示条款——
  集合竞价期间即时行情揭示虚拟参考价格/虚拟匹配量/虚拟未匹配量，不揭示连续竞价式
  买卖盘口；深市同构定义。来源：
  <https://www.sse.com.cn/lawandrules/sselawsrules2025/trade/universal/c/c_20260424_10816492.shtml>。

---

## 一、trading-rules.md 插入文本

> 每条格式与现有条目同构（粗体标题＋登记日期＋依据/口径声明＋适用日期）。
> 「已实现」列表内新条目一律以 `- ` 列表项插入，缩进与现有条目一致。

### 1.1 A1（可选，不计入待同步数）：ADR-0042 指针行

**插入位置**：「## 已实现」列表内，「日终未成交单失效」条目（当前 L35-40，止于
"适用日期：2026-10-15 起（std-thread 翻转批）。"）之后。

**插入文本**：

```markdown
- **成交事件 maker/taker 字段=腿方向（2026-10-16 登记）**：`Event::Trade` 的
  `maker` 恒为卖腿、`taker` 恒为买腿（`maker/taker_order_id`、
  `maker/taker_filled_value_before` 三对字段成组同义）。字段名不承诺"主动/被动"：
  A 股无 maker/taker 费差（佣金双边、印花税恒卖腿、过户费双边），腿方向无歧义且
  费用归户唯一。完整裁定与前端消费面全量扫描见
  [ADR-0042](decisions/0042-trade-maker-taker-leg-semantics.md)。适用日期：2026-10-16 起。
```

### 1.2 A9：集合竞价公开盘口不揭示

**插入位置**：「## 已实现」列表内，收盘集合竞价条目（当前 L53-55，止于
"…不得误作开盘价或投影到开盘分时区域。"）之后。

**插入文本**：

```markdown
- **集合竞价时段不揭示买卖盘口（2026-10-17 登记）**：开盘/收盘集合竞价期间，公开
  行情面不揭示订单簿买卖盘（盘口）。**依据**：沪市交易规则（2026 年修订）行情揭示
  条款——集合竞价期间即时行情揭示前收盘价与集合竞价虚拟参考价格、虚拟匹配量、
  虚拟未匹配量，不揭示连续竞价式买卖盘口；深市为同构定义。本次核对日期
  2026-10-17，仅核对本条款，沿用已登记的其他条款核对边界
  （[上交所交易规则行情揭示条款](https://www.sse.com.cn/lawandrules/sselawsrules2025/trade/universal/c/c_20260424_10816492.shtml)）。
  **引擎实现（runtime desk 广播门禁）**：开盘集合竞价（CallAuction）公开簿=空；
  收盘集合竞价（ClosingAuction）只揭示 `public_cutoff_serial` 截点前受理的老单
  （收盘竞价时段内新申报不揭示，同条款真实制度面）；盘前（PreOpen）/连续竞价
  （Continuous）全公开。**尚未模拟（如实标注）**：真实制度的集合竞价虚拟参考价格/
  虚拟匹配量揭示本项目未实现——竞价期间公开面为空，不以其他量冒充虚拟参考价。
  适用日期：2026-10-17 起（std-thread 迁移批）。
```

代码锚点：`packages/engine/src/runtime/desk.rs:328`（`public_cutoff_serial` 注释）、
`desk.rs:1836-1848`（公开簿门禁）、`session.rs:5567`（镜像字段）。

### 1.3 A10：bound_fallback ±10% 近似声明

**插入位置**：「## 已实现」列表内，涨跌幅条目（当前 L41）之后。

**插入文本**：

```markdown
- **已知近似（std-thread 迁移期）：** runtime 镜像形态下 `StockView` 的
  `daily_upper_limit`/`max_buy_price`/`min_sell_price` 暂无 desk 精确源（desk 广播的
  `StockPublic` 携带精确值），`session/views.rs` 的 `bound_fallback` 以最近价 ±10%
  边界回退近似（`last×11/10` 上取、`last×9/10` 下取，缺最近价时按 0.01 元兜底），
  待投影收口后替换为精确值（2026-10-17 登记）。该近似只影响展示与下单界面预检查的
  边界参考值，不影响撮合、拒单或存档事实——订单簿照常按真实涨跌幅与笼子校验。
```

代码锚点：`packages/engine/src/session/views.rs:114/123/131/379-388`。

### 1.4 A6：frozen_cash=0 与实体在途口径

**插入位置**：「## 已实现」列表内，资源占用条目（当前 L67-72，止于
"…完整口径见 [ADR-0017 分歧 #9]…"）之后。

**插入文本**：

```markdown
- **挂单资源占用的实体在途口径与 `frozen_cash=0`（std-thread 迁移，2026-10-17
  登记）**：runtime 所有权转移模型（[ADR-0043](decisions/0043-std-thread-ownership-runtime.md)）
  下，买单下单即从组账户实际扣出 purse（最坏情况对价+佣金上限+过户费上限）、卖单
  实扣 qty 股；挂单在途期间该笔资金/股票**不在**组账户——占用以实体在途呈现，
  快照 `frozen_cash` 恒为 0 是口径事实，不表示资源未被占用。三层现金语义：组账户
  可分配预算（已扣在途）/总现金观察值/desk 实际占用（价优差额退回后）。旧管线为
  escrow/envelope 预留口径（快照公开 `reserved_cash` 与按股 `reserved_sell_qty`）。
  **口径声明（如实登记）**：两种口径的资源总量等价——对在途挂单再计冻结即重复扣
  （迁移复审裁定）；「本 tick 撤单释放的资源下一 tick 才可复用」的既有游戏简化在
  两种口径下一致。适用日期：2026-10-17 起（std-thread 迁移批）。
```

### 1.5 B4：跨拍透支重验消除（一句，独立条目便于计数与退役审计）

**插入位置**：紧随 1.4 A6 条目之后（同列表）。

**插入文本**：

```markdown
- **跨拍透支重验的消除（2026-10-17 登记，供旧管线退役审计）**：旧管线 NPC 准备域
  需「队列批在下一拍重验现金」防御挂单后现金变动导致的拍中透支；runtime purse
  所有权模型下单即扣，该缺陷形态从根上消除（等价防御=组端 purse 校验）。旧管线
  防御面随其退役消亡，不再回补。
```

### 1.6 A7：计划复审触发=due 拍子集

**插入位置**：「## NPC 策略选择」节末，机构经历段落（当前止于"…公式与数值见
[ADR-0026](decisions/0026-individual-institution-experience.md)。"）之后，作为新段落。

**插入文本**：

```markdown
**计划复审与报价的触发拍（std-thread 迁移差异，2026-10-17 登记）：** runtime 下
计划链的复审与报价只在链 **due 拍**的 `run_chain_decision` 内执行；旧管线每拍复审。
复审语义本身（`record_review`/quiet 门控、观察与恢复门槛、首子单
urgency=initial+pause=Unavailable）已对齐迁移；差异仅在**触发拍集合**为旧管线的
due 子集——决策频率结构不同，属组线程「一股一持久线程+内联决策」调度模型固有，
非交易所制度面（A 股无「计划复审频率」对应规则，不构成交易语义偏差）。行为差异
如实登记；回归面由 runtime 专项测试承载。适用日期：2026-10-17 起。
```

代码锚点：`packages/engine/src/runtime/group.rs:2023`（`run_chain_decision`）。

### 1.7 A8：civil 日历推进编排（存档节注记）

**插入位置**：存档边界段（当前 L143-149）句末——
"公开保存和回滚 checkpoint 的边界见
[ADR-0025](decisions/0025-day-end-only-persistence.md)。"之后同段追加。

**插入文本**（接排在该句之后）：

```markdown
std-thread runtime 的日结编排（2026-10-17 登记）：市场推进内 `run_ticks` 按
`(tick-1) % ticks_per_day` 自动触发日界（world 权威），`driver.day_end` 仅承担 civil
侧桥接；`last_day_end_tick` 守卫显式互斥——重复手动日结返回明确错误、已被自动日结
覆盖的请求显式跳过、Abort 回滚清空当日进度，`World` Drop 兜底关停线程。恢复契约
不变：ADR-0025 日界静点，`WorldSpec.initial_committed_tick` 与 `session.tick()`
锁步对齐贯通相位（ADR-0043）。
```

代码锚点：`packages/engine/src/runtime/world.rs:323/1266`（`last_day_end_tick` 守卫）。

### 1.8 落库核对清单（trading-rules 部分）

- [ ] A1 指针（可选）：ADR-0042 链接相对路径正确（`decisions/0042-…`）
- [ ] A9：依据链接可访问性已在 2026-10-17 核验；「尚未模拟」声明保留
- [ ] A6：与既有「本 tick 撤单释放…下一 tick」简化条目（L51-52）无矛盾（口径一致声明已写入）
- [ ] A8：追加后段落仍为单一段落，不破坏 ADR-0025 链接
- [ ] 全部条目中文＋代码名词英文与源码一致（`frozen_cash`/`purse`/`public_cutoff_serial`/`bound_fallback`/`run_chain_decision`/`last_day_end_tick`）

---

## 二、ADR 0043 最终格式化版

**落库路径**：`docs/decisions/0043-std-thread-ownership-runtime.md`（0043 未占用，
见 §0 核对结论；编号体例对齐 0017/0042：`ADR-00XX: 标题`＋双语小节头＋双语元数据行）。

**完整文件内容**：

```markdown
# ADR-0043: std::thread 所有权转移运行时（NPC 组线程 + 每股交易部线程）

- **状态 (Status):** draft（实现已落地、缺省未翻转；批 11 全量回归 + s5 正式基准 +
  独立复核通过后改 accepted，并如实更新「后果」第 6 条）
- **日期 (Date):** 2026-10-17（决策链 2026-10-11 ~ 2026-10-16）
- **决策者 (Deciders):** 用户拍板（两阶段模型判定 2026-10-11；std::thread 内联方案
  2026-10-14；资源实体所有权转移模型 2026-10-15）；实现由 std-thread 迁移批 0~9
  各任 agent 执行

## 上下文 (Context)

名词与代码对齐锚点：`packages/engine/src/runtime/`（mod/protocol/entities/facts/
group/desk/coordinator/watchdog/world/drive/bridge/event_bridge/decision/fees/plans）。

### 1. rayon 宽度税与串行脊（性能主线实测，16 核 server 路径 s5 负载）

- 优化后仅 ~220x~310x；多核不升反降：128w 档 **−43%~−63%**、每 tick CPU 12~21×；
  16w 仅用 42% CPU（串行脊限死真实并行度）。
- worker 曲线倒 U：4w 293x vs 16w 213x——池宽与真实可并行负载错配（**宽度税**），
  调参类方案（默认宽度/粒度治理/分级池）无法救。
- 根因画像：贵活在串行脊柱（克隆/编排/验证），并行道只剩便宜活；jemalloc 释放
  ~52% + 容器析构 ~20% 的「构建→拆毁→释放」churn。
- 用户判定（2026-10-11，两阶段循环模型）：tick 核心只有两件事——**NPC 决策（并行）
  →交易簿撮合（并行）**，顶多 commit 前对 view 做一次同步；多核倒退是**并行模型
  不对**的结果，不是参数问题。据此否决全部「现有模型内调参数」方案。

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
（NPC 分组线程 + 实体 move），即收敛架构 F。

## 决策 (Decision)

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
   逆序回放全部日志回到上一收敛态。等价于旧管线「整体 clone candidate、失败消费式
   丢弃、成功单点安装」。
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

## 备选方案 (Alternatives Considered)

1. **B 桥接（session 分区桥接）**：账户留协调者+计算亲和下发、股域 working/
   committed 双份+epoch 转正（迁移设计 B）。否决理由：每 tick clone 双份正是一个
   被否决的往返形态；权威分布后整体克隆无对象。其全局域归属清单、lease 只读投影、
   存档重建、毒化传播结论被本模型吸收保留（F §8）。
2. **池宽封顶**：rayon 128 线程池封顶止血。用户否决——128=真实 128 线程池不许缩；
   宽度税治理止步 A+B，不做封顶。
3. **rayon 补丁**：在 rayon 调度器上打补丁修宽度税/嵌套池边界。用户否决——直接换
   更简单的 std::thread，不再投资 rayon（architecture.md 旧文记录的
   `in_place_scope` 嵌套池已知边界随 rayon 退役整体消失）。
4. **原型按股私有臂**：跨股语义损失不可接受——A 股语义要求 retail 全市场均匀选股、
   inst/hot 全市场扫描、跨股持仓常态。
5. **原型跨股中转臂**：escrow 预留+席位镜像+三相位 coordinator 中转：x30 实测
   ~484 t/s，且共享回执记账重新引入协调点。
6. **NPC 分组+放松延迟**（决策→执行延迟 1~2 tick）：登记为备选，默认不启用；
   仅当续行轮/退款时序基准证明必要时按数据提请用户拍板。

## 后果 (Consequences)

**正面：**

- 机制口径 962 t/s（10 组调优，s5 负载），约为 rayon 形态 359 t/s 的 2.7 倍；
  纯机制税 ~8%（批量消息+流水线重叠）。
- 零锁零共享：并发正确性由消息协议（FIFO+收束计数）承载，死锁面结构性消失；
  活性契约 std-thread 重述（任务独立/有限完成/协调者阻塞安全/等待目标无条件入队/
  看门狗逃逸——相位感知 `recv_timeout`，`watchdog.rs` `Phase`）逐前提映射不弱于
  rayon 版。
- 代码本体无线程数概念：部署弹性全部交给 OS（cgroup/taskset），引擎不再携带
  worker/池宽/环境旋钮及其全部测试与台账负担。
- 等价性门禁：A 股语义映射表 30 项（F §7）逐项落地；Abort 回滚等价（tick 起终
  快照哈希一致）、现金/股份守恒、T+1、整手、费用完整口径均有测试锚定。

**负面/限制（如实登记）：**

1. **WASM 串行回退**：wasm 宿主无 OS 线程，`step_runtime_enabled` 恒 `false`
   走串行管线（`runtime/mod.rs`，Web 优先产品语义；已知限制，不做 wasm 线程化）。
2. **线程数随规模线性增长**（G+N，协调者复用调用线程）：测试进程线程数上升，
   `World` Drop 显式 Shutdown 防泄漏；50~100 股 / 128 核场景的组数缺省推导公式
   待基准标定后写入 setup 契约（不写经验魔数）。
3. **严格所有权的单轮资金不可复用**：本 tick 撤单释放的资金/可卖下一 tick 才可用
   （退款实体次拍落账）；组端 skip 计数显式暴露资金不足跳过，不静默。语义差登记
   `trading-rules.md`（对齐撤换单轮复用取消、reserved 口径改「实体在途」）。
4. **上行负载大于原型基准口径**：正式引擎 `FillFact`/`DeskReceipt` 逐条上行
   （原型仅 Entity 聚合批）——962 t/s 只能作机制口径参考，正式基准批 11 重标。
5. **同拍交付双写**引入投影与组权威两条写路径：由「下一拍整值回写」保证不双计，
   测试面已锚定（同拍全量投影/Abort 零泄漏）。
6. **翻转现状**：`step_runtime_enabled()` 缺省 `0`=旧管线；`ENGINE_STEP_RUNTIME=1`
   显式启用 runtime。终态=全局域迁移完成、缺省翻转、旧管线与开关退役
   ——本 ADR 改 accepted 时按当时状态如实更新本条。
7. 迁移期双轨（翻转前）旧管线测试与 runtime 测试并存；旧管线退役批按「死亡测试
   台账」逐条登记机制替代，不静默删。

## 关联 (Related)

- 收敛架构全文与活性契约 std-thread 重述（§9）：
  `agents/std-thread-migration/design-docs/migration-design/F-converged-architecture.md`
- 批次计划：`agents/std-thread-migration/design-docs/migration-design/G-batch-plan.md`
- 选型原型：`packages/std-thread-proto/`（按股私有/跨股中转/所有权转移三臂，
  批 11 验收后退役）
- 有意分歧总账（含批 10 文档同步清单）：
  `agents/std-thread-migration/plans/intentional-divergences.md`
- 相关 ADR：0017（escrow tick 与验证边界——语义等价基线）、0025（日终档边界）、
  0041（StockId 数值身份）、0042（maker/taker 腿方向语义）
- 后续文档：liveness 契约 std-thread 版正式化、`trading-rules.md` 批 10 六条登记
  （frozen_cash 实体在途/复审 due 拍/civil 编排/竞价公开面/bound_fallback/
  透支重验消除）、`architecture.md` 八处更新（批 10 同步执行）
```

**与草案的差异说明**（批 10 落库时按本节核对）：

1. 标题与元数据改为仓库体例（`# ADR-0043: …`＋双语字段），对齐 0017/0042。
2. 「背景」更名「上下文 (Context)」、「被否方案」更名「备选方案
   (Alternatives Considered)」，新增「关联 (Related)」节——均为现有 ADR 通行结构。
3. runtime 模块锚点清单按实际文件补全（bridge/event_bridge/decision/fees/plans；
   原草案漏列）。原型包三臂文件名已核对存在。
4. 删除草案附录（architecture.md 更新清单）——工作清单不入正式 ADR，改在「关联」
   指向 intentional-divergences.md；更新点全文移至本包第三节。
5. 「后果」第 6 条措辞微调（「批 9 C4」是内部批次号，正式 ADR 不携带）；
   「关联」中 `.tmp/` 路径全部改为 `agents/std-thread-migration/` 归档路径
   （.tmp 不入库）。性能数字、决策条目 1~11、被否方案 1~6 与草案逐字保持。

---

## 三、architecture.md 八处更新（旧文→新文对照）

> 行号基于 194 行版（HEAD=5c1f4cd7）；落库以锚点文本复核。均为精确替换或紧随追加。

### 更新 1：§1 核心设计理念·关键点（L28 后追加）

**旧文**（保留不动）：

```
- `game-engine` 是**纯逻辑**：输入"状态 + 动作"，输出"新状态 + 事件"。不碰 DOM、不碰网络、不碰定时器。
```

**新文**（紧随其后新增一条列表项）：

```
- native 运行时（[ADR-0043](decisions/0043-std-thread-ownership-runtime.md)）内部自管 OS 线程与 `std::sync::mpsc` 通道（NPC 组线程 + 每股交易部线程），但对外仍是纯函数契约 `step()`，不碰任何 I/O；wasm 宿主无 OS 线程，恒走串行管线。
```

### 更新 2：§5 目标目录结构·packages 树（L113-115 替换）

**旧文**：

```
├── packages/
│   ├── engine/              # 游戏核心逻辑 (Rust crate, 纯逻辑；不含宿主绑定)
│   └── engine-gpu/          # GPU 能力探测/实验管线；权威计算仍为 CPU
```

**新文**：

```
├── packages/
│   ├── engine/              # 游戏核心逻辑 (Rust crate, 纯逻辑；不含宿主绑定)
│   │   └── src/runtime/     # std-thread 运行时：组线程/交易部/协调者/协议/world/驱动器（ADR-0043）
│   ├── engine-gpu/          # GPU 能力探测/实验管线；权威计算仍为 CPU
│   └── std-thread-proto/    # 三臂选型原型包（ADR-0043 证据；批 11 验收后退役）
```

### 更新 3：「Escrow tick 与验证边界（ADR-0017）」节·管线段（L128-131 整段替换）

**旧文**：

```
市场 tick 采用一条生产路径。SealAllocationSnapshot 由 `DecisionResourceSnapshot::seal` 一次按账户并行固定报价到期释放后的资源；
DecisionShadow 决策影子与 AccountValidation/StockProcessing 就绪轮次承接真实计划依赖，最后统一进入 ReceiptAggregation 收据聚合、Settlement 结算、
Projection 派生、PreCommitValidation 哈希核查及 CommitTick `engine::commit_tick`。账户/股票可并行，同股票 FIFO 不变。
线程预算为 1 不是另一套串行引擎。
```

**新文**：

```
市场 tick 的 native 生产路径是 std-thread 运行时（[ADR-0043](decisions/0043-std-thread-ownership-runtime.md)）消息协议：协调者 `broadcast_start` 广播 `Arc<TickSnapshot>` → NPC 组线程决策∥+实体扣除+分段 flush，与每股交易部线程 `begin_tick`（隐式转正→日界重置→行情快照→开盘清算→到期撤销）边收边撮∥ → `converge_round` 收齐 G+N 报告（commit 前唯一同步）→ `TickEvidence` 过校验后单点安装，或广播 Abort 逆序回放三层 working overlay（组账户/交易部簿/协调者 candidate）。等价语义保持：失败原子性、现金/股份守恒、事件 seq 单调、同拍交付（`apply_same_tick_fill_projection`）。账户/股票可并行，同股票受理先后不变。串行管线仅 wasm 宿主回退使用；native 恒为并行拓扑。
```

### 更新 4：同节·rayon 段（L156-160 整段替换）

**旧文**：

```
股票流使用 `rayon::in_place_scope`：从宿主或测试线程进入时，协调循环留在调用线程，
股票任务才进入共享 Rayon 池，避免多个协调者占满 worker 后阻塞等待自己的任务。
这不创建额外线程，也不改变同股受理顺序或完成后的续行。已知边界仍在：若调用者本已在
Rayon worker 内，多个嵌套 tick 的协调者仍可能耗尽同一个池；本次修复不证明该调度场景无死锁。
把所有等待改成协作执行也不可替代这一边界处理，因为协调者内联执行较慢股票时会阻碍已完成股票的续行。
```

**新文**：

```
股票撮合由每股一个交易部持久线程承载（ADR-0043）：组↔交易部全互联 `std::sync::mpsc` 直发（G×N 下行 `Ops` + N×G 上行 `DeskBatch`），数据面不经协调者；无 Barrier、无 Condvar、无共享 Mutex。正确性基石是锁步不变量：交易部实体批次先于 `DeskReport` 发送、协调者收齐 G+N 报告后才广播下一 `Start`（mpsc FIFO + happens-before）；活性由相位感知看门狗（`recv_timeout` + `Phase`，`watchdog.rs`）守护，重述见 ADR-0043 关联的活性契约。rayon 随旧调度形态退役，`in_place_scope` 嵌套池已知边界不再存在。
```

### 更新 5：同节·panic 段（L140 替换）

**旧文**：

```
panic 是进程级故障，不使用 catch_unwind 冒充可恢复 tick。
```

**新文**：

```
panic 是进程级故障，不使用 catch_unwind 冒充可恢复 tick——串行管线保持该语义。native runtime 的 worker panic 走显式运输协议（ADR-0043 决策 9）：单 tick 执行体 `catch_unwind` 捕获，payload 经 `UpMsg::TaskDied` 上行，协调者广播 Abort+Shutdown，收齐 `GroupFinal`/`DeskFinal` 并 join 后 `resume_unwind` 原 payload 重抛——tick 仍判死、已收敛权威未被触碰，是显式失败运输，不是冒充可恢复 tick。
```

### 更新 6：§4 状态与持久化（L86-87 句后同段追加）

**旧文**（保留不动，作为追加锚点）：

```
[ADR-0025](decisions/0025-day-end-only-persistence.md) 和
[ADR-0029](decisions/0029-responsibility-names-and-contract-versions.md)。
```

**新文**（该句之后接排）：

```
std-thread runtime 的恢复=日终静点重建线程拓扑（NPC 组线程与交易部线程重建，`WorldSpec.initial_committed_tick` 与 `session.tick()` 锁步对齐）；活动委托不进档（日终清簿后天然为空）不变；日内回滚=Abort 三层 working overlay 冲回（[ADR-0043](decisions/0043-std-thread-ownership-runtime.md)）。
```

### 更新 7：§6 数据流（L180 替换）

**旧文**：

```
  → [核心层] GameSession.step() → Event[]
```

**新文**：

```
  → [核心层] GameSession.step() → Event[]（签名不变；内部经 `step_runtime_enabled` 分派——终态=runtime 唯一路径、开关删除，wasm 恒串行，见 ADR-0043）
```

### 更新 8：§7 待定清单（L192 后追加一行）

**旧文**（列表末行，保留不动）：

```
- [x] 联机协议：REST 命令/快照 + WebSocket 事件（ADR-0005）
```

**新文**（其后追加）：

```
- [x] 并发模型：**std::thread 所有权转移运行时** ✅ [ADR-0043](decisions/0043-std-thread-ownership-runtime.md)
```

### 附注（落库时一并裁定，非第 8 点正文）

- L164-167 尚有一处 rayon 残留表述（「Rayon 注册表容量只表示池中配置的 worker 数…
  实际 runnable 线程由外部 runner 从 Linux `/proc` 采样」）——属性能测量方法论叙述，
  草案八点清单未覆盖。建议批 10 落库时将该句中「Rayon 注册表容量」改为「线程池注册表
  容量（历史 rayon 形态）」或整句保留为方法论记录，**不要静默删除测量方法约束**；
  取舍由批 10 执行者记录在提交说明。
- 更新 3/4 落库后，全文 grep `rayon`/`Rayon` 复核：除上述附注处与 ADR-0017 历史语境
  引用外不得再有 rayon 作为现行机制的表述。

---

## 四、批 10 落库执行顺序建议

1. ADR 0043 先落（trading-rules 的 A6 条与 architecture 多处引用它，链接先成立）。
2. trading-rules.md 六条＋可选 A1 指针（§一）。
3. architecture.md 八处＋附注裁定（§三）。
4. 落库后 grep 复核：`decisions/0043-` 链接可达、`frozen_cash`/`bound_fallback` 等
   名词与源码一致、无残留 rayon 现行表述。
5. 按 CLAUDE.md 独立复核门禁：由未实施本包的 subagent 审查完整 diff（大 A 语义、
   最小范围、遗漏边界）。
