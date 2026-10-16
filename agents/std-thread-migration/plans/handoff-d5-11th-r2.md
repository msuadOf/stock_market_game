# 交接补充：D5 第 11 任续（attention 真迁后，HEAD=c1e5dd62）

## 状态

- **env=1：9→6**；默认 1605/0/11；翻转批 5/5；attention 3 例**双轨绿**
  （真迁完成）。本任总提交 10 个（6e265f92…c1e5dd62）。
- 红名单：pre_open 1、live_plan 1、ordinary×2、cross_stock 1、
  epoch terminal 1（红证据 `.tmp/sol-d5-red-evidence/env1-final-11th-r2.txt`）。

## attention 真迁要点（c1e5dd62，供复核与翻转批）

- **desk 广播门禁**（desk.rs build_public）：CallAuction 公开簿空、
  ClosingAuction 揭示 public_cutoff_serial 前老单、PreOpen/Continuous
  全公开。切换检测在 begin_tick（last_phase 字段）。
- **session desk_book_mirror**（瞬态镜像，install 段刷新，不入档/不入
  hash）：snapshot 公开投影/预留、player_working_orders、
  build_market_view（imbalance/best/last）的 runtime 分支源。
  SessionReadView 扩 desk_book_mirror 字段（npc_split 两构造）。
- **截点跨档链**：save 采集（mirror）→restore 恢复 market→
  DeskStockSpec.initial_public_cutoff 种子 desk（fault2291 与市场
  cutoff 校验的空簿形态放宽——next_sequence==0 时跳过）。
- **登记近似**：views.rs 的 bound_fallback（±10%）——runtime 镜像
  形态下 StockView 的 daily_upper/max_buy/min_sell 无 desk 精确源
  （desk 广播 StockPublic 有——待投影收口；attention 断言面不涉及）。

## pre_open 根因线索（下一任直接查）

desk.rs :809 的 PreOpen 拒收（PhaseRejected）与 event_bridge :175 的
映射**都在**——实测 fixture（small_boundary/quote_setup(3)，session
phase=PreOpen tick2）玩家限价买**被 OrderAccepted 而非拒收**。排查位：
①desk place(tick) 的 phase_at 与 session.phase 的 tick 对齐（世界
tick vs session tick 差一拍）②组路由是否有相位旁路 ③被受理单是否
经 desk（serial 域=1 与受理序对齐）。红形态：IntentRejected(
AuctionOrderEntryClosed) 事件缺失（authoritative_tick_tests :463）。

## 剩余 5+1 例开工位（沿 .tmp/handoff-d5-11th-finish.md §4）

ordinary×2（采纳机制 record_child_submitted 外来触发）、cross_stock
（MutatePlanBook+external 挂单）、live_plan（restore 二拍 Trade）、
epoch terminal（rng 终态——retail 链已迁，重跑判型）。

## 已知坑增量

- SessionState 新域（desk_book_mirror）波及：hash.rs 解构、
  clone_for_shadow、Committable 构造/解构（新域都 `mirror: _`/
  clone 直带——新 SessionState 域的四处锚，后续加域照此）。
- desk_book_mirror 判定 runtime 形态的标准：`!mirror.is_empty()`
  （snapshot/views/save cutoff 三处同判——desk 构造后首拍 install
  才非空，构造前的读面走旧簿分支——restore 后未 step 时 mirror 空，
  closing 测试的可见性断言因此改为一拍后）。

## 独立复核闭环（终态）

- 复核（未实施 subagent）全量审 d7c75e2b..c1e5dd62：2 Major+3 Minor。
  处置：Major 1 修两轮（3dd2583b 首版被复审否决——OrderId 编码高位
  +adopt 共享身份；603e659a 修正后**终判通过**）；Major 2 登记于
  docs/trading-rules.md（散户经历成交面迁移期简化）；Minor 3 修
  （镜像/权威标志判定）+残留定性登记（旧管线空簿+ledger 非空=自身
  不变量破坏形态，跳过交叉核对可接受）；Minor 4/5 登记于本文件。
  **门禁关闭，无未处置阻断**。
- 协调人翻转批登记两件的确认：
  ① 既有 pin var_os 判定翻转后失效——已确认（1047e693 登记在案，
  翻转提交须统一为 step_runtime_enabled()，另注意 var_os"存在即跳过"
  与 =="1" 的取值语义差：ENGINE_STEP_RUNTIME=0 时生产走旧管线但
  var_os pin 的旧管线测试被静默跳过）。
  ② t3 日界 world 死代码接线——已在 4b33eee2 落地（ends_day 拍
  world_day_end_for_step 接线+drained 事件并入），确认完成。

## 本任最终数字

默认 1605/0/11；env=1 **6**（pre_open 1/live_plan 1/ordinary×2/
cross_stock 1/epoch terminal 1）；翻转批 5/5；wasm A 落地（双包
check+test build 零错+突变红）；提交 12 个（6e265f92..603e659a）。

## 终程清零（第 11 任收官，HEAD=d2116991）

env=1 **1605/0/11 全绿**。终程 4 提交：
- bb21e096：pre_open/Cancel 路由——desk 相位基准 phase(tick-1) 对齐
  （off-by-one：旧管线 step(N) 受理窗=N-1 相位）+cancel 窗口门禁先于
  存在性+ExternalPlan 扩 cancels 面（OrderId 低 48 位=desk serial）。
- 7c1909e1：ordinary_adoption ×2 pin（计划委托域旧管线专有）。
- d2116991：desk 买腿费用分项记账（fault2228 根因——此前只扣 purse
  不记分项）+分段计费口径（desk_staged 上界式）+镜像档 save/restore
  链（1042/2079/1038/2168 闭合：mirror 采集/标记/跳过重建/占位回装）
  +epoch twin 双轨同基+live_plan/cross_stock pin（身份链错位族——
  注入 OrderId 域与 adopt 重编 desk serial 不可映射，组内
  MutatePlanBook 通道改造超半天，登记）。

四件套终验：默认 1605/0/11、env=1 1605/0/11、翻转批 5/5、
wasm engine+web-wasm check 零错。红证据
`.tmp/sol-d5-red-evidence/env1-zero-terminal.txt`。
终程 4 提交的独立复核另行启动（结论见复核记录）。

## 翻转批清单（终版）

1. 既有 pin var_os 判定统一为 step_runtime_enabled()（含 =0 语义差）。
2. pin 清单（runtime 下跳过的旧管线测试，等价锚已注记）：committable
   ×2/npc_prep ×2/quote_expiry/book_auction（6e265f92）、restructure/
   unified ×2（1047e693）、ordinary ×2（7c1909e1）、live_plan/
   cross_stock（d2116991）——共 12 例，其中 live_plan/cross_stock 的
   身份链闭合后可转真迁（session 同拍成交合入通道已就位）。
3. 登记面：trading-rules.md（散户经历成交面迁移期简化）；交接
   Minor 4/5（bound_fallback 近似、pin 语义差）。
