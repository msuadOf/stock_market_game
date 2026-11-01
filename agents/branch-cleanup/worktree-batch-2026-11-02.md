# 工作树关联 branch 批量核销

冻结基线为 `6fe53f65265e186b62dfcb1d510a993f91b07e11`。下表各 branch 的基线外非 merge 提交经 `git cherry` 均为补丁等价，不表示原提交是基线祖先。独立复核确认各工作树干净、未发现匹配路径的活动进程。执行前复核工作树状态、HEAD、补丁等价与进程 cwd；将 worktree 切为原提交 detached，删除 branch 引用，保留 worktree 及其提交。进程检查只能反映当时状态。

| 原 branch | 原 HEAD | worktree |
| --- | --- | --- |
| codex/branch-cleanup-record-20261101 | 5c82fc9f | stock_market_game-branch-cleanup |
| codex/regression-parallel-20261101 | 24fcb1ca | .worktree/regression-parallel-20261101 |
| codex/candle-fixture-split-r40 | 1a4869bb | /tmp/stock-candle-fixture-r40 |
| codex/convergence-plan-20261102 | 99ba145f | /tmp/stock-convergence-plan-20261102 |
| codex/current-handoff-20261101 | 9d0a85de | /tmp/stock-current-handoff |
| codex/facade-exit-order-r42 | d91ab905 | /tmp/stock-facade-exit-order-r42 |
| codex/branch-convergence-handoff-2026-11-01 | bc308174 | /tmp/stock-handoff-20261101 |
| codex/isolate-report-case-r39 | 097d8e43 | /tmp/stock-isolate-report-case-r39 |
| codex/performance-final-record | d8f1539b | /tmp/stock-performance-final-record |
| codex/r55-web-timeout | 3fc410a5 | /tmp/stock-r55-web-timeout |
| codex/r57-regression-fixture | 11e9031c | /tmp/stock-r57-regression-fixture |
| codex/regression-throughput-r37 | 9fba8046 | /tmp/stock-regression-throughput-r37 |
| codex/supervisor-schedule-r50 | 0c58fcd8 | /tmp/stock-supervisor-schedule-r50 |
| codex/web-regression-r38 | fb8158bb | /tmp/stock-web-regression-r38 |
| codex/runtime-convergence-20261101 | 04831a1e | /tmp/stock-runtime-convergence |

相对路径位于仓库根目录；`stock_market_game-branch-cleanup` 位于仓库同级目录。共删除 15 个已等价吸收的 branch。内容涉及测试调度、测试夹具、性能记录和交接记录，未修改 A 股交易语义；未运行产品测试。
