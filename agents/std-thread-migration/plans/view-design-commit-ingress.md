# 视图内生战役核心问题深化：同拍交付提交点（Q2）+ 宿主写注入（Q4）+ 失败原子性

> 起草 2026-10-18。工作区 `.worktree/std-thread-model`，实际 HEAD `8908ea8d`
> （纲要起草时为 `21b1e88b`，分支已前进——本文行号锚点全部按 `8908ea8d` 现读现标）。
> 性质：设计深化稿，非 ADR。输入=纲要 `agents/std-thread-migration/plans/view-campaign-design-brief.md`
> §3 Q2/Q4 与 Sol 终裁 `reviews/sol-view-model-verdict.txt` 张力表第 2/5 行。
> 约束遵守：只读分析，本文为唯一产物。

---

## 0. 现状时序的精确刻画（设计的事实基础）

以当前代码为准，tick N 的消息时序与落账点：

| 步 | 事件 | 代码锚（HEAD 8908ea8d） | 组账户是否已应用本拍成交 |
|---|---|---|---|
| 1 | 协调者 `broadcast_start(N)` | `world.rs:1448`（run_ticks） | ——（Start 同时隐式转正 N−1：`group.rs:1558` apply_staging、`:1560` undo.clear、`:1587` committed_tick） |
| 2 | 组决策∥+实体扣除+分段 flush | `group.rs` run_round（`:3329` 起消费 external_pending；`:3566` final_flush） | 否（扣除侧已扣，成交侧未回流） |
| 3 | 交易部边收边撮，flush 实体批次**先于** DeskReport | `protocol.rs:1-8` 模块注释（锁步不变量） | 否（批次在组收件箱 staging：`group.rs:492`） |
| 4 | 组发 `GroupReport`（决策/发单结束即发） | `group.rs:3567-3584` | 否 |
| 5 | 协调者收齐 G+N 报告（converge，commit 前唯一同步） | `world.rs:1540` converge_or_die | **否——收齐≠组账户已应用**（终裁张力表第 2 行的代码事实） |
| 6a | 有续行：`broadcast_continue(r+1)` → 组**先落账上轮**再续行决策 | `world.rs:1516`；`group.rs:1708-1727`（handle_continue） | 逐轮部分应用（轮 k 的成交在轮 k+1 决策前落账——ADR-0043 决策 5） |
| 6b | 终轮：validator 过 evidence → `install_tick`（committed_tick=N、public_views 换装、report_log 入册） | `world.rs:1463-1470`、`:1530`、`:1615-1645` | **否**（终轮批次=unsettled 尾巴，`world.rs:285` 字段注释） |
| 7 | 驱动器**双写投影**：把 unsettled_trades 叠加进公开 session 账户 | `drive.rs:727-738` 调用、`:1492` 定义（apply_same_tick_fill_projection）；计划腿 `:859-871`；个人记忆裁剪 `:875-889` | 公开面"已见"（投影），组权威仍未见 |
| 8 | 下一 `Start(N+1)`：组 apply staging（权威落账）+ 驱动器整值回写 | `group.rs:1558`；`drive.rs:548-715`（dirty 两拍并集 `:579`/`:714`） | 是（此时双写合流，整值同步不双计） |

这正是 ADR-0043 决策 8 的双写：**公开面先结算（步骤 7）、组内权威下一拍再结算（步骤 8）**。Sol 终裁已定性其为协议时序的补偿方案（c 类），不是同拍交付的必然成本。视图内生战役要删的就是步骤 7 与步骤 8 的镜像回写（可删清单 D1/D2/D3），因此提交点必须重设计。

---

## 1. Q2 同拍交付的提交点归属

### 1.1 先回答"可见性语义边界在哪"（两个候选极的裁决）

题目给的两个极：**成交事实在 desk 结算时同步更新本组/账户 view** vs **全局裁决点统一发布**。

**结论：外部可见性必须且只能在全局裁决点（发布指针交换）；desk 结算时刻不是、也不能是任何可见性边界。** 展开为三条：

1. **"desk 结算时同步更新账户 view"的字面形态被否决**（不是偏好问题，是结构问题）：
   - 账户权威归组线程（ADR-0043 决策 1/2，R1）。desk 写账户 view=跨 owner 写，直接违反所有权模型；
   - desk 之间无全局序。NPC 跨股持仓是常态（ADR-0043 备选 4 否决按股私有臂的同一理由），一个账户的 view 会被 N 个 desk 的乱序更新撕裂，**不存在一致版本的定义点**；
   - "desk 结算时刻"对组不可观测（异步消息）——"同步"没有物理载体。
2. **owner 内部可见性边界=消息消费点**。成交事实进入组权威的唯一入口是 `apply_staging`（`group.rs:888`），它在 `Start`/`Continue`/`DayEnd` 的消息边界被调用。desk 的 flush 序经 mpsc FIFO 因果保持（`protocol.rs:1-8`），所以"消息消费点 apply"就是"desk 结算事实的确定性重放"——这是字面形态想要的东西的唯一可实现等价物。
3. **外部可见性边界=发布（版本指针交换）**。发布前，owner 内部一切变化（含已 apply 的 working 态）对外部读者**结构上不可达**——不是"被隐藏"，而是外部只有发布版本句柄这一条读路径（Q1 的 `Arc<PublishedTick>`）。因此"提交裁决前 owner 先更新自己的 working view"不构成提前可见，**这正是失败原子性允许 working/committed 并存的原因**（终裁：working/旧 committed 可并存，但不得成为两套独立演进的业务权威——working 是权威本体+undo 日志，不是第二权威）。

### 1.2 推荐方案：Seal 收尾确认相位（两段确认的落地形态）

在现有协议上**只加一个相位**，不改消息拓扑（仍零锁零共享，全部走既有 mpsc）：

```
终轮 converge（G+N 报告收齐，validator 过 evidence）
  → 协调者广播 Seal{tick, round_final}                ← 新消息（或复用 Start 变体）
  → 组：drain 收件箱（锁步不变量保证 tick N 全部 DeskBatch 已入队，见下）
        apply_staging(tick, round_final) 进 undo 日志（不转正）
        构建本拍读面分片（账户/计划进度/个人态/事件事实——不可变候选）
        上报 SealAck{group, tick, final_hash, 分片句柄}
  → 协调者收齐 G 个 SealAck（+desk 侧读面已在 DeskReport 内）
  → 终审（可失败步骤全部在此前完成）→ 交换全局 PublishedTick（原子指针交换）
     = 提交点 = 发布点 = Abort 窗口关闭点
```

要点逐条：

- **S1 提交点=发布点**。tick 的全局提交裁决与读面发布是同一事件：指针交换成功即 tick N 已提交且本拍成交对读者可见。不再存在"已提交但读面未含本拍结算"的窗口——ADR-0017 L86"提交成功后才生成 TickFrame"的倒命题（提交成功时结算与读面已就绪）成立。
- **S2 落账点唯一化**。`apply_staging` 仍是唯一结算落账点（`std::mem::take` 一次消费）。原来发生在 `Start(N+1)` 的终轮 apply 挪到 Seal——**时点移动，不是新写路径**。由此结构性消解三个东西：`unsettled_trades`（`world.rs:285`，Seal 后恒空，`drive.rs:718-726` 的尾巴过滤分支与 `world.rs:1523` 的抓取一起删）、`apply_same_tick_fill_projection`（D3）、`account_dirty_prev` 两拍并集（D2——发布分片直接取 apply 后权威值，无镜像无需并集）。
- **S3 转正点与 Abort 窗口**。组的 undo 退役**不需要新增 Commit 消息**：发布后协调者对 tick N 的 Abort 永不发（发布=不可逆），undo 保留跨发布无害（回放条件已不可能触发），仍在 `Start(N+1)` 清（`:1560` 现状不动）。正确性不变式从「转正点=下一 Start」改述为「**发布之后不得再对该 tick 发 Abort**」——更弱、更贴本质。
- **S4 锁步不变量扩展（正确性论证）**。Seal 为何能保证组 drain 到全部批次：desk 先 flush 批次后发 DeskReport（`protocol.rs:3-5`）；协调者收齐全部 DeskReport 之后才发 Seal ⇒ 批次的入队 happens-before Seal 入队，且共用同一组收件箱 mpsc（`protocol.rs:263-264` 注释）FIFO ⇒ 组先处理完全部 Batch 再处理 Seal。这与今天 Start 的论证**逐字相同**，不引入新不变量。
- **S5 续行语义不动**。`GroupReport` 保持早发（决策事实：continuation/skip/rejection 计数）——协调者的 Continue 判定不等待结算。Seal 只在终轮追加。纯"报告后移"（brief 候选 a）会让 Continue 触发也后移、组间流水线重叠受损（ADR-0043 决策 4 的分段 flush 红利被收束点吞掉），Seal 形态规避此损失。
- **S6 Continue 逐轮消费=前置 #1 的解**。step_frame 接轨前置 #1（Continue 早轮成交双计）要求投影逐轮消费——Seal 方案里没有投影，落账天然逐轮：轮 k 批次在 Continue(k+1) 消费、终轮尾巴在 Seal 消费，每批恰一次。"Continue 早轮成交恰一次落账"红测试（前置 #1 判据）直接作为本方案的行为合同。

**可选实验臂（eager apply）**：组在阻塞等 Continue/Seal 期间，收件箱里陆续到达的 Batch 可以**到达即 apply**（替代 staging 暂存）——apply 工作与协调者等其他组报告的 converge 等待重叠。语义等价（FIFO 顺序同、`verify_tick_round` 防御核对同、undo 记账同），只是把同一工作挪到等待窗。作为 Seal 方案的性能加臂做 A 探针配对实验，不作默认。

### 1.3 对照 ADR-0017"完整结算+提交后交付"逐句核

| # | ADR-0017 原文（锚） | Seal 方案判定 |
|---|---|---|
| 1 | "后端严格一次完整计算、结算并提交一个 tick。提交成功后，才生成一个 `TickFrame{...}`，然后才能开始下一个 tick。"（L86） | **更贴原文**。现状是"结算延到 N+1、N 拍靠投影补可见"——严格读"结算并提交一个 tick"，提交时结算应已完成；现状靠双写伪造该性质（终裁定性 c 类）。Seal 把结算收回 tick N 内（发布前）。 |
| 2 | §阶段 7："Settlement 结算 shadow：按账户分组…同账户同股票按 Buy 再 Sell 的生命周期顺序应用。结算只消费收据中的实收费用增量，不重算…费用或成交 delta。"（L42） | **保持**。apply_staging 逐实体应用（Entity 五类），费用直取 desk 实收值（`group.rs:937` charge=gross+commission+transfer_fee），组端不重算。Buy/Sell 生命周期序由 desk 事实流承载（批次 FIFO）。红证锚：现有 desk 语义测试续用。 |
| 3 | §阶段 8："Settlement 仍是一次统一资金/持仓结算，Projection 不另做结算；失败仍停留在 shadow。"（L43） | **现状双写违反此句精神**（投影=第二条数值应用路径；ADR-0043 决策 8 显式承认）。Seal 删除投影→回归一次统一结算。brief 候选 (c)（派生读面）保留一条派生数值路径——不做结算但仍需与权威落账逐位等价的证明负担= D3 的负担搬家，**成本转移而非消除**，这是不推荐 (c) 的主因。 |
| 4 | 分歧 #2："成交、撤单、拒单、竞价完成和日界释放属于密封批，次一密封才可分配；**CommitTick 提交后快照立即反映释放**。"（L126） | 两半都满足：发布版本 N 含本拍成交（后半句）；资源**分配**仍在下一分配点（前半句——Seal 后无本拍决策，`Start(N+1)` 决策才消费释放资源，与现状等价）。注意 Continue 语义（轮 k 成交对轮 k+1 续行可见可分配）是 ADR-0043 决策 5 的既定修订——Seal 不改它，分歧面不扩大；文档同步批须按 ADR-0043 口径重述 ADR-0017 #2（已在纲要风险表登记）。 |
| 5 | 数据流契约权威清单：accounts cash/positions/T+1、markets/订单簿、plans、envelope、RNG、seq…（L55） | 权威归属不变（组/部/协调者）。发布版本=权威的不可变快照，不并列第二权威（working=权威本体+undo，见 §1.1 条 3）。 |
| 6 | "Settlement 仍是一次统一结算…失败仍停留在 shadow"（L43 尾） | 失败面见 §3：Seal 后、发布前的全部变更仍在 undo 日志（shadow 等价物）内，Abort 冲回。 |
| 7 | TickFrame/timeseries_payload 是时间序列唯一协议来源（L86 后半） | 事件与 K 线分片由 owner（desk/组）在 SealAck 携带事实、协调者组装时分配全局 seq（seq 归协调者= R3 锚，`drive.rs:31` next_seq 迁移）。发布版本天然= TickFrame 的数据源。 |
| 8 | step() 契约与 panic 处置（L117） | 不变。`StepFatal` 边界外移到"发布前"；发布后驱动器残余步骤（派生诊断面）要么不可失败（不可变事实的纯投影）要么属 R5 查询期成本（惰性重建）。 |

**硬约束复述**（纲要 Q2）：同拍可见≠同拍资源复用。Seal 的期末落账发生在最后一次续行**之后**，续行轮资源可见性语义零变化（S6）；下一拍决策消费本拍释放资源的时间点不变（Start(N+1) 决策）。红证 1c 锚定。

### 1.4 需实验验证（承接纲要 Q2 实验 1/2，具体化）

1. Seal 相位 wall 增量：A 探针（10 组）配对——预期=一轮组内 apply（本来就要做，时点从 Start 挪到 Seal）+ G 条消息；eager apply 臂测重叠收益。
2. 协议轮数：SealAck 收集复用 converge 通道形态（`watchdog` Phase 扩一档），预期远小于决策轮。
3. #103 语义单测批（本拍结果可见/失败零泄漏/资源不可提前复用）作为 (a)/(b)/(c) 共同行为合同——本方案预期全绿。

---

## 2. Q4 宿主写注入面（ingress 链的去留）

### 2.1 现有链全清单（代码锚+去留判定）

现有链：宿主 → `SharedSessionIngress`（session 侧互斥队列，ADR-0032 受理序）→ step 入口 `freeze_shared_ingress` 截点 → `take_runtime_player_intents` → 驱动器 `route_external_intents`（Intent→PendingOp 转换+价格符号解析）→ `world.broadcast_external_plan`（**仅发组 0**，`world.rs:879`）→ `GroupIn::ExternalPlan` → 组 `external_pending` 缓冲 → run_round 在组内决策产物之后并入扣除路径（`group.rs:3328-3332`）。

| # | 环节 | 代码锚 | 判定 | 理由 |
|---|---|---|---|---|
| K1 | `enqueue_player_intent`（类别校验/关世代/受理序分配） | `shared_ingress.rs:48-74` | **保留** | ADR-0032 受理序的唯一序列化点；锁内分配 account_ordinal/stock_ordinal=全局受理序，与 owner 拓扑无关 |
| K2 | freeze 截点（迟到输入留下一拍） | `shared_ingress.rs:165-173`；调用 `failure.rs:432` | **保留**（位置=协调者线程 tick 前言，不变） | cutoff 语义是产品契约（`cutoff_keeps_late_external_input_for_next_tick`）；协调者=驱动线程天然持有 tick 边界 |
| K3 | 失败回灌（ready-ingress） | `failure.rs:439-458`（take/restore） | **保留** | 失败 tick 不消费外部队列、受理 gap 保留（`failed_private_receipt_gap...` 契约） |
| K4 | IngressBinding fork/读者水位（回滚 checkpoint 保留窗） | `shared_ingress.rs:97-135` | **保留** | checkpoint 回滚语义（`checkpoint_reader_retains...`）；水印清理已是读者弱引用驱动 |
| K5 | save/restore 投影 | `shared_ingress.rs:202-212` | **保留**（填充源改命令面游标） | ADR-0025 存档契约的 pending_player 键不变 |
| M1 | `route_external_intents`（Intent→PendingOp 转换、符号价解析、qty=0 拒收） | `drive.rs:1341-1470` | **迁入组线程** | 组内已有一切输入：local 路由用组内名册（`root_projection.account_ids` 反查，`group.rs:1048-1054` 同式）；daily_upper/lower 在 `TickSnapshot.stocks`（广播快照=上一收束点公共态，与今天驱动器读的 `world_public_views()` **同源同值**——`failure.rs:441-444`）；qty=0 拒收走组端 `round_rejections`（`group.rs:534-537` 同机制已在） |
| M2 | `broadcast_external_plan` 仅发组 0 | `world.rs:874-882` | **改为按 target_owner 分发** | 单组惯例位残留；多组形态下 `route_external_intents` 的 local=id_map 全局位还是错误映射（生产单组掩盖）。命令面按 (GroupIdx, LocalIdx) 寻址，每组每拍至多一条批消息（复用 `AcceptedBatch` 的按组批发形态，`protocol.rs:343-354` P1-A 先例） |
| M3 | `pending_player` 会话权威暂存 | `session.rs:538` | **迁移** | freeze 产物直接转 HostCommand 批，不进 session.state；save 投影从命令面游标重建（K5） |
| M4 | `InstallPersonal`/`ApplyPlanEvents` 等 cfg(test) 注入 + `AcceptedBatch` | `protocol.rs:358-407`/`:343` | **并入统一信封** | 测试注入、受理下行、玩家意图同乘一个命令面（Q3a 带副作用查询也在此面——`query_stock_history` 的读记录意图） |
| X1 | 驱动器 `inject_player_intents`+upper_limits 依赖 | `drive.rs:1472-1485` | **删除**（随 M1/M2） | 转换与路由都进组后驱动器不再持意图 |
| X2 | `pending_player` 的 NPC 暂存混用面 | `drive.rs:1339` 注释（历史缺陷） | **删除** | 命令面按 owner 寻址后类别混淆结构性消失 |
| X3 | "组簿空时不覆盖 session 测试注入计划"迁移分支 | `drive.rs:831-834` | **删除**（终裁点名） | 测试注入走命令面（M4）后，组簿即为唯一权威，无需空簿保护 |

### 2.2 统一 HostCommand 信封（brief Q4 候选 a 的落地）

```
HostCommand {
    target_owner: (GroupIdx, LocalIdx),   // 或 Desk(StockIdx) / Coordinator 域
    deadline_tick: u64,                    // 受理截点：该 tick 的 Start 前言消费；迟到=顺延下一拍（=K2 cutoff）
    command_id: u64,                       // 结果身份：回执事件/拒收携带，关联宿主等待面
    payload: CommandPayload,              // PlayerIntent(Intent) | QueryStockHistory | AdminBatch 条目 | 测试注入…
}
```

- **候选 (a)（经协调者）vs (b)（owner 直连）裁决：a**。理由：(i) 受理截点仲裁需要全局 tick 前言——freeze 本来就在协调者线程（K2），(b) 直连会制造"组已 Start、命令在途"的截点模糊，须在每组重造 deadline 仲裁=复杂度回潮；(ii) ADR-0032 受理序的分配点在宿主侧锁（K1），与 (a) 天然同点，(b) 反而引入"锁序与到达序"两个序要证明一致；(iii) 一跳的成本在 tick 前言（非热路径），且有 AcceptedBatch 批形态先例。
- **玩家意图 schema 判定：`Intent` 三变体（PlaceLimit/PlaceMarket/Cancel）不变**。信封化只加路由与身份元数据；OrderId 低 48 位=desk serial 的合成约定（`group.rs:320-322`、`drive.rs:1368`）与组端直转 `DeskIn::Cancel` 的通道语义原样保留。NPC `receive_private_intent` 的跨源受理（`shared_ingress.rs:175-200`）继续共用 K1 序源——现金竞争胜者跟受理序的契约（`production_step_cash_winner...`）不因命令面改变。
- **注入序=受理序等价**（P2-3c 判别式沿用）：受理序在 enqueue 锁内固定（K1）；消费序=freeze FIFO→命令批 FIFO→组 `external_pending` 并入序（`group.rs:3329-3332`：外部意图在组内决策产物之后、同批内 FIFO——与今天单组形态逐字段一致）。多组化只改寻址不改组内并入位。

---

## 3. 失败原子性：视图原生更新后 Abort 怎么回滚

### 3.1 现有 GroupUndo 六族盘点（对照基准）

`GroupUndo`（`group.rs:327-398`）现共 **12 个变体**，按回滚机制归并为**六族**（任务书"六族"按此口径对应，如实登记变体数）：

| 族 | 回滚机制 | 成员变体 | 写入点示例 |
|---|---|---|---|
| F1 账户资源增量 | 有符号 delta 逆减（`-= delta`） | `Cash`/`Shares`/`Frozen` | 决策期扣除 `group.rs:3455-3529`；落账 `apply_entity` `:1395-1481`；权益 `apply_right` `:1660-1701` |
| F2 派生累计/在途镜像增量 | delta 逆减（防御钳位） | `CostBasis`/`PendingSell`/`BuyPurseHeld` | `:1030`/`:929`/`:940`（apply_staging 内）、`:3458` |
| F3 在途登记增量 | delta 逆减 | `ForeignActive` | `:3462`/`:3525`/`:1502` |
| F4 标量前值恢复 | `= prev` | `NextDue`/`EverHeld` | `:1572`（ForceDue）/`:3536`（清仓位翻转） |
| F5 整态快照恢复 | Box 快照一次性保存后整体还原 | `Personal`（P2-3e 槽 take 化）/`PlanBook`（`plan_undo_saved` 首变更保存） | `:1221`/`:1068`+`save_plan_book_undo` |
| F6 键控票据 Option 前值 | Some→insert / None→remove | `ChildInFlight` | `:1079`/`:1158`/`:1265`/`:1316` |

`rollback()`（`:785-876`）逆序回放全部日志 + 清 staging/plan_fills/pending_force_due + `cur_tick` 回退 `committed_tick`。

### 3.2 视图原生更新后的两层分工（核心设计结论）

**owner 本地 undo 与版本回退不是竞争方案，是两层各自承载不同载体的分工：**

| 载体 | 回滚机制 | 依据 |
|---|---|---|
| **working 权威**（组账户/簿/计划/个人态——owner 线程内可变态） | **owner 本地 undo（§3.1 六族原样保留）** | 权威还是那组数据结构、写点不变（apply_staging/决策扣除/权益），undo 语义零改动；Seal 只是新增一个 apply 触发点（进同一日志） |
| **候选读面分片**（为发布构建的不可变 `Arc` 句柄） | **版本丢弃（drop）**——新增"族 F7"，无逆操作数据 | 未发布的候选分片无外部读者、无副作用，丢弃即回滚，零成本；与 desk 撤销日志 COW 的"丢弃候选"同档 |
| **已发布版本** | **不可回滚**——published=committed | 发布即 Abort 窗口关闭（§1.2 S3）；旧版本的存续是读者持 Arc 的生命周期问题（Q1 epoch/引用守卫），**不是失败回滚机制**——这个区分必须写进 ADR，防止把"读者还持旧版本"误当回滚能力 |

**business_state_hash 观察点迁移**（终裁表 4 行）：hash 在 Seal 后的期末 working 权威上计算、由 SealAck 携带（与现有 `committed_hash` 同源同法，`group.rs:499-502` 的 O(1) 指纹口径）；候选分片是权威的函数，不参与 hash。Abort 时组 drop 本地分片槽与回放 undo 在**同一消息处理**内完成——不存在"权威已回滚、分片还挂着"的窗口。

### 3.3 失败时点 × 载体矩阵（Seal 方案全表）

| 失败时点 | working 权威 | 候选分片 | 外部读者 | 既有机制对应 |
|---|---|---|---|---|
| 决策/发单中（组内 Err/panic） | Failed/TaskDied 上抛→Abort→六族逆序回放 | 尚未构建 | 零可见 | 现协议原样（`world.rs:1551-1598`） |
| converge validator 失败（终轮） | Abort→回放 | 尚未构建（Seal 未发） | 零可见 | `world.rs:1463-1470` 原样 |
| Seal 后、SealAck 收集/组装失败 | Abort→回放（日志仍在——S3 保证未清） | **drop 本地槽** | 零可见（发布指针未动） | 新增相位，回滚复用 rollback() |
| 发布后 | 已提交（undo 待 Start(N+1) 清，回放条件不可能） | 已发布（不可变） | **已可见——不得再 Abort** | 发布=不可逆点（新不变式） |
| panic（任何相位） | catch_unwind→TaskDied→Abort+Shutdown→Final 上交 | 随 Final 丢弃 | 零可见 | 决策 9 原样 |

对照：session 侧现行 undo（`failure.rs` 五域写时 undo `:841-867` + heap move-undo + 日结 accounts_full baseline `:883-887` + 安装链回滚 `:488-529`）是 D5 删除对象——视图内生后 session 不再被运行时写入（读者只 clone Arc），该族整体消失，**不是迁移是删除**。

---

## 4. 红证设计（每项一组；判据+突变红旋钮）

### 红证组 1：Q2 同拍交付（证明新模型不破坏本拍交付）

- **1a 发布即完整结算**：step() Ok 返回后立即从发布版本 N 读账户/计划进度分片，断言现金/持仓/T+1 冻结/成本基/计划执行进度 == 期末权威值；对照臂=同 fixture 同种子跑现行双写实现，逐位等价。**突变红①**：注释 Seal 相位的 `apply_staging` → 版本 N 缺本拍成交 → 红。
- **1b Continue 恰一次**（前置 #1 判据前移为行为合同）：`ForceContinuationForTest`（`protocol.rs:439`）构造多轮 tick，断言现金/持仓总变化==逐轮事实之和恰一次。**突变红②**：Seal/Continue 改消费全量 desk_trades（回退到 unsettled 过滤前的形态）→ 早轮双计 → 红。
- **1c 资源不可提前复用**：round r 撤单退款/成交释放在 round r+1 续行决策的可用集断言（ADR-0043 后果 3 锚续用）；Seal 期末落账后无本拍决策——结构性保证。**突变红③**：eager apply 臂若把 refund 提前暴露给同拍决策 → 红。

### 红证组 2：Q4 宿主写注入（证明注入序/截点/失败不丢单）

- **2a 注入序=受理序等价**（P2-3c 判别式扩臂）：三臂同种子对照——(i) 现行链（freeze→驱动器转换→ExternalPlan）、(ii) HostCommand 经协调者、(iii) cutoff 后迟到输入顺延 N+1；断言受理事件序==receipt 序（K1 锁序）、订单受理/拒收逐字段等价。**突变红④**：组消费端按键序重排命令（BTreeMap 遍历）→ 同价竞争/现金竞争结果变化 → 红。
- **2b 失败零丢失**（ready-ingress）：validator 失败+SealAck 后组装失败两注入点，断言 (i) 外部队列未被消费（binding snapshot 数不变）、(ii) 受理 gap 保留、(iii) 重跑同 tick 每笔恰一次受理（现 `discarded_candidate...`/`failed_private_receipt_gap...` 契约迁命令面）。**突变红⑤**：失败路径顺带清空命令批 → 输入丢失 → 红。

### 红证组 3：失败原子性（证明零泄漏+回滚等价）

- **3a Abort 回滚等价（权威面）**：SealAck 收齐后、发布前注入失败 → Abort → 断言 tick 起终 `business_state_hash` 一致（现有 R2 锚扩相位）+ staging/plan_fills/pending_force_due 清空。**逐族突变红⑥**：注释掉六族中任一 push（如 `PendingSell`）→ 该域回滚失败 → 红——**六族 × 各一**，矩阵化。
- **3b 读面零外泄**：失败后 (i) 发布指针未动——探针读者持版本句柄断言读数仍==N−1；(ii) 候选分片强引用归零（`Arc::strong_count` 探针/泄漏守卫）；(iii) 重跑成功后版本 N 事件 seq 连续无 gap、不含失败尝试残留（R4 锚）。**突变红⑦**：把候选分片提前挂上发布槽再失败 → (i) 红。**突变红⑧**：发布后再发 Abort（协议违规注入）→ 现有 converge 防御显式失败 → 红（负控：证明不变式有守卫）。

---

## 5. 与纲要的对接（登记，不展开）

- 本文 S1-S6 对应纲要 Q2 候选 (b) 两段确认的落地+吸收 (a) 的"结算收回 tick 内"；明确否决 (c) 派生读面（等价证明负担搬家）。
- HostCommand 信封对应纲要 Q4 候选 (a)，吸收 Q3(a) 查询即命令；M2 修复多组寻址是 Q1 分片粒度（按 owner）的前置一致化。
- Seal 相位与日结（Q5）同源：`handle_day_end` 步骤①的 `apply_staging` 在 Seal 后为空集（防御核对对空批次天然通过），日结静点确认复用 SealAck 形态——一次设计三处复用（纲要 Q5a 预判成立）。
- 文档同步批：ADR-0043 决策 4/5/6/8 按本方案修订（决策 8 双写删除、转正点改述、Seal 相位入协议）；ADR-0017 #2 措辞按 ADR-0043 续行语义重述；ADR-0032 实现映射更新（信封化不改契约）。
