# E4 零 op 初始化去重复校验实验（已撤回，+1.17% 低于门槛）

## 背景与诊断（8 sub-span，插桩按 manifest 原字节撤回）

E2 的 `shadow_step_zero_ops`（跨 worker 2598ms CPU）经 `e4-init-instrument.py` 细分：它包的是 `from_post_expiry` 内 `process_continuous_stock_step(input, 0)` 全调用，即"零 op 初始化"逐 tick 逐股对同一批未变行的四遍全量扫描，**不是调度/零尺寸任务开销**（交接摘要的猜测不成立，目标 A 与目标 B 实为同一段）。诊断 run 236.844x（probe 临时插桩，不作提速证据），12010 次调用 = 5 股 × 2402 Continuous tick，与 E2 的 2598ms 总量对账一致：

| 子段 | wall_ms | cpu_ms | 说明 |
|---|---:|---:|---|
| e4_step_zero_total | 2589 | 2518 | 总量，对账 E2 的 2598 |
| e4_init_snapshots_total | 1297 | 1287 | validate_initial_snapshots 全量 |
| └ e4_snap_loop | 817 | 806 | 逐行 Envelope::validate + origin/stock 检查 + BTreeMap 桥接 |
| └ e4_book_pass | 283 | 272 | 簿序核对（lookup + validate_snapshot） |
| └ e4_orders_clone | 170 | 158 | Market::resting_orders 全簿 Order 克隆 + seq 排序 |
| e4_ledger_new | 192 | 181 | PrivateStockLedger 行收集 + 排序 + 去重 |
| e4_complete_evidence | 536 | 514 | 完整证据；含第 208-210 行**逐行重跑 Envelope::validate** |
| e4_private_market_validate | 525 | 467 | run 尾部全行账本校验（>=512 行 Rayon 并行，worker CPU 不计入） |

关键等价性事实：每个 envelope 进入私有账本前都已通过 `Envelope::validate`（tick 起点行来自 validate_initial_snapshots，created 行来自逐个 validate），其后任何变更都经 `Envelope::apply` 对 candidate 重新验证后才落账——收尾重跑恒为同一结果，属可证死代码；`resting_orders()` 的输出序 = refs 收集后按 seq 稳定排序，可用 `Vec<&Order>` 引用替代克隆而不改变遍历序。

## 实验（两处最小改动，已完整撤回）

1. `PrivateStockLedger` 拆出 `validate_complete_evidence_prevalidated` 变体：跳过收尾逐行重跑 `Envelope::validate`；守恒证据、live/terminal 资源与聚合门禁全部保留；零 op 初始化调用点切换到该变体。
2. `validate_initial_snapshots` 改用 `resting_order_refs()` 收集 `Vec<&Order>` 后按 seq 排序（与 `Market::resting_orders` 输出序列逐字一致），消除整簿 Order 克隆。

TDD：两个 cfg(test) 计数器红灯真实 exit 101（`重验证 2 行` / `resting_orders 克隆 1 次`）→ 绿；`from_post_expiry_does_not_revalidate_unchanged_envelopes`、`from_post_expiry_walks_arrival_order_without_cloning_orders` 作为回归防线保留在 patch 中。定向回归 16 组全绿（continuous_matching 63、read_only_start 7、continuous_tick_transaction 19、stock_execution、matching、quote_expiry、pre_open、decision_resources、hash_contract、auction、stock_stream、ledger 94、hydration、envelope、adapter 32、private_stock_ledger 6；RAYON4/test4、四进程并行、9s TERM+1s KILL）。`shared_ingress` 的 `failed_private_receipt_gap`/`discarded_candidate` 两用例经 HEAD 原字节对照确认为**既有基线失败**（第三个既有失败，此前未登记），与本实验无关。

## 基准（顺序独占相邻配对，3000 tick/16 核 0-15/Rayon16/Tokio2）

| 样本 | speed_x | binary SHA16 |
|---|---:|---|
| e4-head-16a | 239.490 | 30ce134db3a78b46（HEAD 38e452d8 新建构建，与 E3 冻结逐字一致） |
| e4-head-16b | 243.031 | 同上 |
| e4-candidate-16a | 244.458 | e0ba34f922dff7e6（verify-build 通过） |
| e4-candidate-16b | 243.715 | 同上 |

HEAD 均值 241.261x，candidate 均值 244.087x，**+1.17%**，低于 2% 保留门槛 → **撤回**。源码按 HEAD 原字节恢复（git diff 干净），主 release probe 重建后 SHA 回到 30ce134d（verify-build 通过）。完整 patch `e4-dedup-init-rollback.patch`，回滚证明 `e4-dedup-init-rollback-proof.json`。注：同一 30ce134d binary 在 E3 会话测 232.7-234.8x、本会话测 239.5-243.0x，跨会话机器漂移约 +3~4%，再次说明只认相邻配对。

## 教训（影响后续方向选择）

砍掉 init 路径约 560ms CPU（重验证 ~390 + 克隆 ~170）只换来 +1.17%——**branch2b 的 wall 长杆不随这些验证 CPU 线性下降**。大股的 capture(~663µs)+shards(~791µs) 串行链里，剩余主导是每行的物化与业务数学（envelope 构造/克隆、预约/守恒计算），不是可证冗余的重复校验。方向启示：

1. 后续不要再做"删可证冗余校验"类小刀口（本实验是该类的上限样本）；
2. branch2b 内 per-stock 流水无解（大股自身链路不变，跨股重叠已被 par_iter 覆盖）；
3. 下一杠杆在**减少每 tick 需要物化的行数或行成本本身**（结构性复用属已否决区，需全新角度），或转向 join 外的串行相位（candidate_commit 2.3s、npc_queue 2.06s、seal_unknown_checks 885ms）。
