# 公司与回购历史分支归档（2026-11-01）

核对基线：`codex/branch-convergence-2026-11-01`，`28fed1cf4f06d008e2762a9e325a7afd377fcb4d`。以下分支的原 tip 均由 `archive/branch-cleanup-20261101/<原分支名>` 标签保留，原分支引用已删除。关联 worktree 在原 tip 上 detach；执行者记录的操作前后干净状态及 index SHA-256 一致。

| 原分支 | 原 tip | 原关联 worktree |
| --- | --- | --- |
| `feat/engine-repurchase-x-group-tests-28` | `cdcfe43c7b17b67f146486300f03005e56c03ab2` | `.worktree/repurchase-x-group-28` |
| `feat/engine-repurchase-x-transport-28` | `ccaa85265675383082138143a2eab86cc5548128` | 无 |
| `feat/repurchase-order-fact` | `65fa2aa0cf728a472e74e3c1fc65d56251e20812` | `.worktree/luna-repurchase-order-fact` |
| `feat/repurchase-origin-leaf` | `2551fab375e0681df86f14e94e6789d27007e181` | `.worktree/luna-repurchase-origin` |
| `refactor/company-source-read-facts-combined` | `4f65060eea3673cab86d97278c4fcbfd1d0d1b05` | `.worktree/company-source-read-facts-combined` |
| `feat/company-write-worker-2026-10-28` | `8c0cc5a0d407e0d33ec169bd4d9372618654059e` | `.worktree/company-write-worker-2026-10-28` |
| `integrate/company-write-web-db112-2026-10-28` | `ce1b161083f2b3843d74f84040cd40ea3623e461` | `.worktree/company-write-web-integrated-2026-10-28` |

独立只读复核确认，前五项的回购来源、退款身份、Desk 至 Group 运输与公司事实读取在基线有对应实现和测试；后两项的共享 `company-write.ts` 与基线 blob 一致，`db112` 的 `company-write-http.ts` 也一致；各宿主测试名称有对应，未逐行确认语义等价。归档保留原提交与历史验证资料，未对全部历史断言逐行声称等价。独立复核确认引用操作不改变大 A 交易语义。未运行自动化测试。

`integrate/company-write-web-three-transports-2026-10-28` 的 worktree 有未提交内容，未操作。其他覆盖仍不明确的公司核心和 R4 分支亦保留，等待继续核对。
