# Behavior market / retail analysis 共享 price paths 实验（2026-10-06）

## 结论：无收益，撤回，不提交

- 诊断依据：`npc-snapshot-findings.md` 显示 `capture_retail_analysis` 内重复执行
  `market_price_path_observations`，且外层 `build_behavior_market_checked` 已为同一 tick
  构建相同 paths。
- 实验改动（仅 3 个源码文件，已全部恢复 HEAD，diff 保留在本文结论中描述，不进入提交）：
  1. `DecisionChainObservation::capture_for_roots` 改为显式传入 paths；
  2. `capture_retail_analysis_with_shared_paths(Some(paths))` 复用 `BehaviorMarketObservation.price_paths`；
  3. decision snapshot capture 调用点在有 retail observer 时共享 paths。
- 行为测试：新增 `sharing_behavior_price_paths_preserves_retail_analysis_results`，
  断言 `Some(paths)` 与 `None` 的 assessments 和 `session_state_hash` 完全一致；该测试单测通过。
- 撤回后复核：三个源码文件已按 HEAD 字节恢复；因实验测试同时被撤回，后续
  `cargo test` 需先重编（首次恢复后 9s 内只完成编译，未执行 case，不宣称撤回后测试通过）。
- 定向短测（实验源码在位时）：`retail_analysis` 10、`decision_snapshot` 24、`decision_resources` 12 通过；
  `decision_chain` 拆分后 69 通过（`--test-threads=16`，单条命令仍在 9s 内）。
  `npc_state_projection` / `npc_tick_preparation` 各有 1 个失败；随后在未应用实验的
  68db9414 源码基线复测同样失败，且失败原因是既有测试 fixture 覆盖策略后丢失
  `base_observation_probability`，与本实验无关。68db9414 是性能 baseline eac5e6f2
  对应源码的稳定前序提交；该复测只用于排除实验 diff，不作为性能对照。完整实验
  patch 另行归档于 `shared-price-paths-experiment.patch`，便于事后复核。
- 正式构建：受控 probe release exit0，`verify-build` binding
  `shared-price-paths-production-build-binding.json` 通过，candidate SHA
  `0089829e65a814252b721bed816c3374117bfc52de174c11f73b0015b79aa232`。
- 正式 3000 tick benchmark（默认 5 股 / 20,007 NPC / seed 42，16 物理核 affinity `0-15`，
  Rayon 16，Tokio 2，顺序独占）：

| label | SHA 前12 | speed_x | wall_s | ticks | CPU核均值 | user核 | system核 |
|---|---:|---:|---:|---:|---:|---:|---:|
| server-production-shared-price-paths-16 | 0089829e65a8 | 219.705 | 13.655 | 3000 | 5.585 | 3.724 | 1.772 |
| server-production-shared-price-paths-confirm2-16 | 0089829e65a8 | 220.393 | 13.621 | 3002 | 5.550 | 3.692 | 1.828 |
| server-production-shared-price-paths-base-16 | eac5e6f2fa22 | 220.785 | 13.588 | 3000 | 5.637 | 3.772 | 1.818 |
| server-production-shared-price-paths-base-confirm-16 | eac5e6f2fa22 | 222.507 | 13.492 | 3002 | 5.610 | 3.751 | 1.823 |

- candidate 均值 220.049x，相邻 frozen baseline 均值 221.646x，相对变化约 -0.72%。
  第一条 confirm 因外部扰动（initialization 1.16s、速度 179.267x）按异常样本剔除，
  未参与均值。
- 判断：共享只减少一次 5 股 price-path 重建，量级远小于 profiling 中
  `CapturedExperienceObservation::capture` / per-account state 的成本；clone paths 与
  新增 Option/参数路径几乎抵消。无统计可见收益，撤回，不进入主线。
- 下一步应转向 `CapturedExperienceObservation::capture` 的 per-account positions、
  `RetailExperienceState` clone、BTreeMap/BTreeSet 构建成本，或
  `continuous_shadow_initialize` / `borrowed_existing_projection` / `seal_total` CPU 段。
