# 多 tick 会话接续设计盘点（非首 tick 门禁移除）

> 2026-10-15 夜·第 7 任 agent，Sol 三审等待期产出。只读盘点+设计，未动代码。
> 实测基线：P2 整改后（HEAD 54c42c14）ENGINE_STEP_RUNTIME=1 全量 lib =
> **1457 通过 / 101 失败 / 11 忽略**（翻转批起点 106，P2 净消 5：
> failure_tests 3 + hash_contract 1 + npc_working_quote 1）。
> 失败首因抽样：**35/101 的 panic 直接是"仅支持首 tick"门禁**
>（`.tmp/multitick-flip-detail.txt`，多为第二次 step() 的 unwrap）；
> 其余 66 项首步即失败于域断言（非门禁）。

## 1. RuntimeDriver 挂进 session 的形态

**字段位置**：`GameSession` 顶层加 `runtime_driver: Option<RuntimeDriver>`
（与 `compute`/`ingress` 同级），**不进** `CommittableSessionState`——
线程句柄不是可提交状态。

已核实的三个交互点（均有现成缝隙，无需新抽象）：

- `clone_for_tick_shadow`（session.rs:1742）逐字段构造 shadow：新字段写
  `runtime_driver: None`（shadow 只做 civil 投影，从不 step）。
- `commit_tick_shadow`（session.rs:1767）**只换 `ingress` 和 `state`**——
  driver 字段天然穿越日结不被 shadow 的 None 覆盖。
- 序列化：`save()` 走 `CommittableSessionState` 捕获，driver 天然排除；
  恢复侧惰性重建（见 §5）。

**生命周期**：
- 首次 `step_via_runtime` 惰性构造（此刻 session.tick==0，市场=初始，
  投影无损）；构造后常驻，后续 step 复用（world tick 与 session.tick
  锁步推进）。
- 失败/poison：World Abort 语义下 worker 存活，driver 保留（可重跑）。
- Drop 即 `World` 句柄回收（线程随 shutdown/drop 退出——需确认 Drop
  路径不泄漏线程，见 §5 风险 5）。

**与旧管线互斥**：driver 存在 ⇒ `step_frame_and_commit`/
`step_on_context`（旧管线帧路径）显式拒绝（双权威=静默语义丢失）。
反向：旧管线已 step 过的会话（tick>0）构造 driver 需 §2 的状态回填，
首版直接拒绝（保持现门禁的严格子集）。

**门禁替换**：`tick != 0` 检查改为锁步不变量
`driver.world_committed_tick() == session.tick()`（失配=显式
InvariantViolation，不静默重放）。恢复场景的 tick 对齐需要
`WorldSpec` 增加 `initial_committed_tick`（world.rs:541
`tick = committed_tick + 1` 从 0 起——恢复到 tick T 的会话，world 须从
T 起步才能相位正确；约 3 行改动，desk/组相位全部由绝对 tick 推导，
无需其他改动）。

## 2. 每步重建 World 丢了什么（逐项，对照 from_session 实读）

| # | 状态 | 现重建行为 | 丢失后果 | 101 清单对应 |
|---|---|---|---|---|
| 1 | world tick/日计数 | 每步回 tick 1 | 相位全错（每步都是竞价窗）——**门禁根因** | 35 项门禁首因 |
| 2 | desk 行情态（last/昨收/高低/量额/K/分钟/recent） | 回 setup 初始价 | 价格轨迹归零；历史/涨跌停基准错 | t3/t4/authoritative_tick 部分 |
| 3 | 订单簿（挂单/自成交索引/next_serial） | 清空 | 跨步挂单全丢；**OrderId 跨步重号** | quote_expiry/plan_owner/book_auction |
| 4 | T+1 frozen_today / foreign_active | 投影只搬 cash/shares | 日内冻结/在途登记丢失 | authoritative_tick（现金校验类） |
| 5 | fee_sink 日内累计 | 清零 | 费用审计断裂 | persistence/saved_runtime 3 项 |
| 6 | 策略参数/决策链（session.plans 等） | 合成 retail(0.30,200,0.35) | 决策链/机构行为完全不在 runtime | decision_chain 28+1 项（最大块） |
| 7 | attention（next_due/RNG） | **P2 已修复**：从会话恢复 | 已闭合 | attention 3 项待重测（可能仅剩语义差） |
| 8 | 事件 seq | **已闭合**（adopt_session_seq） | — | — |
| 9 | civil/公司/披露 | session 权威，不在 world | 不丢（end_civil_day 常驻） | epoch_alignment 2 项是恢复口径差 |

## 3. 门禁移除后的解锁估计

- **直接解锁上界 = 35**（实测门禁首因数）。其中一部分解锁后会在下一层
  断言再红（需 t3 探针/事件面等价跟上——step_via_runtime 已有
  queue_attempt_seq 探针，t3 类有戏）。
- **剩余 ≥66 是真实域迁移**（首步即失败，与门禁无关）：
  decision_chain 29、executor_perturbation 10、t3_boundary 10、
  authoritative_tick 9、phase_timing 7、quote_expiry 5、plan_owner 4、
  shared_ingress 3、npc_tick_prep 3、persistence 3、attention 3、
  其余 ~9。
- 结论：**D0（常驻 driver）是必要非充分**——解锁 ≤35/101，主战场仍在
  域迁移（决策链 29 是单块最大）。

## 4. D 批计划 v3（老 v2 建立在已作废归因上，全部重排）

- **D0（新增，先行）**：常驻 driver（§1 形态）+ 锁步不变量 +
  `initial_committed_tick` + #3/#4 状态保住（持久 driver 天然保住
  簿/冻结/serial——不需要投影往返）+ 行情态以 world 为权威、会话侧
  只留安装镜像（现 install_market_views 已每步回写，方向正确）。
  验收：35 项门禁首因逐项重跑分类（门禁消失后按新首因归档）。
- **D1**：决策链/计划域（29 项，单块最大；runtime 侧接 PlanChain 或
  会话侧翻译，二选一先做对照实验拿数据）。
- **D2**：envelope/ingress/回执/到期（quote_expiry 5+shared_ingress 3+
  ready 1+book_auction 1+commit_evidence 1 ≈ 11 项——会话↔runtime
  边界事件面）。
- **D3**：candidate/committable 语义翻译（committable 2+authoritative
  大部+plan_owner 4+npc_tick_prep 3 ≈ 15 项——克隆机制锚→Abort 等价）。
- **D4**：执行器绑定/相位计时（executor_perturbation 10+phase_timing
  7+t3 10+t4 2 ≈ 29 项——driver.step 内装 thread-local 绑定）。
- **D5**：存档/恢复内容（persistence 3+epoch 2+history_reads 1+
  continuous_tick 1 ≈ 7 项，依赖 D0-D3）。
- 顺序：D0 → D2（边界面小、快赢）→ D4 → D3 → D1 → D5。
  每批仍按"进程级对照判型 → 真实差异修 runtime 并登记"。

## 5. 风险清单（持久 driver 交互点）

1. **日结交互**：`day_end_with_runtime_market_day` 在常驻 driver 上语义
   已验证（P2 修复含 Abort 全量回滚）；注意 run_ticks 的自动日结
   （world.rs:544，`(tick-1)%tpd==0` 自动 run_day_end）与 driver 编排
   日结**双触发**——常驻形态必须二选一（建议：world 自动日结保持，
   driver.day_end* 只做 civil 桥接，需对表）。
2. **存档/恢复**：driver 不可序列化——恢复=重建。重建时 §2 #2/#3/#4
   全部回到"投影缺失"问题。缓解：恢复点若是日界静点（quiet point，
   旧管线已有该契约），簿为空、冻结已解冻 ⇒ 重建无损；**盘中存档**
   会丢挂单——要么显式拒绝盘中存档、要么做簿投影（登记决策）。
3. **线程生命周期**：World 的组/部线程跨 step 常驻 ⇒ 测试进程线程数
   上升（每会话 ~1+n_stocks 线程）； Drop 路径必须发 Shutdown 否则
   线程泄漏（现 World 无 Drop impl 依赖显式 shutdown——需补 Drop 兜底
   或文档化契约）。
4. **双权威**：§1 互斥不变量必须显式（旧帧路径拒绝），否则混用产生
   不可解释行为（铁律二）。
5. **t3/相位计时探针**：常驻 driver 每步在驱动器线程跑——thread-local
   绑定（executor controls/phase timing）的宿主线程假设会变（D4 域的
   根因就在这，D0 不解决但不要恶化）。
6. **ENGINE_STEP_RUNTIME 缺省翻转时机**：门禁移除 ≠ 可翻缺省——按
   101→0 的实测进度决定，翻转批"最后一步"约定不变。

---

# D0 实施结果（2026-10-15，HEAD 提交见 git log）

- 挂载：`GameSession.runtime_driver: Option<Mutex<RuntimeDriver>>`（Mutex
  保 GameSession:Sync——旧管线多线程测试依赖）；shadow 置 None；
  commit_tick_shadow 只换 ingress/state ⇒ 常驻实例穿越日结。
- 门禁移除 → 锁步不变量 `world_committed_tick()==session.tick()`
  （失配显式 poison）；WorldSpec.initial_committed_tick 贯通
  GlobalState/GroupState/DeskSetup（组/部 Start/Ops 连续性按绝对 tick）。
- 双日结显式互斥：last_day_end_tick 守卫（重复手动=显式 Err、自动
  跳过、abort 回滚清空）+ Drop 兜底关停（发 Shutdown+join）。
- 晚失败注入同判：runtime 路径在等价缝（step 后、安装前）消费
  post_shadow_failure——原被门禁掩盖的两路径不同判。
- **实测**：env=1 全量 101→85（±1 npc_decisions 负载 flake）；
  门禁消息归零；35 门禁型中 16 项转绿（直解 11+注入契约连锁 5），
  其余 19 项换至下一层域断言（decision_chain/executor/phase_timing
  /t3/quote_expiry/plan_owner 等——即 D1-D5 范围）。默认 lib
  1563/0/11（两轮稳定）；翻转批 1-5 全绿。
- 每步重建丢失清单（§2 #1-#5）由常驻天然保住；#6（决策链）仍是
  D1 域迁移工作。

---

# D3 执行记录（A 方向：深移植）

## 拍板（2026-10-15，协调人转述用户既定模型）
- **A（深移植）**：计划状态机迁入组线程 PlanChain。依据=用户 std-thread
  模型拍板（决策内联组线程+所有权转移；tick=决策∥→撮合∥+commit 前同步）。
- **B（会话权威+意向上行/裁决/下插）否决**：三段跨线程往返在 128 核尺度
  回协调瓶颈——正是用户否决 rayon 模型时点名的病根。

## 依赖序勘正（D3a 实测发现）
开仓/复审路径的 PlanOpen 字段（opinion/confidence/urgency）由链观察
（price_paths/技术面/信念态）派生——组线程无观察基则字段必造假。
故 D3a-1 先下沉**观察基**：
- 组线程逐 tick 累积每股最新价路径（快照 last_price；容量 512 滚动）。
- 读面：GroupIn::EchoObservations → UpMsg::GroupObservations →
  RuntimeDriver::collect_observations（D4 读面投影模式）。
- 红证：移除累积→路径 0 条红；恢复→逐 tick 累积绿（实测 5 tick 5 条）。

## 后续子批（B 方向否决理由已入台账）
- D3a-2：技术面/信念个人态下沉 → 开仓路径（PlanOpen 字段同源派生）。
- D3a-3：加仓/撤销路径 + remaining 校验。
- D3b：复审/风险暂停/紧迫度。
- D3c：plans_debug 等会话读面投影组线程计划事实（交付点记录）。

---

# D3a-2b 依赖分析与不可分性发现（2026-10-15）

BeliefParticipantState = {PersonalWatchlist, PersonalPriceMemory,
NpcInformationState, BeliefBook}（全 crate 纯类型，Clone——组线程可持）。
但**生命周期驱动面**才是移植主体：
- information 更新：披露/出版物经 deliver_public_information →
  install_information_update（session 侧信息域每 tick 驱动）；
- watchlist：观察接受时 record_attention；prune_memory 按保护集清理；
- belief 条目：自 information 构建（entry 方法/信号源）→ opinion 输入。

**不可分性**：信念权威必须整体迁移（组持态+信息驱动+读面投影）。
若只搬快照而 session 继续每 tick 更新自己的副本，组态与 session 态
立即漂移；若每 tick 从 session 刷新组态，等价于 B 方向的跨线程状态
往返（已拍板否决）。 ⇒ D3a-2b 最小完整件 = 信念权威迁移 +
information 驱动面迁移 + 会话读面投影（D4 模式）。
预估与 D3a-2 同量级（信息域读面+驱动+测试）。

分片（下一步执行）：
1. Belief-1：组线程持 BeliefParticipantState（构造投影自 session 一次）
   + information 驱动下沉（披露→组内 information 更新——披露事实经
   快照/广播下行）+ 读面同源校验（组 belief/watchlist==session 静止态，
   即构造投影等价）。
2. Belief-2：watchlist/price_memory 生命周期（attention/prune）+
   belief 条目构建 —— belief 动态更新后同源校验改为"信息注入后组内
   自演进==session 版同输入自演进"（同输入同态对比，非共享态）。
3. D3a-2c：PlanOpen 开仓路径（opinion/confidence 自组内观察+belief
   派生——字段同源）。

## D3a-2b Belief-1 落地（2026-10-15）
- 组线程持 BeliefParticipantSnapshot（watchlist/价格记忆/信息态/信念书
  ——crate 纯类型直载）；构造期自 session 一次性投影（id_map 序，
  None=非机构位）；EchoBelief→GroupBelief 读面。
- 同源校验（day 1 内、3 机构 fixture）：组态==session 投影逐字段相等；
  红证=投影禁用→(None, Some(完整态)) 红。
- **登记**：跨日披露使 session 侧 belief 演进、组侧静止——分歧窗口
  存在至 Belief-1b（披露驱动下沉：deliver_public_information 的
  per-NPC 更新提取为纯函数 + 披露事实经广播下行）。Belief-1b 前
  组信念态无消费者（安全暂存）。

## Belief-1b·下 权威调和注记（2026-10-15，回应协调人对清）
P1-10 拍板：**组线程=attention 调度权威**（next_due 演进在组内 decide；
session.npc_attention.candidate 为镜像，每 tick 经 collect→
set_runtime_attention 同步）。本件措辞"注意力权威留会话侧"**勘正**：
- NpcAttentionState 两类域：调度域（next_attention_candidate_tick）
  =组权威/P1-10 链路不变；信息检查节奏域（information_cadence/
  next_information_check）=本件下沉对象（此前只在 session 侧被
  deliver 演进，组从未触碰——不构成既有权威冲突）。
- record_information_check 不碰 candidate——**域不相交**，投递计算
  下沉不触碰调度权威链。
- 下行快照取点=**上一 tick install 同步后的 session 镜像**（candidate
  域即组权威值——非会话独立演进副本）；上行回写**只写 information
  域**（新方法），不覆写 candidate/heap——双权威漂移不可能。
- tick 内时序：decide（组 next_due 演进）→ 收束 → install（candidate
  同步）→ [日结披露发生→驱动器组装快照（此刻镜像）→下行→组内纯
  函数→上行→仅 information 域回写]。红证锚：投递前后各 NPC 的
  candidate 域必须不变（快照取点错/回写覆写即红）。
- session 侧 deliver_public_information 在 runtime 驱动器在位时停跑
  （入口分流）——旧管线（无驱动器）路径不变。

## Belief-1b·下 落地（2026-10-15）
- 稀疏投递：驱动器按库版本（PublicLibrary::version=next_seq）判据，
  仅变化时下行 GroupInformationPayload（库 Arc+注意力镜像快照+issuer/
  spec 快照）；组内 notices::deliver_information_for_npc 纯函数推进；
  回写 set_runtime_information_checks **仅 information 域**。
- session 侧 deliver_public_information 在 runtime 驱动器在位时停跑
  （分流，旧管线不变）——双权威漂移不可能。
- 红证：驱动侧丢弃组产物→"两机构位都应推进(0)"红（机制红 ✓）。
- **如实登记**：candidate 保持锚（覆写实验）未红——顺序流下快照与
  回写间无漂移窗口（纯函数不碰 candidate，回写即同值）；锚保留为
  回归守卫，其真正防护对象=未来投递跨 tick 化时的取点错误。
- 差异登记：组侧 codes=当前持仓∪watchlist（组账户无"曾持有"零量
  条目面——session positions 含之）；影响=D3a-2c 前评估。

## D3a-2c 开仓路径移植图（2026-10-15，依赖判明）
核心派生=InstitutionDecisionRoot::new(account, personal)
  .observe(&RootReadContext, &observations)（roots.rs）——外围
prepare/dispatch 全是 rayon 时代调度+测试探针（组线程模型下任务即
线程，直调即可）。

**协调人预警命中**：institution_behavior 消费
`accounts[&id].positions().get(code).cost_price()`——组账户无成本基
（D1 登记"成本基保留会话侧"）⇒ **必须收口**（派生需要真值，无登记
豁免）：GroupAccount 增 invested/recovered 每股账，由 DeskBatch 的
FillFact（含 price）在实体落账时维护（买入 invested+=price×qty、
卖出 recovered+=price×qty——对齐 Account::apply_buy/apply_sell 口径）；
构造投影自 session positions；会话回写同步回（D1 的"成本基保留"
差异随之闭合）。

移植五件：
1. GroupAccount 成本基（FillFact 落账维护+构造投影+回写同步）；
2. RootReadContext 组侧组装（AccountBook 等价=组账户+成本基；
   PlanBook 组内起始空——runtime 模式 session plans 恒空天然一致；
   CompanyRegistry/Operations/StockCatalog 构造期 Arc 投影）；
3. InstitutionDecisionRoot/observe 提 pub(crate)（roots.rs +
   personal_state take/install 的组内等价——组 belief_states 直接
   承载）；
4. 开仓产物（plan ops→组内 PlanBook+子单 PendingOp）；
5. plans_debug 会话读面投影（D4 模式：交付点记录）。
红证：同输入（同观察+同 belief+同成本基）组内派生==session 版
派生逐字段（InstitutionBehavior/候选集/PlanOpen 字段）。
预估 3-5 小时专注块——独立会话执行（马拉松末段不做终局语义件）。


## D3a-2c 落地记录（2026-10-15，第 8 任，HEAD=0d21521b）

五件提交（6a5ed768/2ca76d31/ccc58cb6/e7814728/0d21521b）：
- **件1 成本基**（6a5ed768）：GroupAccount invested/recovered（仅成交额，
  FillFact 逐笔+清仓清零+GroupUndo 冲回）；WorldSpec 构造投影；回写组权威
  （D1"成本基保留会话侧"差异闭合）。红证=脚本直连落账/清仓/Abort+
  driver 端到端（desk_trades 买腿独立推算+回写==组权威）；机制红=+1 破坏。
- **件2/3 组装+直调**（2ca76d31）：observe/RootReadContext/PlanPersonalState
  等提 pub(crate)；组线程 build_root_read_context（组账户→AccountBook 保真
  投影+构造期 Arc+快照时间面）；intraday_observation_second 秒映射提取共享；
  **红证抓到真实分叉**：from_session 漏设 spec.library（组内空库→belief
  条目不创建）——修复后组内派生==session 版逐字段（assessments+个人态六件）。
- **件4a 开仓落地**（ccc58cb6）：assess_one 无计划分支提取共享纯函数
  open_position_action（session 改调，行为保持）；组内 Lifecycle 消费→
  PlanOpen 落组内 PlanBook（GroupState.plan_book）；红证=组内 opens==
  session 版 collect 的 Create 逐字段（take→observe→install→collect 语义序）。
- **件5 读面投影**（e7814728+0d21521b）：EchoPlans→GroupPlans→driver 交付点
  set_runtime_plans（X1）；**空簿不覆盖**（env=1 plan_owner 回归显红后收口
  ——迁移期 session 注入面保护）。
- **未做：件4b 报价子单**——AccountExecution/报价链（预算 allocate+
  decide_quote+SubmitLimit→PendingOp 并入 run_round）未提取；**due 驱动
  生产接线未做**（observe 直调目前仅 EchoOpenDerivation 读面；run_round
  轮 0 对 belief NPC 替换 decide_data 的生产路径+personal_out 写回
  belief_states/attention_states 是 D3a-3 第一件事）。

数字：默认 lib 1572/0/11（三轮；一轮 2 例 npc_decisions 族 flake 复现后
消失）；env=1 全量 **55**（与基线持平——开仓族测试的消化依赖 4b 子单+
due 注入通道，未落地前无新增绿）；翻转批 10/13/14/14/15 全绿。

登记差异清单：
1. 零量条目面：组侧无"曾持有"条目（qty=0 不建）；session positions 含之
   （runtime 回写保留）。qty=0 时 cost_price=None 派生等价；held 候选面
   理论可漂——红证时窗（day1 短跑）避开，收口排后续。
2. exposed_stocks=空集起步（公司披露曝光面——发现权重公告加成缺失；
   红证两侧同输入空集）。
3. DeskTradeFact maker/taker 语义备忘（desk.rs:1109 既有）：taker_side=Sell
   时 maker/taker 字段对调填入——两方向下统一为 maker≡卖方/taker≡买方。
4. Active 分支（既有计划复审/风险暂停）+quiet review 记录（plan_review_
   facts/resources）=D3b 暂缓；组内已活跃股不重复开仓（active-plan 分流）。
5. 开仓后立即 pause 判定（apply 的 urgency pause 段）组内暂缓——D3b。
6. BeliefParticipantSnapshot 已扩六件（+attention/history_reads——组内
   observe 权威承载）；Belief-1b 投递链路兼容未动。

- 复核收口（a27b0ad2）：独立 subagent 审查四项修复（交付投影吞错上抛/
  清仓清零 foreign_active 守卫/防御统一/相位简化）；登记未修：
  fundamental_ready 三处语义复制（后续提取）。


## D3a-3 落地记录（2026-10-15，第 8 任续，HEAD=f734cda4）

提交（3643ebaf 件4b + f734cda4 执行腿）：
- **件4b 生产接线**：run_round 轮 0 belief 位分流 run_chain_decision
  （observe 直调→组态写回[GroupUndo::Personal/PlanBook 可逆]→开仓→
  预算+报价→PendingOp）；报价决策段提取共享 decide_plan_quote
  （session generate_next_plan_quote 改调——band/cage/protection/
  affordable 一处收口）。
- **收口（红证显红驱动）**：runtime_personal_states 过滤面对齐能力探针
  （retail 误投影→observe panic）；**曾持有位 ever_held**（卖出扣除/
  清仓/构造投影三处置位——held 集等价：sample_discovery_stock 的持仓
  优先采样依赖零量条目面——原登记差异 #1 真实显红后收口）；qty 口径差
  （在途卖单 session qty=0 vs 旧管线在手+在途）登记 D3b urgency 域。
- **执行腿**：挂出 install_execution（不重复挂）+FillFact 推进
  （首见 adopt/ChildOrderFilled）。

**env=1=55 持平归因（协调人预警命中）**：decision_chain 27 未消化。
根因=**注入通道缺失**：chain_restructure_tests 的场景构造
（force_attention 改 session attention、seed_order_for_test 玩家挂单、
queue_npc_for_next_tick enqueue）在 runtime 模式不传导组内——组 due=
NpcProfile.next_due（P1-10 组权威），session 侧注入无下行通道。
产物面（plans 投影）已就绪——注入通道接上即应开始消化。这是
D3a-3 收尾的下一件（queue_npc 的 runtime 分流：驱动器通道下行
"强制到期"指令）。

数字：默认 lib 1574/0/11；env=1=55（名单与基线逐条一致）；翻转批
10/13/14/14/15 全绿。

**D3a-3 剩余（Sol 中点复核后）**：
1. 注入通道（decision_chain 消化前置——见归因）。
2. 撤销路径（chain 产 Cancel——D3b 终止信号联动）+腿清理（Closed
   归属 OrderRef——组内 Closed 实体不带单号，需映射或 desk 带）。
3. adopt qty 弱化与 remaining 校验收口。
4. 成交场景红证（fixture：belief 子单的对手成交）。
登记差异汇总（Sol 复核用）：零量条目面（已收口 ever_held）；exposed
空集；首子单 urgency=initial+pause=Unavailable（D3b）；frozen_cash=0
（所有权口径）；qty 口径差（在途卖单）；fundamental_ready 三处复制；
DeskTradeFact maker/taker 语义备忘（desk.rs:1109）。

- 复核收口（c903f63e）：五项修复（退款 undo 条件入账[高]/笼子信号/
  tick 投影/在途集 child_in_flight——加仓通路/测试名实）；env=1=55
  （56 为单轮 flake，复跑名单与基线一致）。


## Sol 中点整改落地（2026-10-15，第 8 任续，HEAD=798455d6）

裁决不通过（8 阻断）→ 五项最小修复集+注入通道全部落地（6 提交
adba7383/3e59c462/e97146fb/15b6044b/7e2c25a3/798455d6）：
1. Echo 只读+undo 完整性+state_hash（红证 solmid_echo_* 幂等）。
2. 真实受理身份（FillFact.order 合成 order_id_of——不另造计数器）+
   完整子单数量（ChildTicket.qty=挂出量——adopt qty 弱化阻断收口）+
   部分成交+终结全覆盖（Closed/到期/日终清簿/拒收→release_child_ticket）。
   独立预期红证：lifecycle（组簿 filled==desk_trades belief 腿独立源；
   修复中暴露独立源方向映射反与末批落账时序，均修）+day_end_drain。
3. 含在途资产读面（qty 口径差收口：投影/开仓/回写统一"在手+在途卖
   余量"；现金含在途买 purse；GroupAccount.pending_sell_qty 镜像）+
   预算 existing=本轮开仓前集合（NewOpportunity 可达）。
4. 完整计划投影（plan_ids 非空即装——terminal 覆盖 session 旧镜像；
   空簿保护保持）。
5. exposed 收口（exposed_stocks_shared 共享——组内不再空集）+4b 红证
   allocation_cash 独立推导（independent_allocation_cash——不反构造）。
红证据目录 .tmp/sol-mid-d3-red-evidence/（01-05）。

注入通道（裁决方向）：ForceDue 指令（cfg(test)+组验证+可逆）+三类
session 分流（force_attention/queue_npc/seed_order）+ChildAccepted
受理通道（受理即身份——生产通道）+个人态交付投影（observe 演进六件
回写 session 镜像）。

**重新归因（env=1=54）**：institution_observation 转绿（个人态投影
根修）；剩余 chain 族三类：
(a) seed 场景时序差一拍——external 单与 belief 决策同轮、决策快照
    （上一收束）无单→报价 Wait→"in-flight child"断言红。修向：组内
    Continue 轮报价触发面（受理下行驱动续行）或测试两 step 适配——
    **D3a-3 收尾首件**；
(b) retail_analysis 1 例——retail belief 链未迁移（D5 retail 域）；
(c) 复审族（SignalShift 撤单/加仓细节）=D3b 既有边界。
数字：默认 1578/0/11；翻转批 10/13/14/14/15；env=1=54。

## Sol 二审六项整改落地（2026-10-16，第 9 任，HEAD=531c6c15）

六提交（bd9c32af/…d2f67acf/a5a2a5d5/…f4e214f9/531c6c15+）逐项红绿：
1. existing 采集点移到开仓循环之前（run_chain_decision——此前循环后
   采集：同轮新开全成 ExistingPlan，NewOpportunity 不可达）。分类→
   优先级→资金可观察链由组内单测固化（sol1_quote_classification_*，
   双买计划现金恰够一条腿：正确输入挂股 0/错误输入挂股 1）。如实登记：
   调用点时序的端到端红需 observe 同轮开仓+竞争的 belief fixture（种子
   依赖），本轮以代码时序+分类器行为固化承载。
2. 在途票实时余量 remaining+累计成交额 filled_value_cents（fill 递减/
   累加，undo+state_hash 覆盖）；held 读面加回真实余量（红 left=200
   right=100）；买 purse 按余量+累计成交额边际费用口径（desk 同式保守
   下界，红 left=200502 right=100001）；equity/RootReadContext/开仓消费
   随两读面统一；release 镜像余量改票单一来源。
3. 受理事实逐轮下行（先于 Continue——此前只在终轮转发且终轮 evidence
   已不含早轮受理=永不传达）；fill 与票身份不符显式失败（此前以票身份
   推进外来单成交量）；Entity::Closed 带 desk 序号、按订单身份关联释放
   （他单 Closed/拒收不释放本票）。红：03a（时序锚）/03bc（组内直调）。
4. handle_day_end 清日志同步清 plan_undo_saved（此前日结相位簿变更不
   存新快照——Abort 恢复票不恢复执行腿）；ForceDue 延迟到下一 Start
   应用（归下一 tick 日志域：Abort 冲回强制前值、失效需重发）。红：
   04a（Abort 后簿/账户与日结前逐字段相等+重跑日结真正释放）、04
   （next_due 恢复 100+不重发则重跑不决策）。
5. 首步前注入适配（**构造期迁移**形态，非全局分流）：from_session 把
   session 旧簿在途挂单经 ExternalPlan 迁入世界（修"driver 首构造不迁
   入旧挂单"）；force_all_due 改传世界下一 tick（u64::MAX 被组端守卫
   钳成 no-op）。红：05（迁移禁用→首步无受理事件）、05b（回 MAX→
   零决策）。**否决记录**：按 env 全局分流的形态实测连坐旧管线 fixture
   112 例红（capture/seed/staged 面全依赖旧路副作用）——已回退；attention
   首步前经构造器镜像恢复天然生效。
6. 断言补强：d3a3 受理非空、lifecycle 部分成交余量证据；terminal 覆盖
   强化断言实测红后如实登记（belief 链 fixture 无法产生计划层 terminal
   ——完成需 filled==target 巨量、终止走 D3b 未迁移复审路径；子单层
   真实终结由 sol4/sol3 红证承载）。

数字：默认 lib 1588/0/11（watchdog 已知 flake ±1 复现后消失）；翻转批
1-5 全绿 10/13/14/14/15；env=1=54（基线 55，差= npc_decisions 已知
flake；无新增红）。

**重新归因（决策链族 28 例，带实验证据）**：注入已通（sol5 红/绿）、
计划开仓成立（场景断言 1 过）——阻断在**子单报价触发**：旧管线种子是
tick 前簿 fixture（首拍决策快照即含 best_bid）；runtime 迁移单在首拍
round 0 决策**之后**落簿，首拍报价 Wait（空对手档）。两 step 适配实验
仍红（重报价依赖 attention gap 的到期排程，第二拍未必 due）——修向：
fixture 多拍+逐拍 force（测试侧）或 Continue 轮报价触发面（受理下行
驱动续行报价）。复审族（撤销/加仓细节）另需 D3b 复审面迁移（session
侧 personal.install/commit_injected_plan_roots 被组投影逐步覆盖）。
retail_analysis 1 例=D5；其余为旧管线事件面专有（拒收回执事件/零量
诊断/staged 机制）。

红证据目录 .tmp/sol-mid-d3-red-evidence-r3/（02/03a/03bc/04/04a/05/
05b 红+绿，env1 基线/终态对照）。

## Sol 三审四项收尾（2026-10-16，第 9 任续，HEAD=b1c5abd9）

四提交（af1d3478 红证→ca96f449 实现 / 30d286b3 红证→d34fbc76 实现 /
63a44df6 断言+边界 / b1c5abd9 chain 适配）：
1. live purse 按 desk 实际占用：charged_cents=Σbuyer_charge（买腿成交额
   +边际佣金+过户费），读面加回=挂出全额 purse−charged（反例对账
   110001，红 left=899499 right=909499）；Abort 恢复全额；三层现金
   语义注记（可分配预算/总现金观察）。红绿=01-*（测试提交先于实现，
   SHA 可审计）。
2. 首步前 queue 接通：step_via_runtime 构造期消费 staged handover
   （intents 按产出序注入；attention 恰一次——produce 写回镜像即组
   next_due）。红绿=02-*。登记：消费拍不抑制——belief root 不在
   handover 且组簿无注入通道（D3b），抑制不交付使 belief 位双失
   （实测连坐 institution_observation 族后回退）；GameSession::new 清
   staged 槽（防串扰）——孪生消费须先于主 queue。
3. 断言收尾：部分成交绑同一子单（0<remaining<child_qty+desk_trades
   同键聚合下界匹配）；terminal 小型确定性 fixture（组内全额成交⇒
   Completed；session 旧 Active 镜像被 terminal 投影覆盖）。
4. 红证补齐：MANIFEST.md（逐份命令+SHA+内容形态如实标注；r3 突变型
   注明基准与突变，r4 起测试提交先于实现）；未受理票 Closed/拒收关联
   边界固化（实测语义正确）。
5. chain (a) 适配（裁定形态）：seeded_buy_plan fixture 多拍+逐拍 force
   ——chain_restructure 26 失败→12（14 转化；剩 12=D3b 复审/撤销面：
   session 按 id 撤单对 runtime 世界 OrderNotFound）。
数字：默认 1593/0/11；翻转批 10/13/14/14/15 全绿；env=1=40
（55→54(r3)→40(r4)）。

## Sol 四审四小步收尾（2026-10-16，第 9 任续，HEAD=c8b6c263）

1. 部分成交独立事实改**同键**（账户+股+方向）统计（此前 owner.1>=2 全
   belief 同方向累加、目标键只进报错文案）。
2. attention 恰一次**直接断言**：sol5th_queue_consumption_does_not_
   advance_attention（真实 due=1 边界：消费后组 next_due==queue 后镜像
   值——零推进；首步恰一次重排 due∈[2,6)；seed 恢复由既有 sol5 测试
   覆盖）。消费拍两面语义落字于消费器 doc（已交付 intents=执行不重放；
   未交付 belief root=保持自然调度不抑制）。
3. queue 红绿证据重出：同 fixture（twin 先行）同构造序，基准 b1c5abd9
   +突变"消费后丢弃注入"（原 30d286b3 红混入 GameSession::new 清槽
   原因且绿改序——不可认证同源，MANIFEST 已注明）；MANIFEST 三处失配
   修正（02-green 重出仅含存留测试、红描述改 0/1、r3 purse 红标注被
   01 取代）。
4. 清仓回写单列并修：qty（含在途余量）=0 即删条目（对齐旧管线 apply_
   sell 清仓删条目；此前清零保留——completed_sell 2053 红，03-flat
   红绿）。该测试残余 2057=**买腿成交落账一拍差**（runtime 协议批次
   次拍落账 vs 旧管线同拍入账）——独立登记，非清仓回写域、非复审/撤销
   面的第三类残余。
数字：默认 1594/0/11（npc_decisions 已知 flake ±1 复跑绿）；翻转批
10/13/14/14/15 全绿；env=1=40（名单与四审末一致；completed_sell 失败
点前移）。

## Sol 五审三项收尾（2026-10-16，第 9 任续，HEAD=7106eb79）

买腿落账差一拍裁为 D3a 必修等价性缺口（非登记差异）——修复：
1. **本拍公开期末投影**（apply_same_tick_fill_projection）：step() 返回
   前把本拍成交事实合入公开持仓/T+1/成本/现金/执行腿；组内 staging
   下一 Start 合并保留=双写（下一拍整值回写对齐，不双计）。公开现金=
   组现金+在途买占用（账户级 buy_purse_held_cents：挂出+purse/买腿
   −实付/CashRefund −退款——不限计划链票；undo+hash）；卖余量镜像改
   实体驱动（RefundedShares）。DeskTradeFact 增双腿实收费用分项。
2. **同拍全面验证**：sol6th_same_tick_settlement_full_projection
   （持仓/T+1/可卖/成本/现金/清仓条目——全部本拍断言不补拍；下一拍
   不双计）+ sol6th_same_tick_abort_leaks_nothing（失败拍零投影）。
   红绿=04-sametick-*（突变禁用投影：买家持仓条目缺失 panic）。
3. **证据收尾**：02 红态源码组成如实注明（c8b6c263+两项未提交测试
   编辑+突变=1605 vs 1604 的来源）；03-flat-green 重出（结算面
   2053/2057/2061-2067 全过；残余 2069=price_memory 裁剪——D3b 测试
   主题，非结算面）。
数字：默认 1596/0/11；翻转批 10/13/14/14/15 全绿；env=1=40（名单与
四审末一致——completed_sell 失败点已越过全部结算断言）。

## Sol 六审三项回归修复（2026-10-16，第 9 任续，HEAD=740e4b3e）

1. 费用归户跟归一化身份：maker_fees 恒卖腿/taker_fees 恒买腿（此前按
   原始 taker_side 交换——主动卖双方互换、本拍现金错）；fact 增双腿
   订单 desk 序号。
2. 执行腿投影按真实订单身份推进（合成同式比对+结果显式处理）；组内
   身份不符分支勘正为**跳过**（原 Err 形态杀死合法外来单的拍——
   sol7th_foreign 权威落账拍 WorkerFailed 实测红；03bc 测试更新为
   跳过语义，仍与"静默吃腿"缺陷形态可区分）。
3. 首买经历初始化恢复（同拍投影建条目前调 runtime_hold_experience_
   init——fault2038 面）。
附带：Abort 测试成交前提锚；三面自查矩阵（主动卖费用分户+被动买首持
+加仓+保存点×3）——sol7th_projection_matrix；外来同键守卫
sol7th_foreign（投影身份核的突变红在当前 fixture 不可判定——目标子单
被重报价致红绿同值，如实登记；组内跳过语义有实测红）。
数字：默认 1598/0/11；翻转批 10/13/14/14/15；env=1=40（名单同五审末）。

## Sol 七审四点收口（2026-10-16，第 9 任续，HEAD=85ce0ed7）

1. desk 双腿订单序号两方向统一 (sell_ref, buy_ref)（owner 已固定
   (seller,buyer)——按原始方向交换使主动卖时身份核跳过合法计划成交）。
2. buy_vwap 买腿识别改归一化身份（taker 恒买方——主动卖时原判把卖方
   当买方，真买方缺 VWAP 退用上一价）。
3. 测试面：矩阵补卖方下一拍净得/减仓+加仓后下一拍全值+两处新增
   restore；身份核 R1 增读 session 投影簿（plans_debug——被测对象）；
   真实 retail 首买测试 sol8th_retail_first_hold（清仓先行于构造+
   种子扫描卖方前提；fixture_clear_holding_for_test 双面清 stocks+
   feedback.stocks——守卫查后者）。
4. 证据重出：05-foreign-red=guard 回 Err 突变（WorkerFailed ticket=
   OrderId(10) fill=(0,15)——Err 杀合法外来单拍实证）；03bc-red=无
   守卫突变（余量 150 vs 200 被吃）。MANIFEST 更正。
汇报口径勘正登记：上轮"矩阵全覆盖"不实——断言到的面才列报。
数字：默认 1599/0/11；翻转批 10/13/14/14/15；env=1=40（名单同）。

## Sol 八审收官件（2026-10-16，第 9 任续，HEAD=a7d18a2f）

最后阻断收口：外来同键成交测试 R1 改为**直接断言 session 投影簿
filled_qty**（plans_debug 第 5 字段——此前只断 Active+余量读组权威簿，
误推进后照样 Active、下一拍被组权威抹掉，判别力为零）。红绿=
06-identity-*（投影身份 guard 移除突变：left=500 right=400——外来
100 股误推进直接显红；绿=完整态）。
数字：默认 1599/0/11（npc_decisions 已知 flake ±1 复跑绿）；翻转批
10/13/14/14/15；env=1=40（名单与五审末一致——本轮 diff 出的
npc_decisions_reports_first 为同族 flake，复跑绿）。
D3a 六项+三审四项+四审四项+五审三项+六审三项+七审四点+八审收官件
全部落地，待九审收官判定。

## D3b 开工（2026-10-16，第 9 任续，HEAD=f7f4c85a）

九审通过、D3a 闭合。D3b 件 1 落地：
- **个人记忆裁剪组内+同拍双写**：组内 apply_staging 对 fill 触达账户
  裁剪（保护集=在手∪在途卖∪活跃计划股——**不含**曾持有：session 清仓
  删条目等价，误入保护集使售出股永不裁剪；下一 Start 落账=组权威）；
  同拍 step 尾（个人态同步**之后**——整态覆盖会吞先行裁剪）经 session
  原语 runtime_prune_personal_memory（完成拍=末拍时同拍唯一可达；
  无个人态账户跳过）。completed_sell 全链通过（2069/2081 收口）——
  chain 族 26→13 转化累计。红绿=07-prune-*（红=env1 修复前真实失败
  9 vs 8；突变复核 fixture 判别力限制如实登记）。
- 数字：默认 1599/0/11；env=1=39（40−1）。

**D3b 剩余件（交接）**：
2. 复审路径移植：PlanLifecycleReview::collect（session decision_chain/
   lifecycle.rs——依赖面 held/equity/belief/chain_params/total_shares/
   trading_day/lot 组内均有等价）+ apply_plan_lifecycle_actions 的组内
   等价（ExecutionState/Revise/Terminate→plan_book.apply+record_review；
   review_facts/resources 组内近似面登记）。观察输出 operations 组内
   已持有（run_chain_decision 的 operations 变量）——只消费了
   lifecycle_assessments 的开仓面。
3. 撤销路由：DeskIn::Cancel{owner, order}（desk cancel_one 已有）+
   chain_quote_children 消费 QuoteAction::Cancel（order_id 解码
   serial→定向撤；票留待 Closed 释放——release 路径已就绪）。
4. 风险暂停/紧迫度：chain_quote_children 的 pause=Unavailable 收口
   （plan.status Paused reason→PauseAssessment；plan_execution_
   urgency_at_view 的组内等价——experience peak/equity/positions 组内
   全有；institution_behavior 依赖面大，分级移植）。
5. 注入通道：restructure 测试的 personal.install+commit_injected_
   plan_roots 需组内等价（GroupIn::InstallPersonal/ApplyPlanEvents
   cfg(test)——ForceDue 先例）；11 例测试 fixture 适配。

## D3b 件 2/3（2026-10-16，第 9 任续，HEAD=见 git log）

- 件 2 注入通道：GroupIn::ApplyPlanEvents（cfg(test)，组簿 apply
  权威+undo 可逆）；d3b2 红证（Paused 注入→投影可见）。
- 件 3 撤销路由：暂停/撤买前置分支（session quote.rs 头段等价——
  **共享 decide_plan_quote 不含该面**，组内自带）+定向 DeskIn::
  Cancel（serial 逆解；票待 Closed 释放）+撤买 Terminated；在途
  跳过收窄到 Active。d3b3 红证（事件 id 匹配+次拍身份清空）。
- 数字：默认 1601/0/11；env=1=39（名单同件 1 后——chain 11 例的
  复审/注入形态仍需件 4/5 消化）。
- 剩余：件 4 复审路径+紧迫度（PlanLifecycleReview+urgency_at_view
  移植——依赖 institution_behavior 链，最大件）；件 5 InstallPersonal
  通道+11 例 fixture 适配。

## D3b 件 5（2026-10-16，第 9 任续，HEAD=3d638cb2）

- InstallPersonal 通道（cfg(test)）：完整个人态快照下行组 0——组权威
  覆盖+Personal undo 可逆；缺信念态目标位显式失败。红证 d3b5_install
  （突变 no-op：watchlist 读面不可见）=09-install-*。
- assess_institution_behavior 提级 pub(crate)——件 4 复审移植的直接
  依赖（准纯函数：组内输入全有）。
- 台账补登记"共享面提取完整性"教训（件 3 的撤买分支发现）。
- 数字：默认 1602/0/11；env=1=39（持平——件 4/fixture 适配是消化面）。
- 剩余：件 4 复审路径（PlanLifecycleReview::collect 组内等价——
  urgency_at_view 依赖 experience peak/equity/cost positions/moment
  组内全有；价格路径=observation_inputs.minutes）+11 例 fixture
  适配（注入通道三件已就位：InstallPersonal/ApplyPlanEvents/
  force_due）。

## D3b 件 4 第一批：基建落地+接线暂缓（2026-10-16，HEAD=5a4040ca）

- **trait 化完成（行为保持）**：PlanLifecycleReview 与 session 解耦——
  PlanReviewHost 十一面（urgency_at_view/review_facts/resources/
  cancellable/initial_urgency/minute_closes/style/phase/policy/
  institution_behavior/max_holdable）；SessionReviewHost 全委托
  （1602/0/11 验证行为不变）；PlanLifecycleReview pub(crate) 化。
- **GroupReviewHost 起草完毕**：experience peak/成本基/在途权益
  （equity_cents_with_pending）/快照行情/投影策略面——零量条目过滤
  （RiskPositionInput qty>0 要求，调试发现）。
- **接线暂缓（如实登记）**：run_chain_decision 消费块实测回归 2 例：
  ①d3b3 暂停路由被复审撤买分支抢先终止（review 在 quote 前跑，Buy
  且 score≤0 → Terminate，Paused 计划到 quote 时已 terminal）；
  ②lifecycle 独立源对拍多出 (4,0) Buy filled=0 新计划——组内缺
  record_review/quiet 门控（session 每动作前 record_review + 比对
  last_review_*，组内未做→每拍修订、计划身份漂移/重复开仓面）。
  消费块已回退，基建保留 allow(dead_code)。
- **下轮接线前置**：apply_lifecycle_action 每动作前补
  plan_book.record_review（session apply_plan_lifecycle_actions 同式：
  review_facts+review_resources）——quiet 判定依赖簿内 last_review_*
  字段，是 ①②的共同根因。
- 数字：默认 1602/0/11；env=1=39（回归零残留——回退后复核）。

## 第 9 任收尾（2026-10-16）

D3a 九轮闭合（九审通过）；D3b 件 1/2/3/5 落地+件 4 基建（接线暂缓
——根因 record_review/quiet 门控缺失已定位，回归 2 例复现路径登记）。
最终数字：默认 1602/0/11；env=1=39；翻转批 10/13/14/14/15。
交接文件：.tmp/handoff-d3b-finish.md（件 4 校准接线/11 例 fixture
适配/D5 范围/已知坑）。第 10 任接手。
