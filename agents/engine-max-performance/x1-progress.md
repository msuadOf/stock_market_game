# X1 借用解耦 + 跨 tick 投机预计算：设计与进度档

> 两周期大活，多棒接力。每棒开工先读本档"当前状态"节，收工前必更新本档。
> 背景：speculative-precompute-findings.md（E7 收档）、.tmp/perf-expl/tick-overlap/（诊断 patch 与 2084/2084 等价性实证）、E9 churn 地图（脊柱 54% 醒时 = clone/drop 机械）。

## 最终设计（本棒核实源码后定稿，后续棒除非发现新事实不要推翻）

### 精确读写集（已逐函数核实）

queue_npc 链路（capture_decision_snapshot → stream_npc_decisions 回调 → project_npc_account → receive_private_intent）的**全部写点**：
- `state.attention_scheduler`（pop_due_npc_ids，attention.rs:324）
- `state.npc_attention`（mutate_existing_parallel，capture 内）
- `state.retail_experience`（replace_existing_shared_parallel(experience_updates)，decision_snapshot_capture.rs:234）
- `state.last_retail_decisions`（push，npc_state_projection.rs:243）
- `state.pending_npc`（尾部赋值）
- `state.ingress_receipt_cursors` + `GameSession::ingress`（receive_private_intent，shared_ingress.rs:163）
- `state.accounts` 的 strategy 子域（`get_mut(account).restore_strategy(...)`，npc_state_projection.rs:232-237，**唯一 accounts 写点**）

投机 prepare（read_only_start::prepare 三分支：validate_existing_live_projection / DecisionResourceSnapshot::seal / prepare_incremental_continuous_inputs）读 markets、accounts(cash/positions)、setup、npc_order_lifecycles、envelope_ledger、price_history、tick/day/时钟等；**不读上表六个 NPC 专写字段**（seal 语义输出只用 cash/positions——探索波已核；实现时再逐消费方断言一次）。

=> 唯一冲突字段 = **accounts**（queue 写 strategy 子域 vs prepare 读 cash/positions）。同页共存，页面级 COW。

### 方案：双链统一走"读视图 + 专写组"，accounts 永远用浅克隆快照

1. 新模块 `session/pipeline/npc_split.rs`（名字可议）定义：
   - `SessionReadView<'a>`：全部共享只读字段的 `&` 引用（setup/stocks/rng? 只读用途/seed/markets/envelope_ledger/npc_order_lifecycles/price_history/market_minute_closes/candle_book/history_reads/plans/urgency_policy/belief_participants?/parent_orders?/tick/day/civil_clock…以两条链实际访问为准逐步补齐），**accounts 例外**：`accounts: AccountPagedMap<AccountEntry>`（owned 浅克隆——Arc 计数 bump，~µs 级）。
   - `NpcWriteFields<'a>`：上表 7 个写点的 `&mut`（含 `&mut Option<IngressBinding>` 与 `&mut AccountBook`——strategy 写直接走 live map）。
   - `fn npc_split(state: &mut CommittableSessionState, ingress: &mut Option<IngressBinding>) -> (SessionReadView, NpcWriteFields)`：逐字段不相交借用构造（Rust 允许同表达式内不同字段的 & 与 &mut 共存）。
2. **queue 链**全部函数签名从 `&mut GameSession` 改为 `(view: &SessionReadView, writes: &mut NpcWriteFields)`（或只取所需子集）；体内 `shadow.state.X` → `view.X` / `writes.X`。涉及：npc_tick_preparation.rs(473)、decision_snapshot_capture.rs(610)、npc_state_projection.rs(592)、shared_ingress.rs（receive_private_intent 变自由函数）、attention.rs(pop_due_npc_ids 保持 scheduler 方法)、self_views.rs(working_orders_for_accounts → 自由函数)、views.rs(build_market_view → 自由函数或 view 方法)、execution/orders.rs(materialize_parent_order_intents)、execution/reconcile_plan.rs(plan_npc_working_order_reconciliation)、experience 观察捕获（CapturedExperienceObservation 链）、decision_snapshot_capture.rs:495(build_self_view_for)。保留 GameSession 方法外壳做向后兼容转接（薄包装构造 view 后调用新自由函数），**既有调用点与测试尽量零改动**。
3. **prepare 链**同样改吃 `&SessionReadView`（构造时 accounts 做浅克隆）：read_only_start.rs(410)、envelope_projection.rs(321)、decision_resources.rs(321)、continuous_matching_adapter.rs(247)。普通路径 `prepare(session: &GameSession)` 保留为薄包装：内部构造 view（accounts 浅克隆）再调核心。
4. **COW 安全论证（写进 ADR/findings）**：prepare 持 accounts 浅克隆 ⇒ 所有页 Arc 计数 ≥2 ⇒ queue 侧 `Arc::make_mut` 必然克隆页后写入，绝不原地改 prepare 正在读的页 ⇒ 无数据竞争由 Arc 语义结构性保证（非靠纪律）。在 join 前加 `debug_assert`（首页 Arc::ptr_eq(view.accounts, live) 证明共享成立）。strategy 写后 live 与快照仅差已写账户的 strategy 子域；prepare 语义只用 cash/positions ⇒ 快照读等价（探索波 2084/2084 实证同结论）。
5. **commit 尾部（candidate_commit.rs::prepare_candidate_commit）新结构**：
   ```text
   rebase_private_for_tick_commit()   // 提前到 queue 前（错误延后到 queue 之后传播，Rayon1 逐字节等价已证）
   if 需要 queue_npc {
       let (view, writes) = npc_split(&mut candidate.state, &mut candidate.ingress);
       let (queue_result, prep) = rayon::join(
           || queue_npc_on(&writes..., &view),           // 注意 writes 里含 live accounts
           || if 下tick can_prepare(eligibility 用 view 判) { Some(prepare_on(&view)) } else { None },
       );
       queue_result?;  // rebase 错误在 queue 错误之后传播（首错序：重排诊断已证等价）
       stash = prep.zip(tick 号)                                          // 失配即弃
   }
   [swap]
   ```
6. **stash 消费**：GameSession 增加 `speculative_prepare: Option<SpeculativeStash>`（不进存档/不进 hash，沿 EvidenceCache #[serde(skip)] 先例）；plan_tick 入口先校验 `stash.tick == 当前 tick && can_prepare`，命中则消费（各 Result 仍按原优先级位置传播——同 E2 precomputed_stock_inputs 先例），失配/未命中整体回退现算。TDD 红灯：cfg(test) 计数器"命中时 plan_tick 不再内联调用 prepare 核心"。
7. **can_prepare 判定移到 view 上**（读 npc_order_lifecycles/phase/envelope_ledger 皆在 view）。

### 语义红线（复核清单）
- rebase 错误延后传播的首错序（诊断已证，落地时补 fixture：rebase 与 queue 同时坏，断言错误文本/类型不变）
- NPC 决策/attention/experience/strategy 写入顺序与内容逐字节不变（Rayon1 对照）
- 失败原子性：tick 尾部失败 ⇒ candidate+stash 一起丢弃；stash 纯读不落权威
- 首错优先级：hydration → resources → inputs（stash 各 Result 在原消费点传播）

### 测试/基准计划
- 单测：沿 read_only_start.rs 既有 7 测试扩展（命中计数器、tick 失配回退、1/4 worker 一致性、stash 不进 save/hash）；queue 链全部既有 filter（npc_state_projection/npc_tick_preparation/npc_decisions/shared_ingress）不改期望全绿。
- 全量 lib 对照基线 8 败名单不变。
- 基准：锚定交错配对 worker=4、<10% 占用核段、≥5 对、预登记规则；预期 ≥+10%（外推 +16%，E2/E3 历史折扣）。
- 复核：全新二级只读 agent（fork 无派生权限时由主会话代办）。

## 当前状态

- **棒次 1（已完成）**：设计定稿 + 阶段一第一切片。
  - 新增 `session/pipeline/npc_split.rs`：SessionReadView（accounts=浅克隆快照）+ NpcWriteFields + npc_split() + receive_private_intent_on 范例 + GameSession::read_view()；pipeline/mod.rs 注册。
  - 2 个新测试绿：①rayon::join 下写侧改 live accounts、读侧快照不变（页面 COW 隔离实证）②字段组版 receive 与方法版行为一致。
  - 全量 lib：1316 过/8 败 = 基线逐一一致（新增 2 测试，8 败名单未动，本切片纯增量无既有文件改动，失败必属基线）。
- **棒次 2 入口（下一棒从这里继续）**：
  1. queue 链逐函数改造吃 (view, writes)：npc_tick_preparation.rs::queue_npc_for_next_tick → decision_snapshot_capture.rs::capture_decision_snapshot_in_place（体内 shadow.state.X → view.X / writes.X；retail_experience 写走 writes；attention_scheduler pop/enqueue 走 writes）→ npc_state_projection.rs::project_npc_state_in_place（strategy 写走 writes.accounts，last_retail_decisions 走 writes）→ receive 改调 receive_private_intent_on。GameSession 方法保留薄包装（构造 read_view + 一次性 npc_split 不行——薄包装内部单线程直接用 view+writes 调核心即可）。子助手（views.rs build_market_view、self_views.rs working_orders_for_accounts、experience 观察、execution 两函数、market_price_path_observations、capture_retail_analysis）按访问字段逐个转 view 参数；view 缺的字段（如 history_reads/belief_participants/plans/parent_orders 若被用到）按需补进 SessionReadView。
  2. prepare 链改造（read_only_start/envelope_projection/decision_resources/continuous_matching_adapter 吃 &SessionReadView；prepare(session) 薄包装）。
  3. candidate_commit 尾部 join + stash + plan_tick 消费（设计见上）。
  4. 测试扩展、基准、独立复核。
  - 助手定位（本棒已核）：current_market_minute=session.rs:1848、civil_date=session.rs:2420、market_price_path_observations=session/views.rs:127、capture_retail_analysis=session/decision_chain/retail_analysis.rs:9（决策链根部）——这四个是 queue 链仅剩的"GameSession 方法级"依赖，其余全是 state 字段直达。
- 已知并行：同工作区有清理 agent 做小提交；提交一律显式 pathspec，index lock 冲突等几秒重试。

## 坑（本棒新踩/已知）
- rustfmt 禁止对入口文件跑（沿 mod 树重排无关文件，E7/E8 均踩过）；只对已改文件用精确工具，fmt 后 git status 核对改动集。
- `cargo test --release --lib` 禁用于性能诊断（debug-assertions 100x）。
- git 恢复一律 `git show HEAD:<path> > <path>`。
