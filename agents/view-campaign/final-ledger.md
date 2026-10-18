# 视图内生战役收官台账（Sol 战役复核备料，2026-10-19 晨）

> worktree `.worktree/view-campaign`，分支 exp/view-internalization。
> 主线方向："裸引擎疯狂跑，分布式维护 view，外部直接观测，零会话层税"。
> 本文件=战役收官终数与可删项实录——供 Sol 战役级复核。

## 一、终数表

| 口径 | 战役基线 | 终数 | 提升 | 备注 |
|---|---|---|---|---|
| **A 3000（G=10）绑核静置** | 544.7 t/s 同源起点（A 基线 774.7=Arc 优化后） | **1011.8 mean**（948.3/1132.1/954.9） | vs 774.7 **+30.6%**；vs 批 2 912.7 +10.8% | **tag perf/bare-1000x-viewcampaign**；单轮 1132.1 破 1100 |
| **B 3000（G=1）静置** | 544.7（1000 tick 口径） | **1032.7 mean**（刀 2b 前）→**刀 2b 后待静置窗补正式**（配对 1087.1/单轮 1165.7/稳态画像 863.1μs→~1159 极限） | 稳态对照折算 **+56%→+70% 量级** | **tag perf/fullsem-1000x-viewcampaign**（含口径勘误） |
| 段级战果 | — | candle_install 41.13→0.335μs；envelope 80.5→2.7μs；attention_heap 258→23μs；belief_collect 归零；Seal 相位 206→139μs；组端 Seal 处理 max 154→33-37μs | — | 证据散见 evidence/p2/ |

测量口径协议（协调者裁决）：B 双报——3000 tick 主口径+短局冷启动
附注（一次性 X≈330ms=27%@1000 tick）；A=绑核形态优先（taskset -c 0-31
噪声隔离，刀 C 结论）。

## 二、可删六项实录（D1-D6 逐项勾销）

| 项 | 内容 | 状态 | 实录 |
|---|---|---|---|
| D1 | belief/账户/计划独立 Echo 往返 | **全退役** | belief 稀疏分片随 GroupReport 上行（P2-2，belief_collect 段归零）；账户分片随 SealAck（Q2 批 3，D2 退役后稀疏分片即终态）；EchoPlans 残余=plans_echo 16.6μs（⑤刀登记：dirty 往返与"空簿不覆盖"语义兼容已论证） |
| D2 | 账户投影双写 | **类型级删除** | unsettled 字段类型级删除（Q2 批 4，5ebd0a4b）；组端权威→发布版本单源 |
| D3 | 同拍投影 | **退役** | 同拍成交投影随 Seal 收敛（Q2 批 4）；fill_projection 段恒 0 |
| D4 | candle/行情安装往返 | **全退役** | install 读发布版本（candle 批 3991650f——段 41.13→0.335μs）；EchoCandles 生产往返退役（对账面 pub(crate) 保留+零调用登记） |
| D6a | attention heap 每拍全名册重建 | **惰性化** | P2-1 dirty 惰性重建（258→23μs）；残余=每拍真变重建（⑤刀：延迟删除增量模式已论证） |
| D6b | envelope 每拍全量重建 | **快照化** | P2-4 惰性派生（80.5→2.7μs）+resting 快照 Arc（刀 2b） |
| （envelope collect） | DeskResting 每拍往返 | **随报告上行** | P2-3 观察面随 DeskReport（刀 2b Arc 直通） |

## 三、架构终态

- **发布协议**（Q1）：install_tick 单点 publish（Arc<PublishedTick>）；
  desks[].view=Arc<StockPublic>（刀 2b）/resting=Arc 分片（Q2 批 3+
  刀 2b）/groups belief 稀疏分片/events 段/events hash。
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

## 五、复核档索引

reviews/：independent-review-r19/r20/candle/q2/sealtop/b1-spin（无阻断
贯穿；登记级建议全部处置或登记）；刀 2a/2b/③ 批复核（independent-
review-knife2.txt——归来后补录结论）。

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
