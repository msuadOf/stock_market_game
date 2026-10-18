# P0 基线复测与分段画像结论（第 19 任，2026-10-18）

> worktree `.worktree/view-campaign`，HEAD 56268e5f（=main 合并点）。
> 原始输出：`baseline-raw.txt`（A/B 双口径 3 轮）、`profile-1000t-latest.txt`、
> `profile-300t-latest.txt`（分段画像）、`perf-v2-script-excerpt.txt`（perf 叶样本）。

## 1. 基线复测（净机窗，load<1.6）

| 口径 | 本轮 mean (min–max) | 逐位锚 | 对照第一阶段终测 |
|---|---|---|---|
| B 300 tick 全语义 | **307.3** (288.6–318.1) t/s | events **2105** / fees 0 三轮逐位；股份守恒 ✓ | 302.7（+1.5%） |
| B 1000 tick | **544.7** t/s | events **6016**；零衰减（比 300 快=冷启动摊薄） | 501.5（+8.6%，净机窗更好） |
| A 3000 tick 裸 World | **1332.6** (1287.2–1383.2) t/s | 成交量 6701200 / 费用桶 651403179 / T+1 6668400 三轮逐位；守恒 ✓ | 1409.2（−5.4%，±15% 带内） |

**注意**：本树 events/成交量与 P2 账本数值不同（3336→2105、6438600→6701200）——
合并后撮合/事件语义已变（并发撮合规则等），**战役锚以本树本轮为准**。

B/A 收敛比（300 口径）= 307.3/1332.6 = **0.23**。

## 2. 分段画像（STEP_PROFILE=1，新探针 `runtime/step_profile.rs`）

driver 线程单 step 分段（1000 tick 稳态，wall 1832μs/tick）：

| 段 | μs/tick | 占 wall | 可删映射 |
|---|---:|---:|---|
| world_run（裸引擎收束+安装） | 367 | 20.0% | 保留（对照基线） |
| attention_heap（稀疏收集+heap 全名册重建） | 258 | 14.1% | **D6** |
| belief_echo（collect_group_belief+session 整态安装） | 213 | 11.6% | **D1** |
| envelope（resting 收集+增量/全量信封） | 140 | 7.7% | **D6** |
| acct_echo_collect（dirty 标记+稀疏账户上行） | 62 | 3.4% | **D1**（含 D2 并集） |
| candle_install（K 线安装） | 47 | 2.6% | **D4** |
| retail_diag（拒收诊断+决策 trace） | 19 | 1.0% | **D5** |
| acct_writeback（账户镜像回写循环） | 17 | 0.9% | **D1/D2** |
| plans_echo（collect_plans+同拍补写） | 17 | 0.9% | **D1+D3** |
| desk_mirror（簿镜像+lifecycle 清空） | 16 | 0.9% | **D6** |
| market_install 净额（行情/价格历史/分钟线） | 5 | 0.3% | **D4** |
| baseline_snapshot（五域每拍快照） | 2 | 0.1% | **D5** |
| fill_projection / memory_prune / hold_inits / 其他 | <0.1 各 | — | D3（微小） |
| step_entry / undo_clear / lock_route（入口/清理） | ~18 | 1.0% | 部分随 D5 消失 |
| **六项可删合计（D1-D6 段和）** | **≈795** | **≈43%** | |
| step() 内未段化缝隙（内联代码+段间胶水） | 319 | 17.4% | 待 P1 后归因 |
| step() 外循环开销（civil check 仅 0.35μs；余为调度/缺页） | 331 | 18.1% | 冷启动摊薄+系统噪声 |

300 tick 口径（含冷启动，wall 3281μs/tick）：六项可删 ≈25%；各段绝对值同量级、
envelope/attention 随时长增长（挂单存量 305→4766）。

## 3. 归因备注（如实登记）

- driver 线程 CPU 占比 76.6%（perf 858 样本）～80%（ps 瞬时），与上战役账本
  79.85% 一致——**安装链仍是关键路径**，P2 拆迁方向实证有效。
- perf 叶符号：最大单符号仅 3.5%（碎叶）；缺页族（kernel_init_pages 等）≈8%、
  memmove/memcmp ≈7%、BTreeMap 遍历 ≈5%—— jemalloc 缺页税与深拷贝族为主。
- 主线程样本含少量决策链符号（dcf/信息投递/会计 ≈5-8%，v2 叶统计）——归因未闭合
  （dwarf 断栈、LBR 不可用），疑为内联帧误归属或共享代码路径；**不改变"安装链主导"
  的结论，登记为开放项**。
- gdb 栈采样确认热点帧：install_candles 的 take_candle_projection（mpmc send）、
  converge 等待、set_runtime_attention、resting_delta——全部在已段化域内。

## 4. 对战役目标区间的修订建议（数据说话）

Sol 拒绝拍胸脯是对的：**六项可删 ≈43%（1000 tick 稳态）而非"5× 全可回收"**。
若 P1 发布协议+P2 六项全删且无新增税：B 1000 理论上限 ≈ 1832/(1832−795)×545
≈ **912 t/s**（假设 world_run 与缝隙不变、发布成本≈0）；叠加缝隙/调度回收（+31%）
才触 1000+。300 口径冷启动占比高，门槛按 1000 tick 口径修定更稳。

## 5. 开放项（不阻塞 P1）

1. step() 内缝隙 319μs/tick 的逐语句归因（内联展开面）。
2. 主线程决策链符号归属（重编 frame-pointer 版或 samply 复核）。
3. B 探针 fee sink 对账欠账（沿用登记——本轮 fees_paid 恒 0 断言边界不变）。
