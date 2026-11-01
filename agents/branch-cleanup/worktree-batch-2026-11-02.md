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

第二批冻结基线同上，以下 6 条关联 worktree 均干净，先以原 tip 创建 `archive/20261102-<短 SHA>` tag，再将 worktree 切为同一提交的 detached HEAD 并删除 branch。切换前核对 HEAD、状态和进程 cwd；独立复核确认内容依据。保留 tag 以保存可能有用的原始历史。

| 原 branch | 原 HEAD | 依据 |
| --- | --- | --- |
| fix/plan-identity-runtime | b41c8fb2 | 前五项补丁等价，末项增删行与基线祖先 14f5c703 按文件忽略空白后相同。 |
| codex/wasm-runtime-override-20261029 | b57398a0 | 增删行与基线祖先 8a5ca157 按文件忽略空白后相同，初始化行为与测试保留。 |
| codex/clippy-idioms-runtime-20261030 | 2043d5fc | 与基线祖先 ce4b20ba 的改动一致，仅 tick 行缩进不同。 |
| codex/company-archive-record-20261101 | ad550ec5 | 公司归档记录补丁等价。 |
| codex/company-convergence-20261101 | b2f0d353 | 配股除权与简税结算两项补丁等价。 |
| feat/company-disclosures-candidate | 316ca881 | 基线保留更正候选推进、具体错误原因链及相关测试；提交本身不属于补丁等价。 |

前三项的对应提交 patch-id 不同，增删行对照不能单独证明全部上下文语义相同，因此保留原 tip tag 和 worktree。六条 worktree 均保持原提交；没有运行产品测试。

第三批同样使用冻结基线 `6fe53f65`。以下四条均保留原 tip tag `archive/20261102-<短 SHA>` 和原工作树 detached HEAD，删除 branch 前复核 HEAD、工作区及进程 cwd。独立审查确认如下内容依据：

| 原 branch | 原 HEAD | 依据 |
| --- | --- | --- |
| feat/b4c-share-transform-r3a | d239ab47 | 六项补丁等价，余项与基线祖先 46f7692c 按文件忽略空白的非空增删行相同。 |
| feat/b4d-information-prepare | 51ca08c8 | 三项补丁等价，余两项分别与基线祖先 1e54f18b、9d74e819 按文件忽略空白的非空增删行相同。 |
| fix/share-split-zero-issued | 2c331b5c | 与基线祖先 a568584f 按文件忽略空白的非空增删行相同。 |
| codex/regression36-inline-repair | c000d475 | 三组 session 回归夹具的断言保留于基线，两个提交并非补丁等价。 |

增删行对照并非 patch-id 相等或最终上下文的完整语义证明，因此保留原 tip tag。前两处相关 worktree 中，share-transform 的 ignored `bindings/` 及 share-split 的 ignored `.tmp/` 也随完整 worktree 保留；未清除 WIP 或运行产品测试。

第四批将 `integrate/source-a-latest-validation-2026-10-27` 的 `bd1f1dfb` 归档为 `archive/20261102-bd1f1dfb`，保留原工作树的同提交 detached HEAD 并删除 branch。独立对照确认公司精确股数、root 与 retail 正每股估值等新增断言在基线保留；基线调整了公开前史和财务 fixture，因此不称补丁等价。操作前工作树干净并检查进程 cwd；未运行产品测试。

第五批将 `feat/registry-physical-day-phase` 的 `23759efa` 转为 `archive/20261102-23759efa`，worktree 保持原提交的 detached HEAD。该分支唯一提交的新增与删除内容在基线祖先 `6578cff5` 中均有对应，五处旧 `corporate_actions` 访问已适配为 `company_state.actions()`；基线祖先含其他改动，不能称完整 diff 相同。独立复核了存档来源校验和测试适配，操作前确认工作树干净并检查进程 cwd。未运行产品测试。
