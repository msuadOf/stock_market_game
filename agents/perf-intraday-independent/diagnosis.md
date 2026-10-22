# 引擎日内独立跑 / 日终同步——只读诊断与基准报告

> 日期：2026-10-22
> 任务：只读诊断 + 基准测量，不改任何业务代码。
> 测量代码基线：`b86301fc`（当前分支 `fix/multi-group-roster-offset` 的 HEAD；
> 测量期间 HEAD 被协调者推进到 `beeb725d`，但 `b86301fc..beeb725d` 只含
> `agents/` 文档，无引擎代码差异，故二进制绑定仍有效）。
> 对照 main：`4afd4920`。历史峰值 tag：`perf/bare-1000x-viewcampaign`
> = `5e0f7269`，`perf/fullsem-1000x-viewcampaign` = `0ae57763`。

---

## 0. 结论摘要

1. **B 口径（全语义）没有回退，反而高于历史峰值**：
   当前 main 3 轮均值 **1268.5 t/s**，历史峰值记录 **1032.7 t/s**（+22.8%）；
   同机同窗复测历史 tag 为 1172.5 t/s（当前 main 相对 +7.7%）。
2. **A 口径（裸 World）当前代码无法完成 3000 tick**：
   main 与当前分支均在 **tick 241（第一个日界）4/4 复现崩溃**，错误为
   交易部买腿 purse 低于精确需求 / 组端预留失同步。
   - 引入提交锁定为 **`b1a373c8`（日界批：收盘竞价成交归当日日K）**：
     其父提交 `b1a373c8^` 4/4 跑通 3000 tick，main/当前分支 4/4 在 tick 241 失败。
   - 机制最可能：日结 Drain 的收盘竞价 reroll 更新了 desk 的 `last_close`
     （次日涨跌停基准），但 `public_views` 未在日结后刷新；下一拍组端仍用
     旧涨停价预留 purse，desk 按新基准受理，导致 `charge > purse`。
3. **A 口径在崩溃之前已有约 9%–11% 的吞吐回退**：
   同机 ABBA 配对，tag 中位 **957.8 t/s**，`b1a373c8^` 中位 **855.7 t/s**
   （中位 −10.6%，均值 −8.9%）。可测量的主因指向 **`606ba809`（⑤a
   attention due heap）**：A 负载是 10 组×2000 NPC、due 密度高
   （约 1/4 名册/拍），延迟删除堆的 push/pop/sort 反超线性扫描；
   该刀只在 B 口径（1 组×20000、due 密度约 1%）验证过。
   前后配对：⑤a 之前中位 944.6，之后 874.3（−5.2%，均值 −5.9%）。
   剩余约 3%–5% 差异在噪声与代码布局量级，未再细分。
4. **每拍会话同步面实测约 201 μs/tick，占 B 口径 wall 的 25.1%**
   （计入回滚支撑/事件则为 212 μs，26.5%）。最大项是 `belief_echo`
   96.9 μs/tick（12.1%），其次 `desk_mirror` 29.0、`market_install` 25.3、
   `attention` 16.1、`retail_diag` 15.4。
5. **World 内部的 Start/收敛/Seal/SealAck 不能推迟**（提交=发布=Abort
   窗口、撮合轮次、实体所有权）。可推迟的是 **session 镜像安装与外部观察面**，
   不是引擎内部提交屏障。
6. **配置建议**：开关放 `SessionSetup`（不是 `GameConfig`，也不是 `WorldSpec`）。
   数据支持默认 `DayEnd`（可回收约 25% 的每拍同步面），但必须先补
   **存档前强制 flush、civil 公司链前强制 flush、日终原子回滚** 三个合同。

---

## 1. 测量环境与纪律

| 项 | 值 |
|---|---|
| CPU | AMD EPYC 7543 32-Core ×2（128 逻辑核可见） |
| 内存 | 247 GiB，无 swap |
| 亲和性 | `taskset -c 0-31`（与历史 A 口径一致） |
| 自旋 | `ENGINE_SPIN_US=500`（历史 SPIN500 口径） |
| 探针 | A：`runtime_s5_probe 3000 10`；B：`driver_semantics_probe <setup> 3000 1` |
| 运行时 | `ENGINE_STEP_RUNTIME=1`（B 探针强制；A 直跑 World） |
| 测量窗 load | 多数轮 4.4–8.8；构建后短暂 10–13 的轮次已标注，不用作归因主证 |
| 测量时其他任务 | 无 `cargo`/`rustc`/基准任务；仅有低 CPU 的 VS Code/Codex 常驻进程 |
| 日界 | 探针 A 的 `ticks_per_day=240`、`closing_auction_ticks=3`；B setup 的 `ticks_per_day=15300` |

### 1.1 二进制 SHA（性能结论必须绑定）

| 版本 | A 探针 SHA-256 | B 探针 SHA-256 |
|---|---|---|
| 当前 HEAD `b86301fc` | `9e6a2db77e5a6eef616b566ea706eb6cb6d02c7bb3e0a4ef27bb3bb0ffabfff1` | `43857b4c4ae94591c322b72f4b85aef4aaa087246c10e44df7c241a29791a204` |
| main `4afd4920` | `99dace710881928892ee025bc77210dad8c1e059fa931cd5865ca89f29e55b6d` | `44791eb908ea28bc9c46cb417faad34251fffea548cf1737e73fe6e5a5ec1c36` |
| tag `perf/bare-1000x-viewcampaign` | `ccfcabb427b63d55ec817e1e8eed9288e9d40e56f70979d35d767e1a8d302b27` | 同一构建的 driver（见 `.tmp/perf-intraday/bin/driver-tag`） |
| `b1a373c8^` | `1d1d54ab46f38dd7fb5b8a0ebf76e7a3d5edb9412fd2f9859b338f68d3008e27` | — |
| `de430f55` | `54f6148857303e29988fedbe267e2574efd8c4b58289e8e72d31cbafd8fc2f40` | — |
| `ac7ea83e`（⑤a 之前） | `83ac56329b165d7a27f59eda0214bc9035766450fe8e89033815ba4d8513289e` | — |
| `606ba809`（⑤a 之后） | `761764c645220f4e70369f79f3268d755724813230232154d5631f171b72ffb6` | — |
| `955c75b8`（B3 合并） | `817115372cd3334aac12e32eb26a79bfef6c22557ac6eb6263b80ff540c5bdad` | — |

原始输出：`.tmp/perf-intraday/`。

---

## 2. Q1：当前性能 vs 历史峰值

### 2.1 B 口径（全语义，5 股 × 20000 NPC，3000 tick）

| 版本 | 3 轮 speed_x（t/s，去掉热身轮） | 均值 | 中位 | 相对历史峰值 1032.7 |
|---|---|---:|---:|---:|
| 当前 HEAD `b86301fc` | 1253.9 / 1266.7 / 1282.3 | **1267.6** | 1266.7 | **+22.8%** |
| main `4afd4920` | 1260.6 / 1276.4 / 1268.6 | **1268.5** | 1268.6 | **+22.8%** |
| tag 同窗复测 | 1190.6 / 1172.5 / 1170.9 | 1178.0 | 1172.5 | +14.1%（相对同窗 tag +7.7%） |

- 守恒锚每轮一致：`events=21881`、`shares_ok=true`、
  `cash_final == cash_initial == 1655573067947`。
- 结论：**B 口径无回退**。当前 main 比历史记录高 22.8%，比同机同窗 tag 高 7.7%。
  “28x”确系误引旧数据。

### 2.2 A 口径（裸 World，10 组 × 2000 NPC，3000 tick）

**当前 main / 当前分支无法完成 3000 tick。** 4/4 复现：

- main 轮 1：`交易部 4 买腿 purse 不足：purse=218302, charge=232902（组端预留口径 bug）`
  （`desk.rs:1417`）。
- main 轮 2–4：失败撤退路径二次报错
  `Final 收集期望 GroupFinal/DeskFinal，收到 GroupReport(group=0, tick=241, round=0)`。
- 当前分支：`交易部 0 买腿 purse 低于精确需求（组端预留失同步）：
  owner=(0,11), purse=198502, required=224902`（`desk.rs:1741`）。
- 全部发生在 **tick 241**（第一个日界后的第一拍）。
- `b1a373c8^` 同载荷 4/4 跑通，费用桶/冻结量与 tag 逐位一致
  （`651403179` 分 / `6668400`）。

可测的 A 数值（同一窗口、ABBA 交替）：

| 版本 | speed_x（t/s） | 中位 | 备注 |
|---|---|---:|---|
| tag `5e0f7269` | 933.1 / 973.2 / 950.0 / 965.5 | **957.8** | 4/4 跑通 |
| `b1a373c8^` | 869.3 / 842.1 / 842.5 / 928.3 | **855.7** | 4/4 跑通 |
| 当前 main 首日 240 tick 代理 | 865.2 / 883.8 / 866.5 | 866 | 不能跨日界 |
| tag 首日 240 tick 代理 | 942.9 / 918.9 / 971.9 | 943 | — |

- A 口径在崩溃前相对 tag：**中位 −10.6%，均值 −8.9%**（ABBA 配对）。
- 首日 240 tick 对照：main 866 vs tag 943（约 −8.2%）。
- 说明：A 探针在 `b1a373c8` 之后报告的“成交量 0 股”是探针指标被日界
  `reset_market_for_new_day` 清 `volume_today` 的**读面伪影**；费用桶与冻结量
  逐位相同，不能据此判定“没有成交”。

### 2.3 A 回退归因（先归因再报数）

逐步配对（同窗口、同一探针、交替执行）：

| 测点 | 中位 t/s | 相对 tag |
|---|---:|---:|
| tag | 957.8 | — |
| `de430f55`（P1-1/2/3 后） | 约 966–1019 | ≈ 持平 |
| `ac7ea83e`（⑤a 之前） | 944.6 | ≈ −1% |
| `606ba809`（⑤a 之后） | 874.3 | **−5.2%（配对中位）** |
| `955c75b8`（B3 合并） | 约 875–913 | −6%～−9% |
| `b1a373c8^` | 855.7 | −10.6% |

**可确定部分：`606ba809`（⑤a attention due heap）**。

- 该刀把组端 round-0 due 筛选从“全名册线性扫描”改为
  `BinaryHeap` 延迟删除（`group.rs:869` 字段、`group.rs:3832-3845`
  peek/pop/核验、`group.rs:3891` 重排 push）。
- A 负载是 **10 组 × 2000 NPC**，初始 `next_due = i % 8`、`attention_gap_ticks=4`
  （`runtime_s5_probe.rs`），每拍 due 密度约 1/4 名册；而该刀的验证口径是
  B 的 **1 组 × 20000 NPC、due≈20–40/拍（约 1%）**。小名册、高 due 密度下，
  `O(due·log N)+sort` 反超 `O(N)` 线性扫描，方向与实测一致。
- 前后 ABBA 配对（`ac7ea83e` vs `606ba809`）：
  中位 944.6 → 874.3（−5.2%），均值 943.6 → 887.9（−5.9%）。
- 另有约 3%–5% 差异落在 B3/B2 合并前后，但 B3 的 diff 只改
  `bridge.rs`/`drive.rs`（A 探针不经过），机制上无法解释；更可能是
  构建代码布局/测量噪声。**不建议据此追责 B3/B4a**。

> 对协调者“B3/B4a 无性能门”线索的回答：本次是 B3/B4a 之后**首次**
> 对 A/B 双口径做同窗配对。结论是 **B3/B4a 不是 A 回退的原因**
> （A 不经过 session 侧），也**没有造成 B 回退**（B 反而 +7.7%）。
> 真正可归因的 A 回退是 ⑤a 的 heap 策略在“小名册高 due 密度”下失配。

---

## 3. Q1 附带发现：tick 241 日界崩溃的机制链（需修复，非本报告实施）

证据链（当前 HEAD 行号）：

1. `World::run_ticks` 在新一拍 Start **之前**跑日结：
   `world.rs:1694-1700`。
2. 日结 Drain 的收盘竞价 reroll 把 `last_close` 更新为收盘竞价成交价：
   `desk.rs:2274` `reroll_after_drain_clearing` →
   `desk.rs:2210` `reset_market_for_new_day`。
3. 下一拍 `broadcast_start` 仍使用 `self.public_views`
   （`world.rs:1709`），而 `public_views` 只在 `install_tick` 中更新
   （`world.rs:2036`、`world.rs:2100-2114`）；**日结后没有重新
   install/publish**。
4. 组端市价/符号价买单的 purse 参考价取广播快照的
   `daily_upper_limit`（`decision.rs:169`、`group.rs:4140-4147`），
   而 desk 的 `daily_upper_limit` 由 `up_stop()` 按当前 `last_close` 重算
   （`desk.rs:2479`）。
5. 于是 tick 241 组端按旧涨停预留，desk 按新涨停受理，
   `charge > purse`，触发 `desk.rs:1417` / `desk.rs:1741` 显式失败。

**建议修复方向**（供实现批设计门）：日结成功后、下一 Start 前，
用日结候选/desk 当前态刷新 `public_views` 并重发布一次；同时补
“多组、多日、收盘竞价后首拍”回归测试，断言 purse 不失同步且首拍可成交。

---

## 4. Q2：每拍同步面盘点

测量口径：当前 HEAD B 探针，3000 tick，1 组 × 20000 NPC，5 股；
profile 轮 wall = 801.5 μs/tick（speed_x 1247.7）；正式 3 轮为 789 μs/tick。
下表成本均为 **μs/tick**，括号为占 profile wall 比例。

### 4.1 总表

| # | 同步点 | file:line | 每拍做什么 | 数据量级 | 成本 | 是否必须每拍 |
|---|---|---|---|---|---:|---|
| 1 | World Start/收敛/Seal/SealAck | `world.rs:1684-1800`、`world.rs:1813-1932`；组端 `group.rs:5788-5987` | 广播 Start，收 G+N 报告，Continue 轮，Seal 后随 Ack 上行账户/belief 分片 | G 条 + N 条消息；每 tick 可多轮 | `world_run` **379.9**（47.4%），其中 Seal 相位约 11% wall | **是**（提交/Abort/撮合轮次） |
| 2 | 发布 `Arc<PublishedTick>` | `publication.rs:230`；写点 `world.rs:2100-2114` | 组装 N 个 DeskShard + G 个 GroupShard（Arc 句柄），RwLock 交换 | 5 DeskShard + 1 GroupShard | 计入 world_run；`load` 30ns 级 | 构建必须；外部可见性可推迟，但收益小 |
| 3 | `install_market_views` | `drive.rs:2471-2536`；调用 `failure.rs:754` | 全量 `world_public_views().to_vec()`；逐股 restore_prices/价格历史/分钟收盘合并/分钟桶追加 | 5 股 ×（StockPublic + ≤20 价格 + 当日分钟 + 桶） | **25.3**（3.16%） | 否（session 查询/存档面） |
| 4 | attention 稀疏收集 + 镜像 | `drive.rs:2537-2562`；`world.rs:1438-1476` | EchoAttentionSparse 组往返；写 session `npc_attention` | due≈20–40/拍；名册 20000 | **16.1**（2.01%） | 否（组端 next_due 权威） |
| 5 | `install_candles` | `drive.rs:2587-2589`、`2607-2718` | 读发布版本 active K，逐股装/清/补占位 | 5 股 × 1 活动 K | **0.37**（0.05%） | 否（但日界 `commit_active` 前必须装） |
| 6 | 账户回声收集 | `drive.rs:716-773` | 从发布版本借用账户稀疏分片 Arc；覆盖校验 | 本拍 dirty 账户 | **5.3**（0.66%） | 否 |
| 7 | 账户回写 group→session | `drive.rs:775-867` | 逐 dirty 账户记录 undo、合并 positions、写现金/持仓 | dirty 账户；force_full=20000 | **7.9**（0.99%） | 否（组端权威） |
| 8 | belief 回声 | `drive.rs:968-1006` | 发布版本 belief 稀疏条目 → session 个人态 | 本拍 dirty belief | **96.9**（12.1%） | 否（session 查询/存档/通知面） |
| 9 | plans 回声 | `drive.rs:1026-1054` | EchoPlans 组往返；dirty 未变回空 | 计划簿（基准多为空） | **4.5**（0.56%） | 否 |
| 10 | retail 诊断 | `failure.rs:806-834` | EchoRetailTraces 组往返 + 拒收/决策 trace 整值换装 | 本拍事件/trace | **15.4**（1.92%） | 否（纯诊断） |
| 11 | envelope/resting 快照 | `failure.rs:856-884` | 从发布版本取 N 个 Arc 句柄 + 游标 | 5 个 Arc | **0.44**（0.05%） | 否 |
| 12 | desk 簿镜像 | `failure.rs:886-959` | 遍历每股全部在途单，归户组装 `BTreeMap`；清 lifecycle | 5 股 × 在途单（随时长增长） | **29.0**（3.62%） | 否（session 查询/存档面） |
| 13 | 事件组装 | `drive.rs:876-955` | trade/lifecycle/auction/receipt 投影 | 本拍事件 | **1.18**（0.15%） | 可批量化，需保序 |
| 14 | PriceTick 生成 | `drive.rs:1101-1173` | 连续相位每股一条，含五档深度 | 5 条/拍 | **0.60**（0.07%） | 可批量化，需保序 |
| 15 | 安装基线快照 | `failure.rs:1200-1232` | clone markets/price_history/minute_closes/bars/candle 历史+active/resting 快照 | 5 股 × 上述域；日结拍 +20000 账户 | **2.36**（0.29%） | 仅每拍安装回滚需要 |
| 16 | 写时 undo 清 | `failure.rs:699`、`failure.rs:665` | 入口/尾清 undo 槽（2 次/拍） | O(undo 条目) | **6.44**（0.80%） | 仅每拍安装回滚需要 |
| 17 | step 入口健康/策略扫描 | `failure.rs:1303-1312` | `has_missing_npc_strategy` 页级重扫 | 名册页 | **9.07**（1.13%） | 守卫；可缓存，非同步 |
| 18 | civil date / tick capture / seq | `drive.rs:624`、`drive.rs:627-632`、`drive.rs:2361/2380` | O(1) 写 | O(1) | <0.2 | **是**（锁步不变量） |

### 4.2 汇总

- **可推迟的会话/外部门面**（#3–#12、#15–#16）：
  `25.3+16.1+0.37+5.3+7.9+96.9+4.5+15.4+0.44+29.0+2.36+6.44`
  = **209.9 μs/tick ≈ 26.2% wall**。
- 若只计“纯镜像安装”，不计基线/undo：**201.2 μs/tick ≈ 25.1% wall**。
- 事件批量化（#13–#14）另 **1.8 μs/tick**。
- 必须保留的引擎内部面：`world_run` **379.9 μs**（47.4%），
  其中 Seal 相位约 11% wall；加上锁步/civil 的 O(1) 成本。

> 与历史 3000 tick 画像的对照：tag 同 setup 下 `world_run=437.6 μs`，
> 当前 379.9 μs（−13.2%）；install 链两端均约 200 μs。也就是说，
> **B 口径的提速来自 world_run，不是同步面**；本轮所有回退/收益都要分开看。

---

## 5. Q3：哪些同步可以推迟到日终

### 5.1 必须每拍（不能推迟）

1. **World 内部 Start/收敛/Seal/SealAck**（`world.rs:1684-1932`）。
   这是引擎自己的提交屏障：提交点=发布点=Abort 窗口关闭；Continue 轮次、
   实体所有权、失败回滚都依赖它。用户所说“日内不跟外面同步”不能解释为
   取消 Seal；否则等于重写提交模型。**需设计门明确边界。**
2. **tick/seq/civil 锁步**（`drive.rs:2361/2380`、`world.rs:1194`）。
   O(1)，保留成本可忽略。
3. **日界拍的所有语义迁移**（T+1 解锁、活动 K 归档、day 推进、
   收盘竞价清算、保留日冻结）。这些是 A 股语义，不是“同步”，不能省。

### 5.2 可以推迟到日终 / 按需

| 面 | 判断 | 理由 | 风险/前置 |
|---|---|---|---|
| 账户回写 group→session | 可以 | 组端 `GroupAccount` 是权威；session 只服务查询/存档/公司链 | 存档前必须 flush；`end_civil_day` 前必须 flush |
| belief 回声 | 可以 | session 个人态服务查询/存档/通知（`notices.rs`、`history_reads.rs`、hash） | 存档/通知前 flush |
| plans 回声 | 可以 | session 计划簿是镜像；组端 `plan_book` 权威 | 存档前 flush |
| attention 稀疏 | 可以 | 组端 `next_due` 是调度权威；session 镜像供存档/查询 | 存档前 flush |
| market views | 可以 | desk 公共态权威；session 行情供查询/PriceTick | `commit_active` 前需先装 candle；PriceTick 若需实时则不能纯日终 |
| candles | 可以 | session K 书供查询/归档 | **日界 `install_runtime_day_boundary` 的 `commit_active_daily_candles` 之前必须先装**（`day_end.rs:59`） |
| desk mirror | 可以 | desk 簿权威；session 镜像供查询/存档 | 存档前 flush |
| retail 诊断 | 可以 | 纯诊断 | 可在日终批量或按配置关闭 |
| envelope/resting 快照 | 可以 | 查询/存档面 | 存档前 flush |
| 事件组装/PriceTick | 可以批量化 | 事件是可交付输出；顺序需保持 | seq/tick 顺序、DayBoundary 序、收盘竞价序 |
| 安装基线/undo | 可以 | 只为每拍安装回滚存在 | 日终 flush 需一次性基线 + 原子回滚 |

### 5.3 推迟会破坏哪些硬约束（逐条）

1. **失败原子性 / Abort 回滚**
   - World 内部 Abort 不受影响（Seal/Start 仍在每拍）。
   - 但 session 侧若不再每拍安装，就**没有“每拍安装失败回滚”问题**；
     反过来，日终 flush 必须自己成为原子单元：flush 前取全量基线
     （账户+行情+分钟线+K 书+belief/plans/attention/mirror），
     任一步失败整体回滚到上一个日界并 poison，不允许半安装。
   - 结论：**可以做到，但必须重新设计 flush 的原子边界，需设计门裁决。**

2. **存档一致性**
   - `GameSession::save(&self)`（`failure.rs:2586`）直接序列化 session 镜像
     （账户、行情、K、belief、plans、attention，见
     `session.rs:3966` `save_snapshot_projection`）。
   - **日终不同步模式下，日中调用 save 会存到旧状态**。
   - 必须新增 `flush_runtime_sync(&mut self)` 并在 save 前强制调用，
     或明确“只允许日界存档”。这是硬前置，不可静默。

3. **首错顺序**
   - 事件若按日批量交付，必须保留引擎分配的 seq/tick 顺序；
     `DayBoundary` 必须在当日 Trade/竞价事件之后、drained 撤单之前
     （当前合同见 `failure.rs:984-1076`）。
   - 建议仍在每拍分配 seq，只推迟交付，不改变编号顺序。

4. **A 股语义（T+1 / 日 K / 收盘竞价）**
   - `install_runtime_day_boundary` 在 session 侧解锁 T+1 并提交活动 K
     （`day_end.rs:51-71`）。
   - 若账户/candle 都推迟，日界拍必须先装 candle，再 commit_active；
     账户则应在 world 日结（组端已解锁）之后做**全量回写**，
     用组权威覆盖 session 旧值。当前生产路径是“日结后置
     `force_full_accounts=true`，下一拍才回写”（`failure.rs:1074`），
     在 DayEnd 模式下应改为**同拍日结后立即全量回写**，否则
     `end_civil_day` 公司链和存档会读到旧账户。

5. **发布协议**
   - `PublishedTick` 是外部观察面（`publication.rs:188`）。
     DayEnd 模式下外部读者看到的是最后一个日界版本，这是可接受的产品语义；
     但如果 web 端要实时行情，必须显式选择 `EveryTick` 或直接消费更轻的
     发布分片，而不是依赖 session 镜像。
   - 发布本身很便宜（Arc + RwLock），**不建议为了省成本取消每拍 publish**；
     重点是把 session 安装搬走。

---

## 6. Q4：配置面设计建议

### 6.1 放哪一层

| 候选 | 结论 | 理由 |
|---|---|---|
| `GameConfig`（`config.rs:135`） | **不放** | 该结构是交易费率/规则（佣金、印花税、涨跌停、整手），会被组端 clone/共享，混入同步策略会污染 A 股语义面 |
| `WorldSpec`（`world.rs:169`） | **不放** | World 只负责撮合与提交，不拥有 session 镜像；且不是存档/宿主配置面 |
| `SessionSetup`（`session.rs:1191`） | **推荐** | 宿主可见、可序列化、随存档固化；与 `t1_enabled`、`ticks_per_day` 同层 |
| 环境变量 | 仅实验/逃生 | 不适合产品配置；当前只有 `ENGINE_STEP_RUNTIME` 等 |

建议新增（示意）：

```rust
pub enum RuntimeSyncPolicy {
    EveryTick,   // 当前行为，兼容
    DayEnd,      // 非日界拍跳过 session 安装/回写，日界拍一次 flush
    OnDemand,    // 仅在 save/外部读/显式 flush 时同步
}
```

放在 `SessionSetup`，`#[serde(default)]`，旧档默认 `EveryTick`。
非法值显式拒绝，不静默 fallback（铁律二）。

### 6.2 默认值：数据依据

- 实测可推迟的每拍同步面：**约 201–210 μs/tick，占 B wall 的 25%–26%**。
- 当前 B 吞吐 1268 t/s（789 μs/tick）。若同步面完全消失，
  理论上限约 `1268 / (1 - 0.25) ≈ 1690 t/s`；实际日终 flush 仍有成本，
  保守估计 **+20% 左右**。
- 因此：
  - **引擎快速跑 / 无头批量 / server 仿真**：默认 **`DayEnd`**，符合用户
    “引擎酷酷跑、日终同步一次”的指令，且有 25% 量级的实测收益支撑。
  - **Web 交互（需要日内实时表格/行情）**：默认 **`EveryTick`**，
    或 `OnDemand`（外部读时 flush）。在 web 端改为直接读
    `ViewPublication` 之前，不要贸然全局切 `DayEnd`。
  - 若只能设一个全局默认：建议 **`DayEnd`**，但必须同时交付
    `flush_before_save`、`flush_before_civil_day`、日终原子回滚三个合同；
    否则会以“存档/公司行动读到旧账户”的方式静默破坏语义。

### 6.3 实施前必须由设计门裁决的点

1. 日终 flush 的原子回滚边界（账户+行情+K+belief/plans/attention/mirror）。
2. `save()` 增加强制 flush（API 从 `&self` 变为需要 `&mut self` 或宿主显式两步）。
3. `end_civil_day` 公司链与 flush 的先后/失败回滚。
4. T+1 解锁、`commit_active_daily_candles`、收盘竞价 reroll 在 DayEnd 模式下的顺序。
5. 外部发布语义：DayEnd 模式下 web/server 看到的数据滞后一个交易日是否可接受。
6. 事件批量交付的 seq/首错顺序合同。

---

## 7. 诚实边界

- **A 口径当前 main/HEAD 的 3000 tick 吞吐测不出**：代码在 tick 241 稳定崩溃。
  已用 `b1a373c8^`（崩溃前最后一个可跑版本）给出可用数字，并注明它不是
  当前代码；当前代码只能给出首日 240 tick 代理值。
- A 回退的 **~5% 主因（⑤a）有前后配对与机制支撑**；剩余 3%–5% 落在
  构建布局/噪声量级，未强行归因到具体提交。
- 所有性能数字绑定第 1.1 节二进制 SHA；测量窗 load 约 4.4–8.8，
  构建后短暂 10–13 的轮次不作为主证。
- tick 241 崩溃的机制链是**代码路径 + 复现实验**支持的强假设；
  最终触发点（具体哪只股票、哪个价格差）需实现批以最小复现确认。
- 日界批台账里的 `952.2 → 905.8（−4.9%）` 口径是“当时 main vs 日界分支、
  1000 tick、外部 load 45–50”的短窗，原作者已判定为冷启/外部负载伪影；
  3000 tick 复测 −1.1%（±2% 门内）。本报告未把它当作真实回退。

---

## 8. 复现命令

```bash
# 构建当前 HEAD
cargo build --release -p engine --example runtime_s5_probe --example driver_semantics_probe

# A 口径
taskset -c 0-31 env ENGINE_SPIN_US=500 \
  ./target/release/examples/runtime_s5_probe 3000 10

# B 口径（STEP_PROFILE=1 可看分段）
STEP_PROFILE=1 taskset -c 0-31 env ENGINE_SPIN_US=500 ENGINE_STEP_RUNTIME=1 \
  ./target/release/examples/driver_semantics_probe .tmp/driver-semantics-setup.json 3000 1

# 首日 240 tick（避开日界）
taskset -c 0-31 env ENGINE_SPIN_US=500 \
  ./target/release/examples/runtime_s5_probe 240 10
```

证据目录：`.tmp/perf-intraday/`（各轮原始输出、profile、配对记录）。
