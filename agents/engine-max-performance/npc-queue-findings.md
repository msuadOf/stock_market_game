# NPC next-tick queue 嵌套诊断（临时插桩已撤回）

固定默认 5 股 / 20,007 NPC / seed 42，16 物理核 `0-15`，Rayon16/Tokio2，3000 tick。临时 5 个 span 记录同步调用线程 wall/CPU，嵌套 span 重叠不能相加。诊断吞吐 161.059x，明显低于正常约 219x；采样期前半段出现约 5 秒低 CPU 窗口，疑似机器/调度外部扰动，因此本数据只用于相对定位，不作为性能对照或正式证据。

构建：诊断 release `-j16`、299s deadline，Cargo exit0。首次诊断 build 因生成 module 的一行 FFI 语法压缩错误编译失败，修正脚本后重新插桩/build 成功；失败不冒充 TDD 红灯。采样后 4 份源码按 manifest 原字节恢复，临时 module 删除，`git diff` 干净。

| span | calls | wall_s | current-thread CPU_s |
|---|---:|---:|---:|
| queue_capture（`capture_decision_snapshot`） | 3002 | 2.745 | 0.852 |
| queue_stream（`stream_npc_decisions` 外层） | 3002 | 0.425 | 0.349 |
| queue_projection_callback | 31,011 | 0.196 | 0.165 |
| queue_receive_callback | 11,840 | 0.034 | 0.023 |
| npc_decide_account | 31,011 | 0.262 | 0.210 |

## 结论

- **不是 `receive_private_intent`**：31,011 个 due account / 11,840 个 intent 的接收累计仅 0.034s wall、0.023s CPU。
- **不是纯策略 decide**：`npc_decide_account` 累计 0.262s wall、0.210s CPU；projection callback 也仅 0.196s。
- **大头在 `capture_decision_snapshot`**：诊断样本 2.745s wall，调用线程 CPU 0.852s。与上一轮 22-span 结果（npc_snapshot_capture wall 1.765s / CPU 0.696s）方向一致，说明大头在 snapshot capture 内部，而不是 stream/receive。
- 下一实验应拆 `capture_decision_snapshot` 的内部字段：attention mutate、retail observation/experience、working order capture、retail analysis、`DecisionSnapshot::new_shared`；尤其注意 20,007 NPC 下 per-account `CapturedExperienceObservation::capture` 与 `working_orders_for_accounts` 的全簿过滤。不能把 31k 个回调各自 8 微秒的总量误当成阻塞瓶颈。
