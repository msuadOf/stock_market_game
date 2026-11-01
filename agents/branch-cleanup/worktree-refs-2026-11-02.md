# 关联 worktree 的历史 branch 核销

核对时基线 `codex/branch-convergence-2026-11-01` 为 `1a23c4fa23eac0bbb6c5311fc4c3648953e4e51c`，它在核查期间持续前进。`codex/retail-event-docs-20261101` 的 `a4cbc6405f6a53043cbe62b33b12dc0751d68d49` 是该基线祖先，提交内容为运行时同条件性能比较记录。关联 `/tmp/stock-retail-event-docs` 状态干净；独立复核也确认没有已知进程使用该目录。操作前再次确认祖先关系与干净状态，将该 worktree 切至同一提交的 detached HEAD，再删除 branch。worktree 与提交均保留；操作后状态干净。

本次共删除一个已吸收 branch。其他关联 worktree 的 branch 未完成逐项内容核销；基线、main、当前活跃任务及含 WIP 的工作树均不作处理。上述操作不改变 A 股交易语义，也未运行产品测试。
