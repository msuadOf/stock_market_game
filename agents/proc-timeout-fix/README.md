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
   防止“按出生过滤”的回归（撤销前该用例会红）。
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
