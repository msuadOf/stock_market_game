# 交接：D3a-2c 开仓路径移植（第 7 任→第 8 任，2026-10-15）

## 1. 当前状态

- **HEAD**：`b65e31c8`（Belief-1b·下——披露驱动下沉）。工作区**干净**
  （`git status` 无输出，.tmp 不入库）。
- 本会话累计 26 提交（六项 Sol 整改→D0 常驻 driver→D2→D1→D4→
  D3 台账收口/观察基 v2/信念态/披露驱动）；Sol 四审通过、翻转批关闭。
- **env=1 全量 = 55 失败**（101→85→80→71→65→55）；默认 lib 1568/0/11；
  翻转批 1-5 全绿（10/13/14/14/15）。
- D3a-2c 完成后预期吃掉 decision_chain 27 中的开仓族；随后 D3a-3
  （加仓/撤销）→D3b（复审/风险/紧迫度）→D3c（读面投影）→D5→0。

## 2. 移植五件全图（行号级指引）

### 件 1：GroupAccount 成本基（**必须收口，无登记豁免**）
- 开仓派生消费 `positions().cost_price()`（roots.rs:57 附近
  `institution_behavior` 内）——组账户现在只有 qty
  （`packages/engine/src/runtime/group.rs:32` struct GroupAccount）。
- 做法：GroupAccount 增 `invested_cents: Vec<i64>` / `recovered_cents:
  Vec<i64>`（每股）：
  - **落账维护**：`group.rs:513 fn apply_entity` 的
    `Entity::BoughtShares`（invested += fill price×qty）与卖出腿
    （recovered += price×qty）。价格来源：DeskBatch 自带 `fills:
    Vec<FillFact>`（facts.rs:13，含 price/qty/order）——落账实体时
    按 order_ref 匹配 fill 取价；口径对齐 `account.rs:300
    apply_buy_total`（仅成交额，费用不入 invested）/`account.rs:395
    apply_sell`。
  - **构造投影**：`bridge.rs:117 accounts_projection_from_session`
    补 invested/recovered 初值（session positions 的
    invested_cents()/recovered_cents() 求和——注意 session Position
    的 invested 是金额累计，直接取）。
  - **回写同步**：`drive.rs` step() 持仓回写段（搜 `from_restored_parts`）
    把组侧 invested/recovered 写回（替换"保留会话侧已有"——D1 登记
    的成本基差异就此闭合，台账划账）。
- 注意：Abort 回滚——apply_entity 已记 GroupUndo::Shares/Frozen 逆操作，
  成本基需同样入 undo（新 GroupUndo 变体或扩 delta 结构）。

### 件 2：RootReadContext 组侧组装
- 定义：`decision_chain/roots.rs:6`（accounts/stocks/company_registry/
  operations/library/plans/civil_date/market_minute/day）；
  `capture` 在 roots.rs:32 附近。
- 组侧等价：组账户+成本基（件 1）=accounts 面；PlanBook 组内起始空
  （**runtime 模式 session plans 恒空**——run_decision_chain 不跑，
  天然一致，见 failure.rs deliver 分流）；CompanyRegistry/Operations/
  StockCatalog 构造期 Arc 投影进 WorldSpec（沿 belief_seeds 模式）；
  library 已有（Belief-1b）；civil_date/market_minute/day 从快照 tick
  推导（day=tick/tpd、market_minute=tick/60——注意与 session 的
  current_market_minute 口径核对）。

### 件 3：InstitutionDecisionRoot 直调
- 核心：`roots.rs:206 fn new` + `roots.rs:210 fn observe(mut self,
  &RootReadContext, &DecisionChainObservation) -> (AccountId,
  PlanPersonalState, PlanChainOperationBatch, PlanRootDiagnostics)`。
- 提 pub(crate)（roots.rs 与 personal_state.rs 的相关项）；组线程
  due 时直调（rayon 调度/探针全跳过——组线程模型任务即线程）。
- PlanPersonalState：组内由 belief_states 快照+镜像 attention 组装
  （personal_state.rs:96 take / :130 install 的组内等价）。
- DecisionChainObservation（观察输入）：组观察基 v2 已能构建
  price_paths/technical（group.rs 搜 `build_observations`）；
  market_view 用 `decision.rs build_market_view(&snapshot)`；
  exposed_stocks 简化为空集起步（公司披露曝光面——登记差异或随
  披露载荷补）。

### 件 4：开仓产物
- observe 返回 PlanChainOperationBatch——读其结构（plan_chain_
  candidates.rs）拆出：plan 创建（→组内 PlanBook，crate::plans 类型
  纯状态可复用）+子单（→PendingOp 并入 run_round 扣除路径）。

### 件 5：plans_debug 读面投影
- D4 模式：EchoBelief 旁加 EchoPlans（或复用）→组 PlanBook 上行→
  驱动器交付点回写 session plans_debug 投影源
  （decision_chain.rs:1316 plans_debug 读 state.plans——改投影源或
  驱动器直写 state.plans，**写 state.plans 须走直改标记**）。

## 3. 红证计划与测试命令

- 红：同输入（同观察+同 belief+同成本基）组内 observe 派生==
  session 版直调 observe 派生，逐字段比对（InstitutionBehavior/
  候选/PlanOpen 字段）——用 drive_tests 模式（参照
  `d3a2_group_observations_match_session_read_face`/
  `d3a2b_group_belief_states_match_session_projection`）。
- 回归命令：
  - `cargo test -p engine --lib`（1568/0/11 基线；连跑两轮防 flake——
    npc_decisions 偶发单例 flake 已知）
  - `cargo test -p engine --test runtime_flip_batch1 ... batch5`（10/13/14/14/15）
  - `ENGINE_STEP_RUNTIME=1 cargo test -p engine --lib`（55 基线，完成后
    报 X；失败名单 diff 用 `grep "^test .* FAILED$" | sort` 对比）
  - `ENGINE_STEP_RUNTIME=1 cargo test -p engine --lib "session::failure_tests::"`（12/12）
- 每件一提交一报告；红证=临时破坏生产面跑红（禁翻转断言充数）。

## 4. 已知坑与注意项

1. **成本基收口无豁免**（本件核心）——派生消费真值，登记不可替代。
2. **零量条目面差异**：组侧 codes_of_interest=当前持仓∪watchlist，
   无"曾持有"零量条目（session positions 含之）。开仓派生的
   assess_candidates 是否消费零量历史条目需先核对（roots.rs:77
   assess_candidates）——等价则不修，不等价则收口。
3. **attention 权威调和**（台账 Belief-1b·下注记）：调度域（candidate）
   =组权威 P1-10 链路；信息节奏域=组内推进；回写仅 information 域
   （`set_runtime_information_checks`）。candidate 保持锚未红=顺序流
   无漂移窗口（防护对象=投递跨 tick 化）。
4. **权威分流**：session `deliver_public_information` 在
   runtime_driver.is_some() 时停跑（notices.rs 分流）——组内承担。
   开仓路径同理：session 侧 run_decision_chain 若在 runtime 模式
   被触发需要同样的分流审计（当前不触发——plans 恒空的根源）。
5. 协调人纪律：positions 读面先核对等价再决定收口/登记（坑 2）；
   捆绑包边界克制（只捆真实读面）；稀疏下行；B 方向（跨线程往返）
   已拍板否决。
6. 疲劳防线：`deliver_information_for_npc` 提取时曾在拼接上翻车
   （索引漂移污染 notices.rs）——重置文件重做一次成型解决；大段
   拼接一律"同版字符串一次成型"，禁止分段索引。
7. 测试字面量：改 StockPublic/DeskStockSpec/WorldSpec 字段后，
   bridge_tests/decision_tests/skeleton_tests/desk_semantics_tests
   的字面量会缺字段——批量补时注意别把字段插到函数体外
   （desk_semantics_tests 曾中招）。

## 5. 台账/设计文件索引

- **总台账**：`.tmp/multi-tick-design.md`——尾部"D3 执行记录"含：
  A/B 拍板依据（用户 std-thread 模型：决策内联组线程+所有权转移；
  B=三段跨线程往返 128 核回协调瓶颈，否决）、依赖序勘正、
  Belief-1b 权威调和注记、D3a-2c 移植图（本文件的详版）。
- **D4 设计稿**：`.tmp/d4-phase-timing-design.md`（相位计时——已落地）。
- **Sol 证据**：`.tmp/sol3-red-evidence/`（四审红证+README）。
- 失败清单：`.tmp/flip-failures-after-p1.txt`（106 历史快照+归因勘误
  头注）；当前 55 名单用命令重取。
- 角色纪律：每批失败先判真伪（真实差异修 runtime+登记；注入面/
  旧机制契约→pin+登记等价锚）；红证附测试名+行号；env=1 与默认
  口径分开报。
