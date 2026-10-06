# CapturedExperienceObservation 内部诊断（临时插桩已撤回）

固定默认 5 股 / 20,007 NPC / seed 42，16 物理核 affinity `0-15`，Rayon16/Tokio2，3000 tick。临时 6 个 span 在 `CapturedExperienceObservation::capture` 内记录 wall/CPU；span 在 Rayon worker 内执行，CPU 按调用 worker 计。诊断吞吐 214.377x，低于正常约 220x 区间，只用于相对定位，不作为提速证据。

构建：诊断 release `-j16`、外部 299s deadline，Cargo exit0。插桩生成过程有两次工具生成失败：首次 manifest 序列化误把 `Path` 作为 JSON key；随后未成功写 manifest 时源码已被改动，需先按 HEAD 恢复再重跑；另有一次 `let...else` scope / risk loop scope 语法错误，经 `rustfmt` 暴露后修正。这些是诊断工具生成失败，不是 TDD 红灯。采样后 3 份源码按 manifest 原字节恢复，临时 module 删除，`git diff` 干净。

| stage | calls | wall_s | worker CPU_s |
|---|---:|---:|---:|
| experience_get | 31,027 | 0.101 | 0.061 |
| experience_clone | 31,027 | 0.144 | 0.114 |
| positions_equity | 30,887 | 0.138 | 0.109 |
| holding_observation | 30,887 | 0.126 | 0.097 |
| held_set | 30,887 | 0.077 | 0.048 |
| prune_watchlist | 30,887 | 0.068 | 0.040 |

## 结论

- 六段合计 wall 约 0.654s、CPU 约 0.469s，与上一轮 `experience_capture` 0.844s wall / 0.772s CPU 的数量级一致；差值包括 span 外的 `Arc::new`、struct 组装与调度。
- **最大单段是 `RetailExperienceState` clone（0.144s wall / 0.114s CPU）**，但绝对量仅约 0.1s/3000 tick。`AccountPagedMap::get`（0.101s）和 positions/equity（0.138s）次之。
- `prune_watchlist` 与 held set 总计仅约 0.145s wall / 0.088s CPU；即使完全消除，也远不足以支撑 4.5 倍提速。
- 因此 `CapturedExperienceObservation::capture` 不是当前达到 1000x 的主杠杆。尽管 clone/get 的结构性去重（如共享 Arc + copy-on-write）是正确方向，但按本轮量化预期收益不足，不能先做。
- 下一优先级转向 22-span 中更大的 CPU 段：`continuous_shadow_initialize` 1.825s CPU、`borrowed_existing_projection` 1.777s CPU、`continuous_input_prepare` 1.520s CPU、`seal_total` 1.227s CPU（父子重叠不能相加）。尤其 read-only start 未命中的约 904/3002 tick 与 `npc_order_lifecycles.iter().any(...)` 的 O(n×到期数) 线性扫描。
