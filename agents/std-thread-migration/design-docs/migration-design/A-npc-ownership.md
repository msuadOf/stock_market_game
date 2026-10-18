# 任务A：NPC 所有权分线程化设计（std-thread 迁移）

日期：2026-10-14。分析者：任务A subagent（只读分析，未改源码）。
对象：spine-simplify-01 引擎终态 + std-thread-proto 原型。所有行号以两树当前文件为准。

## 0. 结论速览

- 现引擎 NPC 是**全局账户**：现金单一池、持仓跨股、每次 decide 读**全市场**视图、一次 decide 可产**多股** Intent。跨股持仓不是边缘情况而是常态（seed_float 把每只股的流通盘摊给全体 NPC）。
- 三方案中，**(b) 协调者决策**直接否决（串行化决策=放弃内联方案的全部收益）。
- **主推荐 (a-strict)**：NPC 按主交互股分给该股线程（= 原型已实测 982 t/s 的语义），状态全私有、零跨线程协调、资源实体模型退化到"不需要转移"的最简形态。代价是三处**模拟行为语义简化**（非交易规则漂移），须登记并请用户确认。
- 回退方案 (c)（账户分片+跨股只读视图+intent 路由+成交回写）：保住现引擎跨股行为，但每 tick 从 1 个屏障变 3 个相位，文件面大约再翻一倍。
- **最不确定点**：散户"全市场均匀随机选股 + 从全部持仓中选卖"是否为必须保留的模拟语义。它单独决定走 (a) 还是 (c)（见 §5）。

## 1. 现有 NPC→股票交互面（读/写盘点）

### 1.1 读面：全市场视图是决策输入的默认形态

- `DecisionSnapshot` 持有**整个市场**的 `MarketView`，所有 due NPC 共享同一份：
  `packages/engine/src/session/pipeline/decision_snapshot.rs:13-23`（`market: MarketView` 在 :18）。
  `MarketView.stocks: BTreeMap<StockId, StockView>` 含全部股票（`strategy/mod.rs:99-105`）。
- 各策略实际读取范围：
  - **Retail**（`strategy/retail.rs`）：从**全部股票**均匀随机选一只下单（:22-33，rng 索引）；noise 卖分支从"市场 ∩ 自身可卖持仓"里随机选股（:89-108）——读面=全市场 keys + 自身跨股持仓。
  - **Retail 行为层**（`behavior/heuristics.rs:25-50` `select_observed_stock`）：60% 从自身持仓（任意股）选，40% 走 watchlist（70% 概率）或全市场随机。
  - **Inst**（`strategy/institution.rs:18-62`）：遍历 `market.stocks` **全部**股票做估值带判断，每次 decide 可对多股各产一条 Intent。
  - **Hot/HotReversal**（`strategy/hot.rs:13-56、66-107`）：同样遍历全部股票。
- 自身视图：`SelfView.cash` 是账户级单一现金池，`positions: BTreeMap<StockId, PositionView>` 跨股（`strategy/mod.rs:109-114`）。买入可用量按同一 `own.cash` 对每只股独立估算（institution.rs:42、hot.rs:30），真正的现金占用检查在准入层。

### 1.2 写面：Intent 自带目标股票

`Intent::PlaceLimit/PlaceMarket { code: StockId, .. }`（`strategy/mod.rs:136-145`）——一次 decide 的输出可以指向任意多只股票。工作单对齐（撤旧挂单）的范围已被引擎限定为"本次 review 的股票集合"（`npc_state_projection.rs` 的 `ReconcileScope::ReviewedStocks`），即撤单面天然按股 scope，这对按股分线程友好。

### 1.3 账户与辅助状态（都挂在全局 session state 上）

- `Account`：`cash: Money` + `positions: BTreeMap<StockId, Position>`，`Arc` COW 分页存储（`account.rs:93-110`、`session/account_book.rs:17-37`）。
- 初始分配把**每只股**的流通盘摊给**同一个全体 NPC 池**（`session.rs:2167-2199` `seed_float` → `split_float` :2205；default-setup.json 的 `retail_count: 20000` 是全局数不是每股数）→ 一个 retail NPC 通常持有**全部 5 只股**的小仓位。跨股持仓是引擎现状的常态。
- 准入预留按账户聚合：`AccountReservation { cash, sell: BTreeMap<StockId, u32> }`（`live_reservations.rs:13-21`）——现金是账户级、卖量按股。
- 附属状态：注意力调度（`attention_scheduler`/`npc_attention`，session.rs:2465-2484 插入）、belief 参与者（watchlist/价格记忆/信息集，session.rs:2447-2455）、retail_experience、history_reads、plans 链。这些都是"账户归属"状态，迁移时要跟随账户走。

### 1.4 原型（对照）

原型 NPC 是**每股私有名册**（`std-thread-proto/src/setup.rs:166-234` `build_roster`）：retail/inst 按股配额、hot 轮流放前 N 股；SelfView 只插本股键（`worker.rs:413-421`）；MarketView 只插本股（`worker.rs:535-569`）；意图全部本地应用（`worker.rs:457-533`）。即原型从输入到输出都把 NPC 限制为**单股专家**——982 t/s 就是这个语义下的数字。

## 2. 方案 (a)：NPC 按主交互股分配到该股线程（原型模型）

机制：名册按股建立（改 `populate_npcs`/`seed_float` 为按股配额），NPC 的 Account+策略+attention/belief/experience 状态全部下沉为该股 worker 线程私有；每 tick worker 用**本股单股视图**内联跑名册决策，产物全部本地应用。

- 优点：
  1. 与用户模型 1:1（一股一持久线程+内联决策+无线程数概念），与 982 t/s 原型数据同构，无未验证结构。
  2. 零跨线程写、零路由：决策/挂单/撮合/T+1/解冻全在本线程（原型 worker.rs:285-399 四相位已验证）。
  3. 资源实体模型的**最彻底形态**：现金/持仓所有者恒定，连"转移"都不需要。
  4. 工作单撤换范围本来就是 ReviewedStocks，单股语义下自然闭合。
- 代价（模拟行为语义简化，须在 docs/trading-rules.md 登记并请用户确认；**不触及交易规则**——T+1/涨跌停/整手/费用/集合竞价相位不变）：
  1. retail 全市场随机选股 → 恒选本股（retail.rs:22-33 退化为单元素集合）；noise 卖"从全部持仓选"→只卖本股（:89-108）。
  2. 行为层 60%持仓/70%watchlist/全市场发现（heuristics.rs:25-50）→ 全部坍缩为本股；watchlist/注意力调度跨股发现失去意义（可保留机制但候选集=本股）。
  3. inst/hot 全市场扫描（institution.rs:18、hot.rs:13/66）→ 只看本股。现引擎 5 个 inst 是全局账户对 5 股各挂单；原型是 5 个每股 inst 只管本股——市场宽度行为不同。
  4. setup 契约变化：`npcs.retail_count` 从全局数变为按股语义（或新增 per_stock 配置），default-setup.json 与存档契约随之改（无版本概念，可破坏）。
  - 附带：RNG 抽样序列与现引擎不同 → 轨迹不同（用户已明确不要求确定性）。

## 3. 方案 (b)：NPC 决策留协调者 —— 否决

s5 负载每 tick 到期约 2 万账户（base_observation_probability≈1 的 retail 为主），`decide_npc_account_from_shared`（npc_decisions.rs:180-286）含策略恢复+行为层+urgent 评估，单账户微秒级 → 串行决策相位每 tick 数十毫秒，直接吃掉内联方案 982→359 t/s 量级的全部差距。"协调者也一股一线程化"在逻辑上就退化为方案 (a)/(c) 的按股归属。另 DecisionSnapshot 组装（SelfView/行为观测投影）本身也是 per-account 大头工作，一并串行化不可接受。**否决，不进入实测。**

## 4. 方案 (c)：账户分片 + 跨股只读视图

机制（保留现引擎全部行为语义）：
1. 每个 NPC 账户（cash+positions+附属状态）固定归属一个 home 线程（按 AccountId 分片均衡，与股亲和无关——retail 随机选股下没有亲和可言）。
2. coordinator 每 tick 广播 `Arc<MarketView>`（全股只读，原型 MarketSnapshot 的推广）；home 线程内联跑本片账户 decide（复用 `decide_npc_account_from_shared`，SelfView 从本片私有状态组装——持仓跨股但都在本线程内，无跨线程读）。
3. **意图路由屏障**：decide 产物按 `intent.code` 投递到目标股线程收件箱（N×N 队列矩阵）；join 后各股线程撮合。
4. **现金 escrow**：一次 decide 可产多股买单，对同一 `own.cash` 并发预留在不同股线程会超额。解法=资源实体模型：发送线程在意图序内顺序拆分现金实体为每股交易部的 escrow 包（决策产物天然出自单一 decide，本地顺序扣减即可，无需锁）；卖量是 (NPC,stock) 粒度、天然按股无冲突。
5. **成交回写相位**：撮合产生的 per-account 现金增量（买方扣/卖方收）路由回 home 线程应用；持仓增量直接落目标股线程的持仓分片（此后该 (NPC,stock) 持仓的所有权在目标股线程——home 的 SelfView 组装读经快照广播，滞后一 tick 可见）。现引擎的 receipt 聚合/账户结算管道（receipt_aggregation.rs、account_settlement.rs）与此同构，可改造复用。

- 相位账：每 tick 1 屏障（原型）→ 3 相位（决策 join → 路由+撮合 join → 回写 join）。零售随机选股下约 4/5 意图是跨股的，路由+回写是常态路径不是边角。
- 活性：SPSC 队列矩阵 + 双 join 屏障无循环等待，比现 rayon 池的四前提活性契约（stock_stream.rs:3-31）简单得多；但"契约四前提的实质"要求每相位有限完成，路由失败/通道断连要显式 StepFatal。
- 复杂度：intent 路由矩阵、escrow 拆分/退回（未成交必须原路退回）、回写合并、跨股 SelfView 组装四处新机制。

## 5. 方案对比与推荐

| 维度 | (a) 按股名册 | (b) 协调者 | (c) 分片+路由 |
|---|---|---|---|
| 每 tick 屏障 | 1（原型已测） | 1（但决策串行） | 3 |
| 跨线程消息 | 0 | 0 | 意图+escrow+回写，常态路径 |
| 现引擎行为语义 | 三处简化（§2） | 保留 | 完整保留 |
| 资源实体模型 | 不需要转移（最简） | 无关 | 正是 memory 中的转移设计 |
| 与 982 t/s 原型的距离 | 0（同一语义） | 远（串行决策） | 未实测，结构税显著 |

**推荐 (a-strict) 为主案**：与用户拍板的内联模型和实测数据同构，工程面最小，语义简化点集中且可登记。唯一先决问题=§5 末尾的语义确认。若用户要求保留跨股行为（特别是 retail 全市场选股），则走 (c)，接受 3 相位与路由面。

不建议折中（如"仅 hot/inst 给全市场视图"）：那部分意图仍走跨股路由，(a) 的零协调优势和 (c) 的语义完整性各失一半，两头付费。

## 6. 与资源实体模型的兼容性

- (a)：现金=每股名册内私有标量，持仓=(NPC,本股) 私有。memory（project_resource_entity_model.md）设计的"NPC→交易部→NPC 所有权流动"在所有者恒定时退化为**无需转移**——是该模型的最简上界，不冲突。
- (c)：严格实现 memory 的设计——决策时现金实体拆分 escrow 给多股交易部（§4.4），撮合后成交部分留在交易部→回写 home 合并，未成交原路退回（memory 明示"必须实现的路径"）。持仓实体 (NPC,stock) 天然按股分片，所有权随成交迁移到目标股线程。
- 两案都不需要锁；分歧只在"是否真的发生所有权流动"。

## 7. 文件面预估

### (a) 主案（引擎侧改造 + 原型移植）

新增（以原型 2094 行为蓝本，扩展为引擎语义）：
- `engine/src/runtime/worker.rs`（一股一持久线程主循环：名册+订单簿私有，四相位）≈ 700-900 行
- `engine/src/runtime/coordinator.rs`（广播/收束/公共历史，原型 run.rs:113-206 推广）≈ 300 行
- `engine/src/runtime/book.rs`（本地撮合/T+1/费用，原型 book.rs 529 行对齐引擎 orderbook 语义）≈ 600 行
- `engine/src/runtime/roster.rs`（按股名册构造 + attention/belief/experience 下沉）≈ 400 行
- 对应单测 ≈ 1000+ 行

改造（engine 内）：
- `session.rs`：populate_npcs/seed_float/split_float 改按股名册（:2167-2492 区段）；tick 编排骨架改为 coordinator 驱动
- `session/pipeline/authoritative_tick.rs` + `stock_stream.rs`（2401 行）+ `ready_stock_stream.rs`：rayon 池编排+活性契约四前提整体替换为 join 屏障模型（活性前提以"每相位有限完成+通道断连显式失败"重述，不依赖 rayon 术语）
- `compute.rs`：ComputeContext/ComputeBackend/CpuBackend（:1227-1292）与 `npc_decisions.rs` 的 quota/池派发协议（:343-702）删除，决策入口收敛到 `decide_npc_account_from_shared`（:180-286，保留复用）
- `npc_decisions.rs`/`decision_snapshot.rs`：快照组装下沉到 worker（单股视图）；DecisionSnapshot 保留为测试/验证路径
- 横切相位按股化+收束合并：quote_expiry、settlement、receipt_aggregation、day_end、persistence、诊断投影
- 粗计：**改造 ~25-35 个文件，新增 ~5 个文件（不含测试）**；`default-setup.json` 契约改按股配额；trading-rules.md 登记三处行为简化

### (c) 回退案（在 (a) 文件面之上追加）

- intent 路由矩阵（N×N 收件箱+路由屏障相位）、现金 escrow 拆分/退回、成交回写 home 合并相位、跨股 SelfView 快照聚合，再约 **8-12 个文件/改造点**，且 worker 相位机从 1 相变 3 相。

## 8. 待用户确认（进入实施前）

1. 是否接受 §2 的三处**模拟行为简化**（retail 恒选本股 / 行为发现坍缩本股 / inst+hot 只看本股）？接受→(a)；不接受→(c) 并接受 3 相位。
2. (a) 下 watchlist/注意力/计划链（plans/parent orders）这些为跨股发现设计的机制，是保留机制但候选集=本股，还是随简化一起裁剪（建议：retail experience 保留、跨股发现部分裁剪，单独列清单交复核）。
