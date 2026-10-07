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

## 棒次 10 完成记录与待诊断清单（下一棒精确入口）

- **已落地**：rebase 前置重排（Rayon1 等价实证）+ 单侧 join 生产路径（queue_npc_on 经 npc_split 全量套件验证）；prepare 链三文件 on(view) 化验收（分片+本棒修双引用/删零调用包装）；can_prepare_on/prepare_on 组装；**投机分支机制全通但默认关闭**（关闭态 1319/8 基线一致）。
- **待诊断（开启投机臂后两测语义回归，关闭即绿）——棒次 11 三步二分已完成，结论如下**：
  1. **时序假设否证**：投机读推迟到 queue 完成后（串行变体）两测仍红——非"split 时点快照陈旧"。
  2. **生产副作用否证**：保留 commit 尾部生产、禁用消费（消费臂条件置 false）→ 两测全绿——回归锁定在**消费路径**，生产动作无有害副作用。
  3. **Debug 等价陷阱**：消费点逐字段 Debug 字符串比对（hydration/resources/stock_inputs/stock_shards）全部相等、消费确实发生（eprintln 追踪 tick 1 产生/tick 1 消费），测试仍红——**Debug 格式相等 ≠ 语义等价**（嫌疑：某结构自定义 Debug 略字段/浮点位/内部 OnceLock·缓存·Arc 身份在 Clone 后行为差）。
  **棒次 12 根因已钉死（消费点探针实跑数据）**：
  - `resources Debug eq = true / PartialEq = false / diff = [account:AccountId(1)]`——DecisionResourceSnapshot 的自定义 Debug 只打 account_count（空证明），真 PartialEq 在 account 1 的 cash/positions 上不等。
  - **根因类别：stash 无法感知 commit 尾部与下次 plan_tick 之间对账户的直接改动**——测试夹具（或任何宿主直改 state）在两步之间 fixture_set_cash，投机 stash 携带旧现金，消费侧 tick+eligibility 双校验检不出"值过期"，验证器拿到偏松现金 → InsufficientCash 拒单缺失。第二测（母单链接）同机理（parent_orders 直改差异）。
  - 排除项（棒次 11/12 实证）：非快照时序（串行写后投机仍红）、非生产副作用（只产不消即绿）、非 Arc/Weak 身份敏感结构本身（差异在值不在身份——PartialEq 比的是 cash/positions 值）。
  **裁决（协调方）：方案 A（revision 新鲜度校验）。棒次 13 设计细化（含两个已排雷陷阱）**：
  - **陷阱 1（已排雷）**：revision 不能挂在 `AccountBook::get_mut` 等共享写入口——queue 的 strategy 写也走 get_mut，会把正常管线写标记成"外部改动"，每个有 NPC 决策的 tick 都回退，命中率崩溃。
  - **陷阱 2（已排雷）**：revision 挂在 state 且随 commit_from 覆盖——tick 中段的权威直改会被 candidate 的旧值覆盖丢失；但语义上 tick 管线本就赢过直改（直改被提交丢弃），stash 描述 candidate 终态仍然正确——无需 max() 补救，天然自洽。
  - **正解规格**：`revision: AtomicU64` 挂 **GameSession 顶层**（不入 state 克隆链），提供 `mark_external_mutation()`；**红线精确化（棒次 15，复核建议）**：mark 义务仅限**stash 读域的直改**——accounts / markets / setup / npc_order_lifecycles / envelope_ledger / price_history（含 candle_book/market_minute_closes 同族历史）；无关域（如纯诊断字段）直改不要求 mark。已迁入口：fixture_account_mut（accounts）、fixture_market_mut（markets）、存档恢复路径；宿主/未来玩家注入 API 同样先 mark。
  - stash 增 `produced_revision`（生产时 candidate.revision）；消费条件追加 `stash.produced_revision == session.revision`；不等→整体回退现算。
  - TDD：现有两红测即现成红灯（直改后 stash 拒绝消费）→ 绿；再加一红：无直改时 revision 不变、命中率不受损（计数器差分）。
  - 之后：复装并发投机臂（历史 42338194 父代代码）→ 全量 1319/8 → 1/4/16 一致性 → probe+verify-build → 锚定交错配对 ≥5 对 → ≥2% 保留 → 报协调方独立复核 → 终稿。
  3. 我方 mismatch 测试的 hits 精确断言已按进程级计数器先例改差分。
- **协调方第 3 点待办**：seal_on 的"只读 cash/positions"声明要落成真实 debug_assert/测试（写侧 restore_strategy 后 seal 输出必须不变）——加入下一棒首件事。
- 复装投机臂：git 历史本提交父代的 candidate_commit.rs 中被替换为 None 的那臂代码即完整实现（can_prepare_on/prepare_on/for_tick=view.tick）。

## 棒次 9 完成记录

- **stash 消费机制已落地**（15759c6e）：TDD 三件套绿（命中不内联/失配回退/错误原位传播），lib 1319/8 基线一致。要点：stash 无外层 Arc（derive Clone 即可）；计数器进程级按差分断言（先例）；权威 stash 在未提交 plan_tick 后保留（重试语义），随 commit_from 覆盖。
- **prepare 链分片规格（供协调方派只编辑子 agent，配方=execution 分片先例）**：
  - 文件 1 `session/envelope_projection.rs`（321 行）：`validate_existing_live_projection_with_observer` 及其内部 `project_borrowed_envelope` 等子调用 → `_on(view)` + 原方法薄委托（`&self.read_view()`）。读集已核：markets/auction_orders/envelope_ledger/accounts。
  - 文件 2 `pipeline/decision_resources.rs`（321 行）：`DecisionResourceSnapshot::seal` → `seal_on(view)`。读集：setup/stocks/markets/envelope_ledger/accounts（validate_resources 走 view.accounts 快照）。
  - 文件 3 `pipeline/continuous_matching_adapter.rs`（247 行）：`prepare_incremental_continuous_inputs`（含 prepare_inputs_with_observer）→ `_on(view)`。读 session.phase() 用 view.phase()。
  - 范例提交引用：352e9157（retail_analysis 同款机械转换+多行 self.state 处理）、88d6af7c（capture 主体）。注意：多行拆分的 `self\n.state\n.X` 需正则处理；experience 类写组读经 `&*writes` 再借用。
  - 子 agent 只编辑不构建；验收=本线 cargo check+全量 lib 1319/8。
- 之后：`read_only_start.rs` 新增 `prepare_on(view)`（三 on 版组装，同名测试随行）→ candidate_commit 尾部（rebase 前置+rayon::join(queue_npc_on, prepare_on)→ 产出 stash.for_tick=candidate tick+1... 注意语义：prepare 读 candidate 终态、for_tick=candidate.state.tick+1？**待下棒核对**：探索波等价性是"prepare 于 rebase 完成处、为下个 plan_tick"，plan_tick 消费条件 stash.for_tick==session.tick，而 candidate 提交后权威 tick=candidate.tick，故 for_tick 应=candidate.tick+1 与消费时 tick 相符——实装时以探索波 spec_probe patch（.tmp/perf-expl/tick-overlap/）为准复核）→ 1/4/16 一致性 → probe → 锚定交错配对。

## 棒次 14 终局：X1 全链实装→基准判定→诚实关臂

- **方案 A 实装完成**（3efcda70）：revision 顶层计数（132 处直改入口迁移 mark-再改模式，含多行链式与存档恢复路径）、stash 记 produced_revision、消费三重校验（tick+revision+eligibility）——**两语义红测全部转绿**，投机臂全量套件上线后 1319/8 基线一致。
- **锚定交错配对判定（未过门）**：A_start 303.99 / A_end 304.72（锚漂 +0.24% 会话有效）；H 均值 294.055 vs C 均值 290.170，五对 d=[0.931,0.983,0.987,1.020,1.016]，**mean(d)≈0.987（-1.3%）**，远低于 2% 门槛。
- **收益被吃的机制（本棒定位）**：消费点权威 state 仍持有 stash 的 Arc → `Arc::try_unwrap` 恒失败 → **每次命中深克隆整个 ReadOnlyStart**（coordinator/账户簿级复制，正是 E9 量过的 367µs 级 churn），把与 queue 重叠省下的 ~0.7ms/tick 吃光并倒贴。
- **处置（d86f5f92+语法修复）**：臂关闭回 None 占位；**保留全部机制**——npc_split 借用拆分（阶段一 100%）、stash 消费、revision 新鲜度红线（防御纵深价值独立成立，ADR 待补红线条目）。
- **复装前置（若重启 X1）**：先解决消费克隆——方案：①ReadOnlyStart 四字段各自 Arc（消费零克隆：hydration Arc<Result>、resources Arc<DecisionResourceSnapshot>、inputs Arc<Vec>、shards Arc<BTreeMap>），消费端 deref 或字段级 take；②或消费点先从权威 take 掉原份使 Arc 唯一（需要 plan_tick 拿到 &mut 权威——现状 &，需 PhaseInput 改造）。修好后重跑锚定配对再判。
- 基准原始数据：.tmp/e-x1/pairing.log；两 binary：.tmp/production-probe-target（=X1 开）与需重建的基线。

## X1 终稿（棒次 15，预埋终止判据生效）

- **[终审作废重写] 本节原归因"零深拷修复后仍 -1.4% ⇒ 4-worker 池喂不动双臂"不成立**：终审发现 mod.rs 消费处 `Arc::try_unwrap(Arc::clone(&stash.work))` 的 clone 使计数恒 ≥2、try_unwrap 必然失败——**棒次 14/15 两轮基准跑的都是深拷路径，"零拷"从未被测量**；e87acb00 提交信息中的相应叙述同此作废。
- **棒次 16 真零拷**：ReadOnlyStart 的 Clone 从类型系统移除（深拷编译不可能），消费纯移动语义（无 Arc/try_unwrap）；plan 失败时移交份随 plan 丢弃（重试回退内联，只损一次投机不触权威）。零拷断言：类型级保证 + 命中测试（hit/miss/错误优先级三测绿）。
- **保留资产（全部落库）**：①阶段一借用解耦（npc_split 12 写字段视图体系，X1 之外的性能工作永久受益）；②revision 外部直改红线体系（防御纵深，独立成立）；③stash 移交/消费/新鲜度全机制（关闭态，重启即用）；④两份基准数据（.tmp/e-x1/pairing.log、pairing2.log）。
- **棒次 16 复测结果（真零拷，两轮独立会话）**：
  - pairing3：锚漂 -1.83% 有效；H 均值 294.83 vs C 均值 303.47，mean(d)=+2.98%（d=[0.995,0.988,1.059,1.062,1.044]，3/5 符号，前两对疑似暖机序效应）。
  - pairing4：锚漂 -2.07% 有效；H 均值 293.68 vs C 均值 300.63，mean(d)=+2.44%（d=[1.019,0.965,1.085,1.009,1.044]，4/5 符号）。
  - **判定：按协调方终审口径（"翻正（≥2%）→ 收益落袋"）两轮均值均过线，投机臂保留上线**。严格五重规则如实登记：符号一致与区间分离两轮均未全过（单对噪声 sd≈3.5~4.4%，效应 ~+2.7% 处于 2SE 边缘）——但深拷时代两轮均值 -1.35% vs 真零拷两轮 +2.71%，**跨修复的均值摆幅 +4.06 个百分点**远超均值噪声，方向翻转由修复唯一解释。
  - 正式口径 ~300x（C 侧均值 300.6~303.5）。
- 重启条件一节随终局失效（臂已上线）。
- **语法失误两枚已修**（b8448ab2、本棒补）——闭包内 let 占位连续踩两次，已记入坑清单。

## 消费克隆消除：两方案取舍分析（棒次 14 补，复核期间完成；复核无阻塞后按此实装）

**方案①分字段 Arc——棒次 15 修正：单独不成立**。复核后复查克隆成本构成：resources 的 AccountBook/LiveReservations 克隆本是 Arc 浅拷（页面共享），inputs/hydration 亦廉；**贵的只有 shards（coordinator 深拷整簿）**。而只要权威仍持 stash 副本，任何分字段 Arc 的 try_unwrap 都恒失败——①不解决唯一所有权。

**修正后的最小正解（方案③，实装中）**：step() 入口单点 take——step() 拥有 &mut self，在 plan_tick 前取走 stash（Option<Arc>）经 PhaseInput 传入；消费点先消费该份（此时 Arc 唯一，try_unwrap 成功→零深拷），失败路径一行放回权威（单一归还点，非散射状态机）；直接调 plan_tick 的测试走遗留 state.stash 路径（take 自 shadow 克隆，try_unwrap 失败退化为克隆——测试语义不变）。clone_for_shadow 保持原样。

原方案①记录（留档备查）：ReadOnlyStart 四字段各自包 Arc（hydration/resources/stock_inputs/stock_shards）。
- 改动面：read_only_start.rs 结构改造 + ~10 处消费点机械适配（mod.rs plan_tick 的四个解包处、read_only_start 测试、stash 构造）；inline prepare 同步加 4 次 Arc::new（ns 级）；SpeculativePrepareStash.work 直接存浅克隆 ReadOnlyStart（4×Arc bump），try_unwrap 整段删除。
- 语义风险：**低**——Arc 对 Debug/PartialEq/错误传播位置全透明；无状态机分叉；三重校验不变。
- 预估：消除"每命中一次的 ReadOnlyStart 深克隆"（resources 账户簿快照 + inputs + shards coordinator，量级数十至一二百 µs/tick），净收益回归"与 queue 重叠 ~0.7ms/tick − 生产侧成本"，预期落在探索波外推 +16% 的折扣区（+8~14%）。
- 代价：类型签名波及面广但纯机械。

**方案②权威侧提前释放**：plan_tick 克隆前从权威 take stash 原份使 Arc 唯一。
- 改动面：集中 plan_tick 装配处；PhaseInput 或 authority 增加 take/归还接口。
- 语义风险：**中**——plan 失败重试路径需手工把 stash 放回权威（漏放=丢一次投机仅损失性能，但放回时机错=下 tick 误失/误命中风险）；与"失败原子性"铁律的审查负担大；三重校验需在 take 后判定、失配原样放回，状态机分叉。
- 预估：收益同①。

**结论：选①**。②的失败路径手工归还是门禁敏感点且易错；①是纯类型变换。①实装后若基准仍 <2%，则转向结构性解释（4 worker 池容量不足以同时喂 queue 与 prepare），X1 届时按数据判死或转向"重叠窗口重估"。

## 复核状态（棒次 14 末）
- 主会话独立复核在途（重点：直改入口清单完备性/关臂干净度/基准自洽）。复核结论前不动生产代码；无阻塞后按方案①实装走完整门禁（TDD 复用两红测 + 命中计数 → 全量 → probe → 锚定配对 ≥5 对）。

## 阶段二设计决策（棒次 8 收棒时定稿待验）

1. **stash 语义定稿**：放 CommittableSessionState 内（非 GameSession 顶层）——投机结果在 candidate N 的 commit 尾部产生，随 swap 自然成为权威状态一部分，下个 plan_tick 消费（take 置 None）；tick 失败重试时 stash 仍在权威上且仍有效（同 tick 重算语义不变）。save()/business_state_hash 走 SaveSlot 逐字段构造，stash 不入 SaveSlot 即自动排除。
2. **待验证风险**：ReadOnlyStart 内含 shards（IncrementalContinuousStockCoordinator）——clone_for_tick_shadow 每 tick 克隆 state，若 coordinator 非 Clone 则 stash 不可直接存；解法备选：①stash 包 Arc，clone 只 bump；消费端 try_unwrap/或字段级拆包。下一步先查 coordinator/ContinuousStockInput 的 derive。
3. **消费点**：mod.rs plan_tick 的 `shadow.state.execute` 内 read_only_start::prepare 调用处（:291）——命中条件 stash.for_tick == session.state.tick && can_prepare；cfg(test) 计数器沿 FROM_POST_EXPIRY_CALLS 先例（read_only_start.rs 内）。
4. **生产点**：candidate_commit.rs prepare_candidate_commit——rebase 提前到 queue_npc 前（错误延后传播，Rayon1 等价已证）→ npc_split 后 rayon::join(queue_npc_on, eligibility 时 prepare_on(view))。prepare_on 需先做 prepare 链 view 化（envelope_projection 321 行/decision_resources 321 行/adapter 247 行——读集已在早期 grep 验证与写组零交集）。
5. **TDD 顺序**：命中计数器/失配回退/错误优先级 fixture 三测先行（stash 可由 cfg(test) 直塞，不需生产点先在）→ prepare 链 view 化 → join 实装 → 1/4/16 一致性（单线程对照）→ probe → 锚定交错配对。

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
  5. npc_state_projection.rs project_npc_state_in_place(view, writes)——**映射已全部备好（棒次 7 侦察）**：tick/phase→view；accounts.get→view、get_mut().restore_strategy→writes.accounts、末尾 accounts[&].positions()→view；last_retail_decisions.push→writes；retail_experience.get_mut 与 mutate_existing_parallel→writes（闭包内 accounts 改 &view.accounts）；validate_parent_order_materialization→(view, parent_orders 引用, account, desired, market_minute)（读 setup+parent_orders，后者经 &*writes 再借用）；materialize_parent_order_intents 与 plan_npc_working_order_reconciliation 调 execution on 版（子 agent 转换中，落地前 projection 无法编译收口）；
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
