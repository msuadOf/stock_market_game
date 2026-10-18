# E3：capture 侧双排序融合（identity 一次排序承担查重与有序发射，已保留）

## 动机：E3 两轮诊断（插桩均按 manifest 原字节撤回）

E2 交接的候选是 hydration 内排序、seal_unknown_checks 与 input 侧双键排序，并留有"hydration 是 read-only join 新长杆"的判断。E3 第一轮 5-span 插桩（`e3-ros-instrument.py`，诊断 225.11x）在 post-E2 布局上实测 join 分支（2096 次命中）：

| 分支 | wall_ms | 每 tick |
|---|---:|---:|
| prepare_total | 3157.7 | 1506µs |
| branch2b = capture 1390.4 + shards 1658.6 | ~3049 | **~1454µs（真长杆）** |
| branch1 hydration | 1609.3 | 768µs |
| branch2a seal | 1091.0 | 520µs |

E2 的"hydration 长杆"判断不成立：真长杆是 branch2b（E2 数据是 fusion 前采的，布局已变）。第二轮 per-item 采样（`e3-ros-item-instrument.py`，诊断 216.88x）显示两相都在 ~16 线程间真分布，但 per-stock 耗方差极大（capture p50 94µs / p90 589µs；shards p50 71µs / p90 750µs）——单一大股在两段相位都占关键路径，逐股流水与加并行均无解，只能砍大股串行工作。shards 侧的 `validate_initial_snapshots` 索引替代在否决清单内，故选 capture 侧未被否决的③：**同一批订单先 BTreeSet 逐条插 `(owner, id, side)` 查重，捕获完又按完整 `EnvelopeKey` 排序——而单股内 stock 是常量，两个序严格一致**。

## 实验：一次排序承担两项职责

`prepare_stock_input` 改为：对 `Vec<((AccountId, OrderId, u8), book_index)>` 一次 `sort_unstable`，相邻相等标记簿序较大者为重复（与旧 BTreeSet"第 2+ 次出现于簿序"同一集合，3+ 重复亦同），捕获仍按簿序执行/保序收集/按簿序选首错，最后按 identity 序（= 单股内完整 key 序）发射 envelopes，删除第二次 String 全键排序与 `matched_keys`。identity 元组是 Copy，排序比较不再触碰 String。改动 2 文件（adapter + tests），+118/-25。

## TDD 与测试

- 红灯：`prepare_stock_input_stops_using_incremental_identity_set`（cfg(test) 计数器 `IDENTITY_SET_INSERTS` 在旧实现两处插入点递增）——重构前真实 exit 101（delta=1024≠0），重构后计数器无递增点、恒 0。按复核意见，测试注释已改写为"回归防线"性质（描述重构前的红→绿证据，避免陈述与最终代码不符）。
- 等价 pin：`dense_descending_books_emit_ascending_envelope_keys`（1024 单降序挂入，输出必须升序）重构前后均绿。
- 定向回归（filter 有重叠不累加）：adapter 32、read_only_start 7、continuous_tick_transaction 19、stock_execution 6、matching 71、quote_expiry 28、pre_open 12、decision_resources 12、hash_contract 14、auction 120、incremental_continuous 18、stock_stream 15、ledger 94、hydration 14、envelope 55，全部 9s TERM+1s KILL、RAYON4、四进程并行。既有基线固有失败（npc_state_projection/npc_tick_preparation 两 fixture、save_contract 旧日历）不在本批 filter 内，未触碰。

## 基准（顺序独占相邻配对，默认 5 股/20,007 NPC/seed42/16 核 0-15/Rayon16/Tokio2/3000 tick）

| 样本 | speed_x | binary |
|---|---:|---|
| e3-fused-head-16a | 223.273 | `.tmp/server-e3-head-baseline`（SHA16 1548a88d，verify-build 通过，与 E2 终版冻结逐字一致） |
| e3-fused-head-16b | 224.746 | 同上 |
| e3-fused-candidate-16a | 232.730 | `.tmp/server-e3-fused-candidate`（SHA16 30ce134d，verify-build 通过） |
| e3-fused-candidate-16b | 234.773 | 同上 |

HEAD 均值 224.010x，candidate 均值 233.751x，**+4.35%**；逐对 +4.24%/+4.46%，candidate 区间 [232.73, 234.77] 与 HEAD 区间 [223.27, 224.75] 完全分离。判定 >2% 且方向一致，**保留**。测试文件复核修复后重建 release 与已基准 candidate 逐字一致（SHA 30ce134d），基准无需重跑。1000x 未达成。

## 独立复核记录

全新一次性二级 agent（未参与实现，general-purpose）只读复核完整 diff：**PASS，无阻塞**。等价性逐条通过（EnvelopeKey::Ord 与 identity 元组同序、重复标记集合一致、首错仍按簿序、消息与 location 不变、observe_row 次数与位置不变、并发共享只读安全），并实跑 adapter 14 + continuous 140 用例。两条次要发现的处置：
1. 计数器测试在最终代码中为重言式 → 注释已改写为回归防线性质（只动测试文件，release 产物逐字不变）。
2. 重复标记算法缺直接 fixture：`place` 与 `restore_resting_orders` 均拒绝重复 order id，`BookState` 为 `pub(super)`，无注入路径；补 fixture 需给生产文件加 cfg(test) 注入 API（会改变 release 产物、需重跑基准）。此为重构前即存在的覆盖缺口（旧 BTreeSet 路径同样无 fixture），如实登记不硬补，等价性由代码复核承担。

## 下一步（按 E3 诊断数据）

branch2b 仍以 shards 段（大股 from_post_expiry ≈790µs，validate_initial_snapshots + PrivateStockLedger::new 全量重建）为主长杆，但其索引替代多在否决清单；可评估 hydration 内 `sort_identity` 533ms（跨股 String 键排序）与 `seal_unknown_checks` 885ms（join 外串行 tick 受益）；更高层可重新审视 step_frame 内 join 后各相位（candidate_commit 2.3s、npc_queue 2.06s）。
