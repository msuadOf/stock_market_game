# Read-only continuous start 最终正式基准（2026-10-06）

- 配置：默认 5 股 / 20,007 NPC / seed 42，16 物理核 affinity `0-15`，Rayon 16，Tokio 2，普通 production probe，3000 tick；四条 benchmark 顺序独占执行，无测试/构建重叠。
- candidate release：SHA `eac5e6f2fa225cf0944c757cd31e1097ff3a08f02d7acd010485da968d10b796`，verify-build binding `read-only-start-review-fix-production-build-binding.json` 全部通过。
- baseline：稳定 68db9414 冻结 binary `.tmp/server-stock-row-parallel-final-candidate`，SHA `a643e60b516d207a8652b16ea9c58c70cfd0accf91fca6d16f9580b2f6d95c4f`。

| label | SHA 前12 | speed_x | wall_s | ticks | CPU核均值 | user核 | system核 |
|---|---:|---:|---:|---:|---:|---:|---:|
| server-production-read-only-start-final-16 | eac5e6f2fa22 | 222.757 | 13.472 | 3001 | 5.982 | 4.048 | 1.923 |
| server-production-read-only-start-final-confirm-16 | eac5e6f2fa22 | 216.501 | 13.866 | 3002 | 5.984 | 4.043 | 1.929 |
| server-production-read-only-start-final-base-16 | a643e60b516d | 189.914 | 15.802 | 3001 | 5.342 | 3.631 | 1.701 |
| server-production-read-only-start-final-base-confirm-16 | a643e60b516d | 189.614 | 15.832 | 3002 | 5.311 | 3.559 | 1.741 |

- candidate 均值 219.629x，baseline 均值 189.764x，相对提升 15.74%；candidate CPU 均值约 5.98 核。
- 1000x 仍未达成，goal 保持 active；本结果只证明本批次收益，不作为最终目标完成证据。
