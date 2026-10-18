# 当前 HEAD（39b58fb6）全规模瓶颈地图（2026-10-06，临时插桩已撤回）

配置：默认 5 股 / 20,007 NPC / seed 42，16 物理核 `0-15`，Rayon16/Tokio2，3000 tick，production server Fastest。本轮为 33-span 一次性插桩（比 382c1871 时代 22-span 增补 NPC 内部、拍卖/结算/校验相位）+ 官方 binary（30ce134d）gdb 全线程 60 组采样。诊断吞吐 235.05x / wall 12.76s（run3；run1 230.76x 复证），处于正常区间但**全部数字只作相对定位，不作提速证据**。产物：`.tmp/profiling-20261006/`（instrument.py/rollback.py、build*.jsonl、run1-3.json、run*-summary.txt、gdb-mi-raw.log、gdb-samples-mi.log、thread-states.log、stack-probe.py）。

插桩工程备注：①同文件多 span 的 manifest 快照 bug 两轮修复（第二轮曾致 speed 171x 失真样本 run2，已作废）；②conservation `validate_with_preseal` 每行一次（3000 tick 内 20,890,466 次），span 开销使整 run 失真，该粒度不可插桩，守恒成本以 E4 zero-op findings 为准。

## 主 span 表（run3，3002 tick，wall 12.76s；父子重叠不可加）

| stage | calls | wall_ms | cpu_ms | wait_ms |
|---|---:|---:|---:|---:|
| actor_protocol_batch | 744 | 12757.7 | 5158.2 | 7599.5 |
| protocol_step_frame | 3000 | 11214.1 | 3648.8 | 7565.2 |
| read_only_prepare_join | 2096 | 2644.8 | 17.8 | 2626.9 |
| continuous_shadow_apply | 2100 | 2524.0 | 905.3 | 1618.6 |
| process_stock_step | 12000 | 2471.9 | 2400.1 | 71.7 |
| candidate_commit_prepare | 3000 | 2351.9 | 1267.3 | 1084.6 |
| npc_queue_next_tick | 3001 | 2096.1 | 1011.3 | 1084.8 |
| npc_snapshot_capture | 3001 | 1776.3 | 703.6 | 1072.8 |
| borrowed_existing_projection | 2999 | 1750.6 | 1745.1 | 5.4 |
| continuous_shadow_initialize | 2400 | 1734.3 | 1644.8 | 89.5 |
| auction_shadow_apply | 600 | 1658.7 | 120.4 | 1538.3 |
| seal_total | 3000 | 1349.8 | 1250.6 | 99.1 |
| continuous_input_prepare | 2400 | 946.0 | 927.2 | 18.8 |
| validate_private_market_ledger | 14985 | 891.8 | 818.5 | 73.3 |
| protocol_checkpoint | 3745 | 350.2 | 345.4 | 4.8 |
| stream_npc_decisions | 3001 | 259.3 | 243.9 | 15.4 |
| quote_expiry_apply | 904 | 178.6 | 176.8 | 1.8 |
| engine_frame_after_step | 3000 | 164.7 | 161.5 | 3.2 |
| account_validation_round | 2843 | 109.8 | 43.8 | 66.0 |
| settlement_prepare(with_beliefs) | 3000 | 32.2 | 28.1 | 4.1 |
| retail_analysis | 31001 | 65.6 | 30.6 | 34.9 |
| 其余（snapshot/tick_batch/prepare_frame/context/roots/sources/ledger_apply/receive/validate_round/broadcast） | — | 各 <110 | — | — |

帧构成 3000 = PreOpen 300 + CallAuction 600 + Continuous 2100；join 命中 2096/2100。

## gdb 全线程采样（官方 30ce134d，12000 tick，诊断 250.83x）

1080 份 worker 线程样本：**70.5% 在 `rayon_core::sleep` 睡眠**、8.5% 在 `sched_yield` 空转（rayon 等工作的 spin-yield）、3.7% 在 `LockLatch::wait_and_reset`（join 闩等待）、**仅 ~5% 在执行引擎工作**（可见帧：PrivateStockLedger::get、LedgerPagedMap::get、process_continuous_stock memcmp、validate_private_order、consume_iter memcpy、clone/_rjem_malloc）。主线程 120 份样本 100% 停在 tokio park。/proc 状态 60.4% S / 39.6% R。结论：**并行池绝大部分时间空闲，工作量本质上 ~1–2 个活跃 worker；扩展性受限于串行 tick 依赖链而非池配置**——与"16 worker 反而慢于 8/4 worker"的倒 U 现象（expl 数据 4w≈293x 诊断）一致。

## 综合瓶颈表（按 wall 占比 × 可压缩性排序；上限=该项清零且其余不变的 Amdahl 折算，假设注明）

| # | 瓶颈 | 成本 | 瓶颈性质 | 修复方向类别 | 清零上限 | 与否决清单关系 |
|---|---|---|---|---|---|---|
| 1 | read-only join 三分支 | wall 2.64s (20.7%) | branch3（input 0.95 + shadow_init 1.64）为长杆，单一大股主导；hydration 1.75 与 seal 1.35 被 branch3 遮蔽 | 削 branch3 工作量（E4：四遍未变行全量扫描=0.86s+0.53s+0.53s…；写集驱动跳过未变页） | branch3 清零→10.5s→285x；整 join 清零→10.1s→294x | 页级缓存/page proof/稀疏 ledger 已否决；dirty-key splice（expl ③）未否决 |
| 2 | continuous_shadow_apply 本体 | wall 2.52s / CPU 0.91s | 每连续 tick 1.20ms wall，内含 stock processing（process_stock_step 2.47s CPU 在 worker）与校验相位 | 削单股工作量/跨 tick 重叠 | 清零→10.2s→288x | — |
| 3 | candidate_commit 链（含 NPC 队列） | wall 2.35s；wait 1.08s = npc_snapshot 内串行 par 相位 | E5 证明是尾延迟非空闲；到期账户仅 ~20/tick，单条 retail ~140µs | 削单条工作量；或跨 tick 投机预计算（expl ②：2084/2084 等价、外推 +16%） | 清零→10.4s→288x | 通道合并已撤回（E5）；投机预计算未否决 |
| 4 | auction_shadow_apply（600 拍卖 tick） | wall 1.66s / CPU 0.12s（每 tick 2.76ms，93% 等待） | **本轮新发现：拍卖 tick 占 13% wall 且几乎纯等待**，内部未细分（待查盲区） | 先插桩细分再定 | 清零→11.1s→271x | 未涉 |
| 5 | process_stock_step CPU | CPU 2.47s（12000=5 股×2400 tick；内嵌 validate_private_market_ledger 0.89s） | 真实业务计算+每行物化；wall≈CPU 纯算 | 削物化/校验融合（E4 教训：纯删冗余收益≤1.2%） | 属 #2 内部 | 校验融合部分变体已否决 |
| 6 | protocol 外层 | ~0.63s（checkpoint 0.35+after_step 0.16+prepare 0.08） | 纯串行小项 | 低优先 | 清零→272x | — |

**总上限检验**：把 #1–#4+外层全部清零（12.76−9.8=2.96s）≈ 1014x——**即 1000x 要求当前已识别的每一段串行 wall 几乎全部消除**。任何单项做不满，必须组合：跨 tick 重叠（commit 期间预计算下一 tick join）+ branch3/物化跳过 + 拍卖 tick 等待消除 + worker 数调整（expl ①：4w 峰值外推 +15~35%，未否决）。

## 与旧 22-span（382c1871）结论的差异

1. **candidate_commit 与 npc_queue 的"等待"已定性**（E5 本轮复证 1.08s）：NPC 串行 par 相位尾延迟，非调度空闲。
2. **拍卖 tick 是此前完全未见的 13% wall 纯等待段**——旧 22-span 没有任何拍卖相位 span。
3. join 长杆自 E3 后保持 branch3（capture+shards），hydration/seal 已被并行遮蔽，与旧"hydration 1.78s 长杆"结论相反。
4. retail_analysis 方法本体仅 65.6ms/31001 次（~2µs/次）；旧 0.865s 的"retail 分析"成本实际在 experience 捕获/物化等外围（E5/本轮 wait 差 1.07s 归入 npc_snapshot_capture 内部，细分待查）。

## 未测/局限

- conservation 逐行校验：粒度过细不可 span（20.9M 次/tick 轮），成本沿用 E4 zero-op findings。
- npc_snapshot_capture 内部（experience 捕获 vs market view vs 其它）未再细分（沿 E5 结论）。
- 拍卖 tick 内部相位未插桩（#4 待查）。
- PreOpen 300 tick 未单独 span。
- run2 为失真样本（171x，conservation 插桩污染），已作废不入结论。
