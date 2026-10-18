# 视图内生战役收官台账（Sol 战役复核备料，2026-10-19 晨）

> worktree `.worktree/view-campaign`，分支 exp/view-internalization。
> 主线方向："裸引擎疯狂跑，分布式维护 view，外部直接观测，零会话层税"
>（**方向**而非已达成终态——残余镜像税见 §三，Sol 复核提示词同口径）。
> 本文件=战役收官终数与可删项实录——供 Sol 战役级复核。

## 一、终数表

| 口径 | 战役基线 | 终数 | 提升 | 备注 |
|---|---|---|---|---|
| **A 3000（G=10）绑核静置** | 1332.6 t/s（std-thread 战役终值近邻——**勘误**：初版误写 544.7，544.7 是 B 的同源起点；A 基线 774.7=Arc 优化后口径另计） | **1011.8 mean**（948.3/1132.1/954.9） | vs 774.7 **+30.6%**；vs 批 2 912.7 +10.8% | **tag perf/bare-1000x-viewcampaign**；单轮 1132.1 破 1100 |
| **B 3000（G=1）静置** | 544.7（1000 tick 口径） | **终数 1032.7 mean**（04:23 静窗，1007.2/1036.1/1054.9）；刀 2b 后双窗互证 1006.3（05:24 窗）+**单轮 1168.1 全程最高**；稳态画像 863.1μs→~1159 极限 | 稳态对照折算 **+56%→+70% 量级** | **tag perf/fullsem-1000x-viewcampaign**（含口径勘误）；证据 b-final-3000-spin2b.txt |
| 段级战果 | — | candle_install 41.13→0.335μs；envelope 80.5→2.7μs；attention_heap 258→23μs；belief_collect 归零；Seal 相位 206→139μs；组端 Seal 处理 max 154→33-37μs | — | 证据散见 evidence/p2/ |

测量口径协议（协调者裁决）：B 双报——3000 tick 主口径+短局冷启动
附注（一次性 X≈330ms=27%@1000 tick）；A=绑核形态优先（taskset -c 0-31
噪声隔离，刀 C 结论）。

## 二、可删六项实录（D1-D6 逐项勾销）

| 项 | 内容 | 状态 | 实录 |
|---|---|---|---|
| D1 | belief/账户/计划独立 Echo 往返 | **belief/账户退役；EchoPlans 生产往返仍在**（**勘误**：初版"全退役"过计） | belief 稀疏分片随 SealAck 上行（P2-2→第 22 任 P1-2 迁移，belief_collect 段归零）；账户分片随 SealAck（Q2 批 3）；EchoPlans 残余=plans_echo 16.6μs 生产往返（⑤刀登记：dirty 往返与"空簿不覆盖"语义兼容已论证——**未退役**） |
| D2 | 账户投影双写 | **类型级删除** | unsettled 字段类型级删除（Q2 批 4，5ebd0a4b）；组端权威→发布版本单源 |
| D3 | 同拍投影 | **退役** | 同拍成交投影随 Seal 收敛（Q2 批 4）；fill_projection 段恒 0（清仓经历面的同拍补偿随 P1-2 由 SealAck 分片承接——见 §七） |
| D4 | candle/行情安装往返 | **全退役** | install 读发布版本（candle 批 3991650f——段 41.13→0.335μs）；EchoCandles 生产往返退役（对账面 pub(crate) 保留+零调用登记） |
| D5 | retail 诊断面派生 | **部分落地**（**勘误**：初版缺行） | 拒收诊断 Rejected 单遍派发（P2-3c）+决策 trace 收集（D5 #4）落地；受理/成交/撤销诊断面"随事件域迁移逐步补"仍为登记未补（failure.rs D5 注释口径） |
| D6a | attention heap 每拍全名册重建 | **惰性化** | P2-1 dirty 惰性重建（258→23μs）；残余=每拍真变重建（⑤刀：延迟删除增量模式已论证） |
| D6b | envelope 每拍全量重建 | **快照化** | P2-4 惰性派生（80.5→2.7μs）+resting 快照 Arc（刀 2b） |
| （envelope collect） | DeskResting 每拍往返 | **随报告上行** | P2-3 观察面随 DeskReport（刀 2b Arc 直通） |

## 三、架构终态

- **发布协议**（Q1）：install_tick 单点 publish（Arc<PublishedTick>）；
  desks[].view=Arc<StockPublic>（刀 2b）/resting=Arc 分片（Q2 批 3+
  刀 2b）/groups belief 稀疏分片（**第 22 任 P1-2 起随 SealAck 上行**
  ——Seal 落账后构建，含清仓经历行）/events 段+events hash（**默认
  占位——事件组装仍在驱动器 step 内，R4 事件迁移时填充，非已完成**）。
- **提交语义**（Q2）：提交点=发布点=Seal（S1-S6+存活契约+Start 防御）；
  组端落账两相（eager 分域：即时相=Batch 到达/延迟相=消息点重放）；
  seal_acked_len 日结守卫；undo 跨发布冲回语义保持。
- **desk 投影**（刀 2a/2b）：book_ver/market_ver 内容版本缓存+Arc 全链
  直通（三处深拷贝退役）；"Seal 后 desk 静止"协议前置在案（升格触发
  条件登记）。
- **组线程等待**（B-1'）：worker_recv_spin 分级自旋（热态 500μs 窗+
  ENGINE_SPIN_DISABLE 逃生+ENGINE_SPIN_US 调档非法值显式 panic）；
  只组线程（desk 每拍必醒本就热态）。
- **组端账户**（Seal 攻顶 A-1）：Vec<Arc<GroupAccount>> COW（34 写点
  make_mut）+分片条目 Arc 共享+InlineVec 内联（7 Vec 域栈拷贝）。
- **驱动器残余镜像税**：belief 72.8（评估不做——大手术 5%）/retail 15.7/
  acct_writeback 14.3/acct_echo 12.6/desk_mirror 11.6。

## 四、tag 指针

- `perf/bare-1000x-viewcampaign`：A 1000 线（绑核静置 mean 1011.8）
- `perf/fullsem-1000x-viewcampaign`：B 1000 线（3000 tick 口径 1032.7
  ——含 1000 tick 口径 27% 冷启动税勘误声明）

## 五、复核档索引与 R19 登记三项处置结果（勘误补录）

reviews/：independent-review-r19/r20/candle/q2/sealtop/b1-spin（无阻断
贯穿；登记级建议全部处置或登记）；刀 2a/2b/③ 批复核（independent-
review-knife2.txt——归来后补录结论）。

R19 登记三项处置结果（**勘误补录**——初版未落处置去向）：
1. **旧管线形态 hash 双源对照守卫**（heap 派生 vs npc_attention 派生
   逐位一致）→ **未补，维持登记**（等价论证仍为注释级：heap=每账户恰
   一项的权威快照，排序后同序列；第 22 任 P2-4 已把无快照 hash 收口到
   实际 ledger，heap 双源守卫待旧管线退役前统一收口）。
2. **死字段清理**（runtime_heap_undo 槽恒 None+failure.rs 消费段
   no-op）→ **未清理，维持登记**（槽位仍在——clone/rollback 消费面
   保持结构，防扩散）。
3. **跨 Continue 多轮注入守卫**（经 ForceContinuationForTest 基建）
   → **已闭合**：solfinal1b_continue_early_round_first_buy_initializes_
   experience（Continue 早轮注入意图+经历初始化断言——基建已用上）。

## 六、登记项终态与升格触发点

1 落地（ensure_fact 零拷贝）+6 维持登记（InlineVec pub/GroupDayDone
O(N) tpd=1/首拍 force_full/Seal 后 desk 静止/迟到 AcceptedBatch/handle_
start 冗余——依据见 current-state.md"第 21 任：登记项收口"节）。
升格触发：engine 外部库消费（InlineVec 收口）；协议引入 Seal 后 desk
演进（静止守卫——**desk 投影进一步 dirty 化时此条为合同依据**）。

## 七、第 22 任接手面（⑤刀+尾巴）

1. **⑤安装链小刀**（协调者裁定顺手做，刀法已论证）：
   a. heap 延迟删除增量（22.9μs：push dirty 项新值+pop 时核验候选
      ——标准延迟删除模式；等价合同=pop_due 序不变）；
   b. plans dirty 往返（16.6μs：组端 plan_book dirty 标记+EchoPlans
      未变回空——与 drive"空簿不覆盖"语义兼容已论证；注意组端计划
      清空时 session 镜像现状也不清——等价）。
2. B 3000 静置正式数字（刀 2b 终态——本任配对 1087/单轮 1165 待静置
   正式落账）。
3. npc rayon 注入家族：高载窗（load 10-17）连发 4 轮——低载复证绿
   （1698/0/11）✓ 登记收口；CI 限核注意（ENGINE_SPIN_DISABLE=1）。
4. spread 残余 ~110μs（组 0 忙段）三选项——协调者裁定本里程碑收袋，
   B 冲高需要时回头。

## 八、Sol 战役复核提示词（草稿——协调者发）

> 你是战役级复核者（Sol）。对 worktree .worktree/view-campaign 分支
> exp/view-internalization 的视图内生战役（main..HEAD，约 60 提交，
> 2026-10-18 至 10-19）做收官复核。战役目标："裸引擎疯狂跑，分布式
> 维护 view，外部直接观测，零会话层税"——A/B 双口径 1000 t/s 双达标
> （tag perf/bare-1000x-viewcampaign / perf/fullsem-1000x-viewcampaign）。
> 复核判据：①战役主张与代码/证据一致（agents/view-campaign/
> final-ledger.md 的六项可删实录与终数表逐项核对 evidence/）②大 A
> 语义零触碰（交易制度面未被性能刀模糊）③发布协议/Seal 提交语义/
> 账户 COW/自旋等待四条主线的语义合同完备性（守卫红证链）④遗留登记
> 项与升格触发点的完备性⑤可合并性（无未守卫的语义漂移面）。产出：
> 【阻断】/【建议】/【确认】三段式报告至 reviews/sol-campaign-final.txt。

## 九、Sol 战役复核四阻断处置（第 22 任，2026-10-19）

Sol 一审判四阻断+台账勘误；第 22 任逐项修复（红证先行+突变红实证+全量
回归，一件一提交）：

| 阻断 | 处置 | 提交 |
|---|---|---|
| P1-1 在途快照生命周期（shadow 克隆/commit/回滚清空+日终装于 Drain 前——checkpoint→rollback→driver 重建读 stale ledger） | 快照保留（shadow 携带）/移交（commit_from）/恢复（回滚基线）+日终 live 重刷+runtime_envelope_ledger_snapshot 快照感知（重建种子读现势）。四红证：交接/回滚投影集/重建种子/日终空在途 | c253361a |
| P1-2 Seal 后清仓经历未入 belief 发布分片（分片构建早于 apply_deferred；同拍补偿已随 D3 删——fault2051 形态） | 分片随 SealAck 上行（apply_deferred 后构建——组权威单源四面同拍一致）。红证：跨日买建仓→全额卖出四面真对账（组权威/发布/session/save-restore 行级相等）；publication 测试升行级对账；突变红实证 | 08f07686 |
| P1-3 真实 Seal 超时违反存活合同（[Seal,Abort] 队列竞态判协议错位退出；迟到 Ack 污染收束） | Seal drain 的 Abort=合法竞态（回滚+存活）；注入面迁广播后（真实路径+部分 Ack 形态）；seal_round 重复 Ack 跳过；converge_round 迟到 Ack 跳过。红证：liveness 真实路径红→绿+部分 Ack 测试+converge 单测；drain 外来消息测试改真外来消息；突变红实证 | 3f45b4b3 |
| P2-4 无快照时业务 hash 漏真实 ledger | 统一"有快照派生、无快照读实际 ledger"合同（对齐 save 面先例）。红证：对照档设计（同档在途完整 vs 合法清空——JSON diff 证实其余域逐位同）hash 必异；突变红实证（恒派生→相等红） | 7a82ecc1 |

台账勘误同步落地：A 基线 1332.6（544.7 是 B）/D1 EchoPlans 生产往返
仍在/D5 补终态（部分落地）/events 段=默认占位/"零会话层税"改方向口径
/R19 登记三项补处置结果（§五）。
