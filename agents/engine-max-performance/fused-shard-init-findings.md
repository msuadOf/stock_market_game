# 融合 stock input 捕获与逐股 coordinator 初始化（已保留，双轮独立复核通过）

## 动机：E2 嵌套诊断（21 sub-span，已撤回）

对 22-span 四大 CPU 段做内部拆解（`e2-four-span-instrument.py`，诊断 214.488x，接近正常区间，仅相对定位；插桩按 manifest 原字节撤回，git diff 干净）：

| 段 | wall_ms | 当前线程 CPU_ms | 内部大头 |
|---|---:|---:|---|
| borrowed_existing_projection（3001 次） | 1908 | 1903 | sort_identity 533、project_loop 479、ledger_audit 480、final_validate 289 |
| seal（3002 次） | 1391 | 1284 | **unknown_checks 885**、validate_resources 361/255、live_reservations 120 |
| input_prepare（2402 次，分支内） | 1600 | 1549 | identity_collect 503、sort 646、capture(调用线程) 407 |
| shadow_initialize（2402 次，5 股并行） | 1930 | 1828 | **step_zero_ops 跨 worker 合计 2598/2598 CPU** |
| accounts.clone() | — | 8 | 非问题（Arc 页共享） |

关键结构发现：`from_post_expiry`（shadow 初始化）每股只消费**该股自己的 input**（owned、不读 session），却排在 read-only join 之后的独立相位（join2）。两相位串行贡献约 1.5ms/tick。

## 实验：消除相位屏障

read-only 准备第三分支在 capture 完成后立即 `continuous_shards(inputs)`（同一 par 分支内，与 hydration/seal 并行）；输入所有权移入 shards。错误传播位置不变：捕获错误仍在 join1 detached（context 之后），初始化错误仍在 join2（validator 之后）。未命中路径（miss tick / PreOpen / auction）完全不变。改动 6 文件：read_only_start.rs、pipeline/mod.rs、continuous_tick_transaction.rs、incremental_continuous_stock_shadow.rs（cfg(test) 计数器）、continuous_matching.rs（mod 可见性放宽）、quote_expiry_tests.rs（结构体字段补 None）。

## TDD

- 红灯：`read_only_prepare_initializes_stock_shards_before_transaction` —— cfg(test) `FROM_POST_EXPIRY_CALLS` 计数器断言准备阶段完成每股初始化，改动前真实 exit 101（0 ≠ 市场数）。
- 绿灯：7/7 read_only_start 通过。
- 等价：`precomputed_shards_tick_matches_inline_path`（强制内联 vs 预计算全 tick 输出 Debug 一致）、1/4 worker shards Debug 与串行参考逐字段一致。
- 定向回归（filter 有重叠，不累加）：read_only_start 7、continuous_tick_transaction 19、stock_execution 6、matching 69、quote_expiry 28、pre_open 12、decision_resources 12、hash_contract 14、auction 120、incremental_continuous 18、stock_stream 15、ledger 94、hydration 14，全部 9s TERM+1s KILL、RAYON4/test4、四进程并行。
- 既有基线固有失败（npc_state_projection_tests::later_projection_failure、npc_tick_preparation_tests::next_tick_account_validation）不在本批 filter 内，未触碰。

## 基准（顺序独占相邻配对，默认 5 股/20,007 NPC/seed42/16 核 0-15/Rayon16/Tokio2/3000 tick）

| 样本 | speed_x | SHA16 |
|---|---:|---|
| e2-fused-head-16a（HEAD e613783e，=冻结 eac5e6f2 逐字一致） | 221.822 | eac5e6f2fa225cf0 |
| e2-fused-head-16b | 216.893 | eac5e6f2fa225cf0 |
| e2-fused-candidate-16a（修复前源码） | 230.290 | 091a722a0fea1b90 |
| e2-fused-candidate-16b（修复前源码） | 230.122 | 091a722a0fea1b90 |
| e2-fused-final-16a（终版源码，复核修复后） | 225.665 | 1548a88d27ecfc86 |
| e2-fused-final-16b（终版源码，复核修复后） | 228.012 | 1548a88d27ecfc86 |

HEAD 均值 219.36x；修复前 candidate 均值 230.21x（+4.95%）；复核修复（仅 cfg(test) 测试断言与 import，release 语义不变，但行号变化使产物字节不同）后终版均值 226.84x（**+3.41%**，逐对 +1.7%/+4.0%/+2.8%/+5.1%，如实记录最低对）。全部 4 个 candidate 样本（225.67–230.29）与全部 2 个 HEAD 样本（216.89–221.82）区间完全分离，方向一致无重叠。candidate CPU 约 5.99 核。构建均 -j8、299s deadline、exit0；verify-build 全部通过（HEAD 基线 SHA 与 `.tmp/server-read-only-start-final-candidate` 冻结 binary 逐字一致，构成第三方对照）。终版冻结 `.tmp/server-e2-fused-candidate`。

收益低于结构性预估（~20%）的原因：read-only join 中 hydration（636µs/tick）成为新长杆，branch3 提前完成部分被 join 屏障吸收；join2 中 shards 原本与 first_ready_batch 有一定重叠。1000x 仍未达成。

## 独立复核记录

首轮全新 reviewer（未参与实现）：生产语义全部通过（错误优先级、数据流等价、空 Vec 下游、miss/PreOpen/auction 不变、A 股语义）；1 项阻塞——红灯测试全局计数器严格相等断言在并行 test harness 下被污染（threads=8 复现 3/3 失败）；1 项次要——atomic import 缺 #[cfg(test)]。修复：断言改为前后增量 >= 市场数（延迟初始化时增量 0 仍红灯，污染只增不减）；import 加 cfg(test)。修复后 test-threads=4/8 均 7/7 通过；cfg(test) 行号变化使 release 产物字节改变（091a722a → 1548a88d），按纪律用终版产物重跑基准 ×2（225.665x/228.012x），判定不变。

终审全新 reviewer（第三个 agent，未参与实现与首轮复核）：**PASS**。确认阻塞真正消除（单向假绿风险已在测试注释声明，可接受）、修复未引入新问题（剩余警告均为 HEAD 既有）、首轮通过的关键点（join1/join2 错误优先级、空 Vec 唯一消费者、expiry 分支内联回退）未被破坏。

## 下一步（按本轮数据优先级）

1. hydration 内部（borrowed_existing_projection 1908ms）：sort_identity 533（每 tick 全量 identity 排序）与 final_validate 289；注意 initial-snapshot-index / arrival-sort 已否决的是别的位置。
2. seal_unknown_checks 885ms：`reservations.iter().any(|row| !accounts.contains_key(&row.account))` 的逐行账户查找，可与 validate_resources 单遍融合（保持首错顺序）。
3. inp_identity_collect 503 + inp_sort 646：同一批订单一 tick 内按两种键各排一次。
