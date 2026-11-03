# proc-timeout-fix：正式 all 中 supervisor EACCES 断言收到 cleanup deadline 的定位与修复

对象：独立 worktree `.worktree/proc-timeout-fix-7b9f60bc`，起始 HEAD
`7b9f60bc5640f142b093bdadeceea7d535b9a74c`。
依据：`.worktree/branch-convergence-final-20261102/agents/branch-convergence-final-validation/README.md`
（7b9f60bc 单批 `all` 在 `scripts/rust-test-case-supervisor-proc.test.mjs` 失败）。

## 现象与复现

正式批次失败子用例为「真实活 owned 的 non-dumpable environ EACCES 仍必须显式失败」，
期望 `/cannot confirm owned process <pid>.*EACCES/`，实际得到
`case token cleanup deadline was exhausted before process tree confirmation`。

隔离复现（本 worktree、load1≈48、`/proc` 约 2100-2200 个进程）：

```
node --test --test-reporter=spec scripts/rust-test-case-supervisor-proc.test.mjs
```

修复前连续 10 次：两个真实 fixture 子用例耗时 **570-1365ms**（预算 1000ms），
**5/10 次运行失败**，失败均为同一 deadline 文案。

## 定量定位

`createCompletionCleanup().flush()` 每次 case 完成都全表扫描 `/proc`，对每个条目先读
`environ`，`EACCES` 时再读 `/proc/<pid>/stat` 判断 `ppid`/`pgid` 是否等于 launcher。
注入 hook 计时（单次 cleanup，约 2100 个进程）：

- `readdir("/proc")`：8-20ms；
- `readEnvironment` 约 2100 次（其中约 2000 次是其它用户的 EACCES）：累计 110-490ms；
- `readProcessStat` 约 2000 次：累计 0.3-1.6s，墙钟约 0.3-0.6s。

即单次清理是 **O(机器进程数) × 两次 fs 读取**，且原实现对全部条目使用无界
`Promise.all`。`node:test` 下同一段代码比裸 `node` 慢 2-3 倍（线程池争用），是隔离通过、
批次失败的放大因素。这两次读取都不能删：environ 用于发现 token 持有者，stat 用于
EACCES 候选的状态/birth/归属判断。

并发上限实测（同机、`node --test`、2100 项仅 stat 扫描、12 轮交错）：

| 策略 | 中位 | 尾部 |
|---|---|---|
| 无界 `Promise.all` | 218ms | 319ms |
| 32 项一批 + `setImmediate` | 173ms | 192ms |

## 被否决的方案：按出生时间跳过旧进程

曾实现“读取 stat 后跳过 starttime 早于 launcher 的进程，不读其 environ”，并在隔离下
把该文件降到 10/10 通过。**该方案已撤销**：Linux `execve` 不改变 `/proc` starttime，
出生早于 launcher 的旧进程可以在之后 exec 到带 `STOCK_GAME_RUST_CASE_TOKEN=<launcher.pid>:<seq>`
的新映像；token 可预测，协议也没有限定只能靠继承获得。原实现会清理这种 token 持有者，
按出生过滤会静默漏掉它，属于削弱原有保护。独审给出静态判据：listing=`['401']`、
stat.birth=`1999`、`launcherBirth=2000`、environ 精确含 active token 时，旧实现 terminate 401，
过滤实现 environ/terminate 均为 0。

## 最终修复

`scripts/rust-test-case-supervisor.mjs`：

1. **保留全量 environ 保护**：列举到的每个 PID 都必须读 environ，不按出生时间/uid 跳过。
2. 整机扫描按 `PROCESS_SCAN_CONCURRENCY = 32` 分批，批次之间 `setImmediate` 让出事件循环，
   避免一次发起约 2100 个并发读取；任何一批失败仍立即停止后续批次（fail-fast 不变）。
3. 其余语义原样：首次 environ 只筛候选，归属由同 birth 内双读 environ 确认；
   EACCES/EPERM 的活候选仍显式失败；Z/X 仍跳过；birth 变化仍报 identity changed；期限未改。

`scripts/rust-test-case-supervisor-proc.test.mjs`：

1. 两个真实 fixture 只列举**本 fixture 的 PID**（真实 `/proc` environ 与 stat 不变）：
   该用例验证的是 EACCES 归属语义，不再把整机 2100+ 进程数塞进 1000ms 预算。
2. 新增「出生早于 launcher 的旧进程在 exec 后携带 token 仍必须被发现并清理」，
   并显式注入 `launcherBirth: "2000"`：被否决的 c5992f76 实现会因此跳过 PID 401，
   该用例实测变红；最终实现为绿。
3. 新增「整机 /proc 扫描并发有上限」，并断言列举中的每个 PID 都被检查（无跳过）。

## 验证结果

| 项 | 结果 |
|---|---|
| `rust-test-case-supervisor-proc.test.mjs` 单跑 10 次（`timeout -s KILL 10s`） | **10/10 通过**；真实 fixture 子用例 59-91ms（预算 1000ms） |
| 同一文件 4 路并发 × 3 轮（`timeout -s KILL 10s`，共 12 次） | **12/12 通过** |
| 相邻 supervisor 套件（supervisor、blocked、execution、lifecycle、export、export-timeout） | 全部通过（2/1/3/10/2/1 例） |
| `run-with-deadline-cleanup.test.mjs` | 16/16 通过 |
| `run-long-validation-process.test.mjs`（slow-proc 注入仍命中 `fs/promises` 清理链） | 2/2 通过 |

撤销前（含出生过滤）独审曾复现 4 路并发 4/4 失败（1172/1119/1117/1273ms）；该结果与
出生过滤方案一并作废。最终版本在相同 4 路并发下 12/12 通过。

## 边界与限制

- 未运行正式单批 `all`、全仓回归、Clippy/fmt 之外的长验收（按任务要求不跑全仓）；
  端到端绿需 owner 在正式单批上复验。
- 生产清理仍是 O(机器进程数) 的双读扫描，本修复只限制并发和峰值、修正测试的预算依赖；
  机器负载极高时，若共享期限本身耗尽，报 deadline 仍是正确行为。
- 真实 fixture 收窄了列举范围；全表逐项检查由注入列举的用例覆盖，生产默认
  `listProcesses = readdir("/proc")` 未变。
- A 股语义无变化：仅测试基础设施，未改 engine、交易规则、单位、UI 文案、存档或 API 契约。

## 独立复审（Sol）

由未实施该改动的独立 Sol subagent 复审 `8c5b5b22` / `242321c3`（父 `7b9f60bc`）：**批准，两个原阻断项均关闭**。

- 出生过滤问题关闭：逐 PID 读 environ、EACCES/EPERM 显式失败、同 birth 双读、identity changed、Z/X 判断均保留；
  回归用例显式注入 `launcherBirth` 后能触发被否决实现的漏清理分支。
- 并发 fixture 问题关闭：真实 fixture 保留真实 environ/stat、原断言与 1000ms 预算，仅限定列举范围；
  独立 256 PID 用例覆盖逐项检查与并发上限，默认 `readdir("/proc")` 由 supervisor 集成用例覆盖。
- 分批语义可接受：未放宽或重置共享 deadline；前批失败后停止后续批次，仍显式失败。
- 复审独立验证（均 10 秒外部硬上限）：proc 单跑 14/14、4 路并发 × 3 轮 12/12、execution 3/3、lifecycle 10/10、
  long-validation-process 2/2；未跑全仓、`all`、Cargo、Clippy 或前端构建。

## 177eee27 单批失败：真实子进程 proto-exit 窗口（EACCES + stat=R）

### 现场事实（不做 PID 复用假设）

集成记录见
`.worktree/branch-convergence-final-20261102/agents/branch-convergence-final-validation/README.md` 末节，
原始日志 `d3-full-regression-177eee27.log`。

- 失败：`cannot confirm owned process 463293 (state=R, ppid=432775, pgid=432775, starttime=63282484): EACCES ... /proc/463293/environ`。
- 日志 `:24552` 证明 PID 463293 是本批次真实 spawn：token `432775:2661`，
  context `…session::company_source_runtime_tests::failed_tick_after_intraday_monthly_publication_rolls_back_library_and_information`，
  `spawned_ms=632824849`；`starttime=63282484`（100Hz tick）与之精确吻合，ppid 正是 launcher 432775。
  因此**不是 PID 复用**，也没有证据支持该假设。
- 时间线：2661 于 632824849 启动；2660 在 632827045 记录 `rust-case-termination-confirmed`（+2.2s）。
  2660 没有 `rust-case-complete`，而 `termination-confirmed` 只在 `fail()` 内写出；据此可证
  的顺序是：2660 的完成清理扫描先读到 463293 的 environ EACCES 并使 cleanup 失败，
  之后才触发 `fail()` 与 `stopDomain` 组终止。日志**不能**证明组 SIGKILL 制造了该退出窗口；
  更可能是 463293 当时正在自然退出（它自己的 case 已结束），但现场没有保留它的 statm，
  无法最终确认。

### 可复现根因（真实进程实验）

`/tmp/environ-race.mjs`（不改仓库）：SIGKILL 子进程后立即读 `/proc/<pid>/environ`，
150 次中 **75 次 EACCES 且 stat 仍为 R**（另 X 2、Z 55、gone 18）；未 kill 的 200 次 spawn 均无 EACCES。

`/tmp/environ-race2.mjs`：活着的 non-dumpable 子进程为 state=S、`statm=6073 2960 1743 793 0 1553 0`；
SIGKILL 后捕获的 6/6 样本为 state=R、`statm=0 0 0 0 0 0 0`、maps 长度 0。

机制：`exit_mm` 先释放地址空间，`exit_notify` 才把 task 置为 Z/X。该窗口内 environ 必然 EACCES、
stat 仍是 R，旧代码只把 Z/X 当作“无执行实体”，于是把正在退出的本批次子进程当成
“活 owned 不可确认”硬失败。该机制与批量现场“扫描时另一个 case 进程正在退出”的解释一致，
但不是由现场证据直接证明；本机空载窗口中位约 0.55ms（最大 2.6ms），高负载下可跨过两次 5ms 重试。

**边界（如实声明）**：批量现场没有留下该 PID 的 statm 证据，无法证明它当时处于 mm 已释放窗口；
可复现的是同一失败路径上真实存在的竞态，且现场 PID 出生时间与 spawn 精确吻合、排除了 PID 复用解释。

### 修复（单次快照不作为终态）

`scripts/rust-test-case-supervisor.mjs`：

- environ EACCES 且尚未确认 token（`candidate=false`）、同 birth 的进程已释放地址空间
  （`/proc/<pid>/statm` 全 0）时，**继续按同 birth 等待**，每轮重读 stat/environ/statm：
  - 直到 Z/X 或消失 → 正常结束；
  - birth 改变 → identity changed；
  - mm 恢复但 environ 仍 EACCES → 回到原有 fail-closed（owned EACCES 不吞）；
  - 共享 deadline 耗尽 → 原有 deadline 错误。
- 已匹配过 token 的 `candidate` 不进入该等待（保留既有 fail-closed 契约）。
- 活着的 non-dumpable 进程（statm 非 0）不受影响，仍显式失败。

测试（先红后绿）：

- 单测「environ EACCES 且地址空间已释放时必须等待同 birth 的终态」：注入 state=R→Z、statm 全 0；
  旧实现红（直接报 EACCES），新实现绿。
- 单测「退出窗口等待受共享 deadline 限制」：statm 恒 0、state 恒 R、60ms 预算 → /deadline/。
- 单测「地址空间仍存在的 owned non-dumpable 进程必须继续显式失败」：statm 非 0 → EACCES。
- 单测「地址空间从全 0 恢复且 environ 仍 EACCES」：statm 前两次全 0、随后非 0 → 必须显式失败且不得清理。
- 真实用例：捕获 `state=R + statm 全 0` 的 SIGKILL 子进程后，cleanup 必须最终正常结束。
  它补充证明真实窗口存在，但捕获后进程可能在 cleanup 前已变为 Z/消失，**不能单独证明等待分支被执行**；
  该分支由上面的确定性注入用例覆盖。

验证：proc 文件单跑 **10/10**（每次 19/19）、4 路并发 × 3 轮 **12/12**、execution 3/3、lifecycle 10/10、
run-with-deadline-cleanup 16/16、run-long-validation-process 2/2（均 10 秒外部硬上限）。
未运行全仓或正式 `all`。

### 限制

- 未能重放失败批次；结论基于“真实进程可复现竞态 + 现场 PID 与 spawn/birth 精确吻合”；
  未把 PID 复用当作事实，也未把单次 statm=0 当作终态。
- 若进程在共享期限耗尽前始终停留在该窗口，仍会按期限报错（如实失败），不静默放行。
