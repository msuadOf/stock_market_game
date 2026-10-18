# 382c1871 后当前 critical path 诊断（临时插桩已撤回）

配置：默认 5 股 / 20,007 NPC / seed 42，16 物理核 `0-15`，Rayon16/Tokio2，3000 tick，普通 production server actor Fastest。临时 22 个 span 仅记录同步调用线程 wall/CPU；父子 span 重叠，不能相加。诊断吞吐 218.378x，低于无插桩正式 219.6x 均值，不能作为提速证据。

构建：插桩 release `-j16`、外部 299s deadline，Cargo exit0。首次构建因 20 个临时 span 误写 `engine::performance_profile` 编译失败；修正为 `crate::performance_profile` 后成功。失败不是 TDD 红灯。verify-build 的 `main.rs 与既有真实 actor 探针漂移` 检查因临时插桩 probe 输出 profile 而按预期失败；这是诊断专用 binary，不用于正式验收。采样后 18 份源码已按 manifest 原字节恢复，临时 module 删除。

| stage | calls | wall_s | current-thread CPU_s |
|---|---:|---:|---:|
| actor_protocol_batch | 797 | 13.739 | 5.082 |
| protocol_step_frame | 3002 | 12.800 | 4.179 |
| candidate_commit_prepare | 3002 | 2.317 | 1.237 |
| npc_queue_next_tick | 3003 | 2.057 | 0.979 |
| continuous_shadow_initialize | 2402 | 1.926 | 1.825 |
| borrowed_existing_projection | 3001 | 1.782 | 1.777 |
| read_only_prepare_join | 2098 | 1.779 | 0.019 |
| npc_snapshot_capture | 3003 | 1.765 | 0.696 |
| continuous_input_prepare | 2402 | 1.562 | 1.520 |
| seal_total | 3002 | 1.338 | 1.227 |
| protocol_checkpoint | 3800 | 0.355 | 0.350 |
| engine_frame_after_step | 3002 | 0.161 | 0.158 |
| tick_frame_validate | 6004 | 0.150 | 0.144 |
| snapshot_runtime | 3798 | 0.107 | 0.103 |
| account_validation_context | 3002 | 0.105 | 0.097 |
| protocol_tick_batch | 795 | 0.104 | 0.103 |
| tick_batch_delta | 795 | 0.102 | 0.101 |
| protocol_prepare_frame | 3002 | 0.088 | 0.085 |
| ready_ingress_capture_roots | 3002 | 0.045 | 0.041 |
| public_runtime_state | 795 | 0.030 | 0.029 |
| ready_ingress_capture_sources | 3002 | 0.014 | 0.011 |
| actor_broadcast | 796 | 0.005 | 0.004 |

## 结论

1. **协议/发布路径不是当前主因**。`step_frame` 总 wall 12.8s 中，engine tick 之外的外层 protocol 成本合计很小：checkpoint 0.355s、frame_after_step 0.161s、frame validate 0.150s、snapshot_runtime 0.107s、tick_batch_delta 0.102s、prepare_frame 0.088s。此前静态担心的 NPC order 全量 runtime snapshot 扫描和重复 canonical 验证，实测各仅约 0.1–0.15s/3000 tick，暂不值得先改。
2. **candidate commit 尾部的 NPC next-tick queue 是最大的明显等待段**：wall 2.057s，调用线程 CPU 仅 0.979s，内部约 1.1s 在等待 Rayon/子任务或串行接收。它与 `npc_snapshot_capture`（wall 1.765s / CPU 0.696s）高度相关。
3. **read-only start 只在 2098/3002 tick 命中**；命中时 join wall 1.779s，几乎全部是 `borrowed_existing_projection` 1.777s。未命中的约 904 tick（到期报价或空 ledger）回退旧路径，这部分仍包含在旧各阶段中。并行准备本身没有额外同步 CPU。
4. **CPU 密集段**集中在 `continuous_shadow_initialize` 1.825s、`borrowed_existing_projection` 1.777s、`continuous_input_prepare` 1.520s、`seal_total` 1.227s。这些 parent/child 重叠，不能相加。
5. 下一步优先级：
   - NPC queue/decision snapshot：把纯值决策与投影和串行 `receive_private_intent`/权威 mutation 分离，保持 account/intent 原顺序和失败原子性。
   - read-only start 未命中路径：约 30% tick 因到期/空 ledger 回退，检查是否能让“无到期”的空 ledger/到期扫描与其他只读准备重叠，或减少 due lifecycle 线性扫描。
   - hydration 与 stock input 的共同费用 facts/共享扫描，仍不重复已否决的 initial snapshot 索引、整股缓存、page proof。
