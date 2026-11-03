# run-long-validation-process 1600ms 终态与 slow-proc 注入修复

- 基线：`5d5fb649`（当前集成 HEAD）
- 独立 worktree：`.worktree/deadline-test-audit-fix`
- 集成树未修改
- 日期：2026-11-03

## 问题

`scripts/run-long-validation-process.test.mjs` 在 5d5fb649 的完整回归中记录到
`1600.91ms` / `1610.82ms`，且 `slow-proc` 模式没有真正覆盖 production cleanup：

- 原测试用父进程 `close` 回调计时，包含父事件循环与报告调度迟延，无法区分 CLI
  真实终态和观察者迟延。
- 原 preload patch 的是 `node:fs` 的同步 `readFileSync`；`scripts/run-with-deadline.mjs`
  的 **当前 production** 使用 `node:fs/promises` 的 `readFile`/`readdir`，所以该模式
  主要拖慢 fixture child 自己的同步 stat 读取，没有进入 cleanup 扫描。
- 原先 `finalSignalReserve = min(100, reserve/2)`、`finalReturnReserve = min(50, reserve/4)`；
  在 `cleanupReserve≈300ms` 时 hard rejection 只比 final signal 晚约 50ms，高负载下
  定时器派发迟延会把 CLI 真实终态推过 1600ms。

## 修复

1. 新增 `scripts/run-long-validation-process-test-support.mjs`：独立 observer 先写
   `observer-ready`，父测试回 `observer-go` 后才在 spawn 前记录
   `process.hrtime.bigint()`；随后轮询 CLI 与 owned child 的 `/proc/<pid>/stat`，
   按 PID+birth 记录 Z/X/消失终态。父测试的 `close` 回调延迟不再进入 1600ms 预算，
   但 observer 的真实终态必须 ≤1600ms。
2. `slow-proc` preload 改为 patch `node:fs/promises` 的 `readFile`，对
   `/proc/<pid>/stat` 的前 16 次读取异步延迟 50ms，并写入每次命中的 PID、路径、
   `hrtime` 和调用栈。测试要求：至少一次命中发生在真实
   `total - cleanupReserve` execution cutoff 之后，且至少一条命中栈包含
   `run-with-deadline.mjs` 的 `terminateTree`/`tracedChildren`/`linuxProcesses`，
   从而直接证明 production cleanup 路径被拖慢。更早的 final cutoff 会先终止
   尚未完成的 cleanup，因此不要求一定跑到 `linuxProcesses` 全表扫描。
3. `scripts/run-with-deadline.mjs` 新增 `cleanupReservePhases`：在 300ms reserve 内
   先安排 final signal（200ms），再保留 hard rejection 返回缓冲（150ms）；
   `reserve < 3ms` 时只安排 final signal，hard rejection 落在原 deadline。
   全部仍来自原 cleanup reserve，不扩大外部 deadline。脚本测试直接验证
   timer 顺序，以及 hard rejection 不会取消尚未执行的 final owned-handle kill。

## 红绿记录

修复前实测：原测试在同一 worktree 的两次运行分别得到
`wall 1614.528ms` 与 `1610.658ms` 失败；旧 cutoff 下独立 observer 还记录到
CLI 终态 `1602.758ms`，owned child 终态 `1522–1546ms`。这证明 CLI 本身存在
真实轻微越界，不能只归因于父 close 派发迟延。新增 `cleanupReservePhases`
导入测试在实现前以“does not provide an export”失败；新增 observer 测试在旧
preload 下会因 `hits` 为空失败。

修复后，6 个相关文件并行运行（每个 `--test-timeout=10000`、外部 10s）：

| 文件 | 结果 |
|---|---|
| `scripts/run-with-deadline.test.mjs` | 24/24 pass |
| `scripts/run-with-deadline-cleanup.test.mjs` | 16/16 pass |
| `scripts/run-long-validation.test.mjs` | 4/4 pass |
| `scripts/run-long-validation-process.test.mjs` | 2/2 pass |
| `scripts/run-full-regression.test.mjs` | 14/14 pass |
| `scripts/run-script-tests.test.mjs` | 8/8 pass |

压力复跑：4 份 `run-long-validation-process.test.mjs` 并行，两个 mode 共 8 个 case
全部通过；最新一次 CLI 终态最大 **1531.522ms**，owned child 终态最大
**1446.516ms**，均 ≤1600ms。`slow-proc` 注入最多延迟 16 次读取，但在更早的
final cutoff 下实测每个 case 命中 4 次；测试把这些命中换算成 ms，并与 CLI
错误中真实的 `total - cleanupReserve` execution cutoff 比较，证明命中发生在
cleanup 阶段，而不是 initial root 绑定。原始日志在 `evidence/`，汇总见
`evidence/stress-summary.json`。

完整 `scripts` 批次记录：

- cleanup 尾部为 200ms/100ms 时：60 个测试文件、4 个并发 worker、case 10000ms、
  单文件外部 10s deadline，**31.701s** 完成，exit 0，无失败。
- cleanup 尾部改为 200ms/150ms 后，受当前外部负载影响，两次完整批次重试分别于
  约 21.5s / 22.1s 失败在 `rust-test-case-supervisor-execution`、
  `rust-test-case-supervisor-lifecycle`、`rust-test-case-supervisor-export(-timeout)`
  的 10s 文件级 deadline。失败原始日志保存在 `evidence/run-script-tests-batch.log`。
  这些文件隔离复跑均通过：execution 8.44s、lifecycle 8.06s、export 4.41s。
  该失败可能是外部负载导致的既有 10s 门限抖动，但没有完整批次绿灯，不能宣称全绿。
- worktree 初始没有依赖目录；为运行完整批次，临时从当前集成 worktree 链接了
  `node_modules`（未提交），批次结束后已移除。

## 边界

- 仅 Linux 有独立 `/proc` 终态观察；非 Linux 保持原 skip。
- 未运行完整 300s `all` 回归、Windows 实机或 browser 验收。
- observer 自身在 3000ms 内未观察到终态会标记 `timedOut` 并失败，不能用观察器
  清场冒充 CLI 在 1600ms 内退出。
- 本改动只涉及 deadline 工具与测试，不改变交易语义、存档或 API。
