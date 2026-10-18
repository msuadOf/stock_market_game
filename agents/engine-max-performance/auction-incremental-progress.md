# 拍卖增量复用实现进度档（F1 残余影子缓存）

## 诊断拆分（2026-10-07，.tmp/e-auction-impl/diag-split.json，engine example 292x 诊断，3000 tick）

600 拍卖 tick（全部为开盘竞价，3000 tick 内无收盘竞价）总 wall 1896ms：
- prep_gate（validate_live_orders）411ms——**红线保留全部检查**，不动
- prep_replay 144ms + prep_continuous 91ms + prep_assemble 91ms（开盘段 resting 为空，continuous 实际近零）
- shards_from_post_expiry 406ms（含 shadow_ledger_build 101ms）
- finish_process 416ms（AuctionTick 事件带 tick 号→输出不可跨 tick 缓存；不动）
- finish_sort 98ms（每 tick 向量小，边际；不动）
- apply_finish_total 112ms

## 定案设计：残余影子跨 tick 缓存（零拷贝所有权移交，沿 X1 stash 先例）

命中时消除 replay+continuous+assemble+from_post_expiry 全部（≈732ms，外推 +5~7%）：

1. `process_auction_stock` 重构：非完成 tick（finish_auction=false 且 finish_day=false）不销毁 (state, continuous_envelopes, market)，以 `AuctionResidual` 归还；完成路径照旧消费。
2. `AuctionStockShadow::finish` 非完成 tick 产出"下一 tick 起始影子"（created/receipts/terminal/facts 清空重置——它们已随输出发出；ledger 保留 rounds 应用后的全量 envelopes）。
3. `finish_auction_shards` 汇集 residual_shadows；`apply_incremental_auction_finish` 安装 `state.auction_shard_cache = Some({day, phase_kind, tick_after, revision, shadows})`（不进 hash/存档/shadow 携带，沿 speculative_prepare 先例逐点）。
4. 命中判定（全 exact，无指纹信任）：day 相同、phase 同类（Opening/Closing）、tick==tick_after+1、revision 相同（外部直改红线）、每股 authoritative auction_orders snaps == 残余 state.orders() 投影、authoritative resting (owner,id,side) 键 == 残余 continuous keys、股集 == markets 键集。任何失配整体回退全量（沿 read-only start 先例）。
5. 命中路径：照常跑门禁（validate_live_orders，行为不变）；跳过 prepare 装配与 from_post_expiry，coordinator 直接由残余 shadows 构造（等价于 fresh from_post_expiry：seen 集/计数器空、detached 空）。
6. 取消单：经 rounds 进 shadow state/ledger，残余自动反映；外部直改经 revision 失配回退。

## TDD 计划

红1：连续 2 个 Opening tick（只追加单）→ 第二个 tick 命中计数>0（cfg(test) 计数器）。
红2：命中 tick 的 step 输出事件与全量路径逐字节一致（对照跑：禁用缓存的开关或独立会话）。
红3：cancel 后前缀变化→失配回退（计数不增）；fixture_market_mut 外部直改→回退。

## 步骤与状态（棒次 1 中断存档，2026-10-07）

- [x] 拆分诊断（本档头部数据，diag-split.json）
- [x] **Cut A 已实现并实测**（死 ledger 守卫：finish_process 416→150ms，诊断 speed 292.4→297.3 = **+1.7% 单刀**，低于 2% 门槛须与 Cut B 合批判定）——补丁全文存 `.tmp/e-auction-impl/cuta-dead-ledger-guard.patch`（tracked 编辑被并行 agent 还原，补丁内容完整可重放，条件精确性论证在补丁注释）
- [x] Cut B 设计定稿（本档"定案设计"节）+ 关键代码侦察全部完成：
  - shadow.apply_operation 维护 self.ledger（insert_created/apply_local）✓ 残余影子可整体跨 tick
  - coordinator.finish 于 auction_day_end.rs:445（逐 shadow 串行 finish，收集 workers）✓
  - IncrementalAuctionFinish 定义于 :167；process_auction_stock 唯一调用方=shadow.finish（operations 恒空）✓
  - CommittableSessionState.speculative_prepare 的 6 处生命周期镜像点（session.rs:1522/1867/4669/4723/4772/4821 + hash.rs:87）✓
  - validate_live_orders 独立可调（门禁在命中路径照跑）✓
- [ ] Cut B 实现（process 解构+residual 归还 → shadow/coordinator/IncrementalAuctionFinish 水管 → state 字段+排除 → 命中接线 → TDD）

## ⚠️ 中断原因：同工作区并行 agent 冲突

任务清单出现 #22"并行化四路试验（逐股/克隆/提交链/粗化）"，与我在同一 worktree 活跃（发现时 5 个 cargo/rustc 进程、新目录 agents/day-end-storage/）。其"按 HEAD 原字节恢复无关文件"的清扫把我全部 tracked 在途编辑还原（Cut A 实现+Cut B 半成品+插桩），未跟踪产物幸存。**下一棒须先与协调方确认工作区独占**（或转投一次性 worktree），再重放 Cut A 补丁并继续 Cut B。

## 临时插桩重放说明

performance_profile.rs（12 span）+ examples/auction_split_diag.rs 仍在树上未跟踪；插桩点：stock_auction_adapter.rs（span 0-5）、auction_day_end.rs from_post_expiry×2/finish/process/apply_finish、auction_tick_transaction.rs（span 10）。lib.rs 的 `pub mod performance_profile;` 需重新加。

## 风险与红线

- 门禁行为零变化（gate 代码不动，只在命中路径跳过"装配"而非"检查"）。
- 失败原子性：残余安装发生在 apply_finish（commit 前、无失败点之后）——安装失败即 tick 失败，缓存不装。
- 首错序：命中不改变任何事件顺序（prepare 不产事件；from_post_expiry 不产事件）。
- 排序等价：残余 state.orders() 顺序 == 下 tick 全量重放顺序（追加序保持）——测试覆盖。

## 棒次 2 终局（2026-10-07，完整实现+门禁+基准）

**实现**（工作区在途未提交，8 文件）：Cut A（死 ledger 守卫）+ Cut B（残余影子缓存全链路：process 解构+residual 归还→shadow/coordinator/finish_shards 水管→state 字段+hash/两克隆排除→PhaseInput 旁路参数传递（X1 模式）→命中判定全 exact→逐股 coordinator 包装）。

**过程中消灭的两个真 bug**（TDD/全规模各抓一个）：
1. ledger 随残余携带会带入上 tick 回执/terminal 行（与全量重建形态不同）→ 3000 tick 全规模在 tick 87 炸不变量；修法=残余不携带 ledger、命中时以活跃 envelope 廉价重建（与 from_post_expiry 同构，~0.03ms/股/tick）。
2. 命中计数器全局 static 在并行测试下互扰 → 合并测试为串行段。

**门禁**：lib 1320 过/8 败=基线逐字一致（新增 2 测试：逐 tick 事件数+状态哈希等价、直改后首 tick 失配回退）；正式 probe+verify-build 通过（candidate SHA f709775a…，baseline 7aabf745… 隔离 worktree 构建）；3000 tick 全规模跑通（events 25509）。

**基准（锚定交错 astart,(H,C)×5,aend，16-31 核占用 0.9%，.tmp/e-auction-impl/pairing2.log）**：
- H=[233.0,229.8,228.8,222.3,235.6] C=[242.5,237.7,231.0,229.0,234.6]
- d=C/H=[1.041,1.034,1.010,1.030,0.996]，**mean +2.21%**，4/5 正，锚漂 1.82% ✓
- 预登记五重：均值≥2% ✓、锚漂 ✓、全正 ✗（末对 0.9955）、区间分离 ✗（H_max 235.6 > C_min 229.0）→ **判定：边缘未过门，交协调方裁决**

**裁决输入**：C 中位数 234.6 > H 中位数 233.0，C 均值 235.0 > H 均值 229.9；波动源为 H 臂散布大（222-236）。两个处置选项：(a) 复跑一轮确认（12 跑≈4 分钟）；(b) 按预登记撤回（patch 在工作区+本档，Cut A 补丁另有独立存档 cuta-dead-ledger-guard.patch——注意 Cut A 单刀 +1.7% 也不过线）。

**遗留清理**：auction-base worktree（7d6558c8 隔离基线构建用）用完应删；base-target 产物在 .tmp/e-auction-impl/。

## 终局（协调方裁决复跑后：诚实撤回）

**第二轮锚定配对**（0-15 核占用 0.4%，试验 E/F 在其他核段，锚漂 2.58% ✓，pairing3.log）：
- H=[238.3,234.6,216.9,239.5,247.1] C=[242.7,230.7,249.8,248.5,238.5]
- d=C/H=[1.019,0.984,1.152,1.038,0.965]，**mean +3.13%，正号仅 3/5**

**裁决规则**（协调方预登记）：mean ≥+2% 且 ≥4/5 正 → 落地；否则撤回。第二轮 mean 过线但正号 3/5 不满足 → **撤回**。两轮合并 7/10 正、均值 +2.21%/+3.13%。

**死因（如实）**：机制存在且端到端收益方向为正（两轮均值均 >2%），但 H 臂散布巨大（216.9~247.1，跨 14%）把符号一致性淹没——当前多 agent 并行诊断的机况下拍卖段占全 wall 仅 ~18%（1896ms/10.3s），+5~7% 段收益折算全局限度在噪声带内。机制证据链完整保留：P0 oracle（prepare -84%）、本轮 diag-split（门禁 411ms 不可动、可缓存段 732ms）、Cut A 单刀 +1.7% 实测（diag-cuta.json）。

**撤回执行**：8 个源文件按 HEAD 原字节恢复、缓存测试文件删除；lib 1319/8=基线逐字一致；probe 重绑 HEAD（verify-build 通过）。实现 diff 可从本档+工作历史重建（关键结构：残余影子=shadow 整体跨 tick、ledger 必须重建不可携带、X1 参数旁路传递模式）。

**重启条件**：机况安静（无并行诊断 agent）时单段收益可被分辨；或拍卖负载占比上升（更长竞价窗口/收盘段纳入基准）后复跑——设计与全部坑位已在本档。
