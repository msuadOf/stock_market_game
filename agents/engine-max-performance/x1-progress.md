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

- **棒次 2（已完成）**：写集侦察完整化 + 叶子层全部 view/write 化。提交链：86f5a598（骨架+范例+2 测试）→ 2acc0984（写组 11 字段补全+phase/minute/date 视图助手）→ views/self_views 转换提交（build_market_view/market_price_path_observations/working_orders_for_accounts 自由函数化+薄委托）→ npc_split 扩展提交（公司域三 Arc+issuer_of_stock+npc_writes 窄构造）→ personal_state 提交（take_on/install_on 写组版）。每步 lib 1316/8 基线一致。
- **棒次 3（已完成主体）**：decision_chain（capture_for_roots/build_technical/exposed_stocks _on 化，含 view.observation_civil_instant 助手新写）与 retail_analysis 四函数（capture/observe/candidate_signals/cost_signal 全 _on 化，experience 经 &*writes.retail_experience 再借用传入）——已提交，lib 1316/8 基线一致。
- **棒次 4 入口（剩余环节，从叶子往上）**：
  0. （已完成项 1、2——保留编号防引用错乱）
  3. execution/orders.rs:11 materialize_parent_order_intents → on(view, &mut writes.parent_orders, ...)。**棒次 4 侦察发现（必须先处理）**：
     - **已解决（棒次 5 首笔）**：npc_order_lifecycles 已改 owned Vec 克隆快照并列入写组第 12 字段，拆分测试含并发 push 隔离断言（提交见 git log，lib 基线一致）。
     - **已审计（棒次 6）**：`append_parent_child_or_working_intent`（orders.rs:184）体内 self.state 仅触 setup（无 pending_plan_events 写，非第 13 写字段）；`remove_npc_order_lifecycle`（session.rs:2936）仅写 npc_order_lifecycles（已在写组）。**写组定格 12 字段**。remaining_qty 为 Order 内部读（orders.rs:251 作用于订单条目，非 session）。
     - orders.rs 其余读：setup.config.lot_size、self.phase()（view 已有）、tick。
  4. execution/reconcile_plan.rs:25 plan_npc_working_order_reconciliation → on(view,...)：读集仅 tick/setup（:63-64 已核），其余参数传入；177 行小文件，可先转。
  4. decision_snapshot_capture.rs 全文件（capture/view+writes、experience 观察读 retail_experience 经 &*writes 再借用、build_self_view_for/build_behavior_market_checked/validate_market_view_inputs 全 view 化）；
  5. npc_state_projection.rs project_npc_state_in_place(view, writes)（strategy 写走 writes.accounts、last_retail_decisions 走 writes、validate_parent_order_materialization 随行）；
  6. npc_tick_preparation.rs queue_npc_for_next_tick → queue_npc_on(view, writes) + session 薄包装（candidate_commit.rs:65 调用点零改动）；
  7. 全绿后进入阶段二（join/stash/plan_tick 消费/TDD/基准，见上文设计）。
  - 已就绪的 view 助手：phase/current_market_minute/civil_date/issuer_of_stock/build_market_view_on/market_price_path_observations_on/working_orders_for_accounts_on/receive_private_intent_on/take_on/install_on。
- **本棒新发现（下一棒必读）**：
  1. **写集实为 11 字段**（E7 清单只列 7）：新增 `plans`、`parent_orders`（materialize_parent_order_intents 写，execution/orders.rs:11）、`belief_participants`、`history_reads`（PlanPersonalState::take/install 移除重装，decision_chain/personal_state.rs:85/112）。prepare 链已 grep 验证不读这四者（decision_resources/envelope_projection/continuous_matching_adapter 零命中）。
  2. **SessionReadView 还需补**：`company_registry`、`library`（Arc 字段，廉价引用）——observe_retail_analysis 经 `self.state.issuer_of_stock`（registry+stocks 翻译）与 `self.state.library` 读公司域；operations 视 roots 调用面再定。
  3. **既有先例可复用**：decision_chain/roots.rs:11 的 RootReadContext 已是"owned 快照式读上下文"（accounts/stocks/registry/operations/library/plans/日期分钟），DecisionChainObservation::capture_for_roots(session) 构造它——转换时可让 capture_for_roots 吃 &SessionReadView 从 view 构造 RootReadContext，避免两套快照逻辑并行。
  4. 需要转 view 的 GameSession 方法完整清单（含文件:行）：views.rs:13 build_market_view、views.rs:127 market_price_path_observations、self_views.rs:8 working_orders_for_accounts、session.rs:2388 phase（已在 view 复刻）、session.rs:1848 current_market_minute（已复刻）、session.rs:2420 civil_date（已复刻）、decision_chain/retail_analysis.rs:9 capture_retail_analysis（含 observe_retail_analysis 同文件、roots.rs capture_for_roots）、execution/orders.rs:11 materialize_parent_order_intents、execution/reconcile_plan.rs:25 plan_npc_working_order_reconciliation、decision_snapshot_capture.rs:495 build_self_view_for、CapturedExperienceObservation::capture（decision_snapshot_capture.rs 内，读 retail_experience 经 writes 引用再借用）。
  5. 转换顺序建议（每步全量 lib 对照 8 败不动）：views+self_views（leaf，方法保留薄包装）→ personal_state take/install_on(writes) → retail_analysis+roots（view 化）→ orders/reconcile → decision_snapshot_capture（view+writes）→ npc_state_projection → npc_tick_preparation 包装 → lib 全绿小步提交。
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
