# Q05：scripts 测试持续入口

## 决策与范围

用户逐项讨论后选择“接入根测试、手动开发 CI 和独立入口”。正式执行器为
`scripts/run-script-tests.mjs`，根 `test` 的 sealed execute 调用同一批次函数；
现有手动 CI 已调用该 execute，无需改变 workflow 或另起重复批次。
独立开发入口为 `corepack pnpm test:scripts`。产品发布只构建的政策不变，
不修改普通 commit 自动触发、交易制度、存档或任何其他尚未确认的 Q 项。

开工全文读取根 AGENTS、principles、testing、open-questions、ADR-0028，
现有 full-regression/Web runner 和相关测试、Q05 原文及 reaudit-tools。
已有 `agents/oop-release-validation/run-scripts-tests.mjs` 的递归发现与逐文件
隔离执行不是缺失能力；本次将该算法正式化，不让长期开发入口依赖工作记录目录。
旧任务 runner 和历史运行证据保留，不把历史通过结果计入本轮验证。

## 实现边界

- 递归发现整个 `scripts/**/*.test.mjs`，排序，拒绝零文件、重复路径和 symbolic link；
  新建深层测试与 runner 自测都自然纳入，不维护手工文件清单。
- 每文件独立 Node 进程，文件内 concurrency=1，case timeout=10000ms，
  文件进程树 deadline 不超过 10000ms；没有 test-force-exit。
- 按可用 CPU 最多 4 个文件 worker 并行；正常结束释放 worker 后继续队列。
  失败取消在途 siblings、停止队列，等待实际清理，保留每项实际失败及 cause。
- 独立聚合是明确的长验收，进程外 supervisor 为 300000ms，预留 1000ms 清理。
  根回归调用批次函数，沿用 execute 已有的进程外期限及剩余预算，不另起五分钟窗口。
- 根 inventory 指纹由少数 runner 文件扩为整个 `scripts/`，新测试或工具修改使旧
  inventory 失效。没有引入新契约版本或旧版本兼容。
- 测试直接启动 Node，绝不递归执行 `pnpm test`；runner 自测通过注入执行器
  验证生产批次入口，不再次启动完整脚本测试或根回归。

## TDD 与短测证据

先增加“根回归纳入完整 scripts 发现入口及其源码指纹”测试，使用十秒进程外门禁、
case timeout=10000ms 和 test-isolation=none 执行；红灯为
`steps.some(step.kind === "script-test-batch")` 断言失败，而非 import/编译失败。
实现后曾发现新指纹测试在 before 声明前使用变量，已修正位置，不削弱断言。
沙箱首次执行阻止已有 fixture 的 Node 子进程，使用审批后的本地定向命令复验。

用户确认后补充正常并行重叠、真实挂起进程终止及 Windows 路径分隔符用例。
定向验证只运行以下两个文件，不运行整个 scripts 集或完整回归：

```sh
node scripts/run-with-deadline.mjs 10000 -- node --test --test-timeout=10000 --test-concurrency=2 scripts/run-script-tests.test.mjs scripts/run-full-regression.test.mjs
```

2026-10-04 执行 Node v25.8.2，两个真实测试进程并发，43/43 通过、0 失败，
reporter wall-clock 为 2366.541505ms。真实挂起 fixture 使用 600ms 总门禁、
150ms 清理预留，确实拒绝且约 519ms 完成；这只是代表性 watchdog 单测，
不是长脚本聚合验收。根接线测试使用替代执行器核对全部实际发现文件恰好执行一次、
每文件门禁及共享期限，不冒充全部工具行为已重新通过。

非作者 `review_q05_entry` 已完整读取全部diff、新runner、现有root/手动CI execute链
与正式文档；三项门禁通过。沙箱初次复验39/43、四个既有真实子进程fixture受EPERM/
输出限制，未计作全绿；审批后的同一两文件定向复验43/43通过，wall-clock2750.773604ms，
没有削弱断言。未运行完整脚本集或完整回归，也未接入产品发布测试。
