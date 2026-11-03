# ts-rs export_bindings case 在正式 all 中撞 10 秒上限的调查

基线：HEAD `607bf2f3d80af435b51c04492cdd3cc05d545640`。
失败点：`packages/engine rlib:engine case session::snapshot::export_bindings_snapshot failed: bounded command exhausted its execution budget within the total 10000ms deadline`（`agents/branch-convergence-final-validation/README.md` 的 607bf2f3 轮）。

## 现场事实（来自 g5 日志）

- case spawned 638498372.9ms，termination confirmed 638507685.8ms，进程存活 **9312.9ms**，撞 10000ms 总期限；不是断言失败或 panic。
- 该 case 是 ts-rs 自动生成的导出测试。`ts-rs` 12 的 `export.rs::export_and_merge` 对每个文件执行 `File::create → write_all → sync_all()`；本 case 实际写出 **12 个文件**（3 次隔离运行共 36 个文件，`strace` 计数 12 次 `fsync`）。
- 正式批次的 `TS_RS_EXPORT_DIR` 位于 `/data1/.../stock_market_game/.tmp/process-tmp/full-regression/rust-export-*/N`，该挂载为 **100% 已用的共享 bcache**。
- 同批 184 个完成的 export case：p50 588ms、p90 1679ms、p99 4619ms、max 5046ms；同批非 export case p90 981ms、p99 2516ms。

## 隔离测量（本机，每进程外层 `run-with-deadline.mjs 10000`）

| 导出目录 | 结果 |
|---|---|
| `/tmp`（tmpfs） | 3 连跑 0.02 / 0.03 / 0.02 s，wall 0.16–0.24 s |
| 主仓 `.tmp`（bcache） | 交错测量 0.06 / 0.06 / 1.73 s；另 3 连跑 1.16 / 2.42 / 3.05 s |
| 8 路轻并发（同 case，各独立目录） | tmpfs 0.13–0.20 s；bcache 0.31–0.32 s（未落在慢窗口），均 8/8 通过 |

所有隔离运行 user CPU 仅 0.12–0.17 s。`strace -c` 快窗口显示 12 次 fsync 合计 13ms；慢窗口 wall 2.4s 时全部 syscall 仍只有 16–31ms。墙钟缺口无法由 CPU 或 syscall 计时解释，落在内核写回/IO 停顿与调度饥饿上。

## 归因边界（重要）

**失败当时的逐次 fsync 耗时没有取证**（当时未运行 strace）。因此“共享满载 bcache 的 fsync 尖峰导致该次 9.3 秒”是**最强候选推测**，不是已证明的唯一根因；CPU 调度饥饿可能叠加。已证明的是：该 case 逻辑本身极快、每次固定 12 次 `sync_all`、导出目录在共享慢盘、换 tmpfs 后稳定在 0.02s。

## 修复

- `scripts/rust-test-case-supervisor.mjs`：新增 `exportRootParent()`，并让 `createExportRoot()` 复用它。Linux 使用 tmpfs `/dev/shm`，`/dev/shm` 不存在或不是目录时**显式抛错**，不静默回退到共享工作区磁盘；非 Linux 沿用调用方 `env.TMPDIR/TMP/TEMP`，空则 `os.tmpdir()`。`createExportRoot()` 仍为每批生成唯一 `rust-export-<pid>-<uuid>` 目录。
- 保持：每个 export case 独立 `TS_RS_EXPORT_DIR` 子目录、批次结束的独立清理进程、清理 1000ms 预留、10 秒每 case 与 300000ms 批次期限、失败显式上报；未改 ts-rs 依赖或生产类型生成路径（`types:generate` 仍写入仓库生成目录）。
- `docs/architecture.md`、`docs/testing.md`：登记 tmpfs export root 例外、清理与强杀残留责任。

## 验证

- 新增 4 个单测覆盖：Linux 使用 tmpfs、`/dev/shm` 不可用时显式失败、非 Linux 沿用调用方环境临时目录、每批 root 唯一且位于隔离根下。
- **行为红态**（不是 import 缺失）：取基线 `HEAD:scripts/rust-test-case-supervisor.mjs`，以 `createExportRoot({ TMPDIR: "/tmp/shared-workspace" })` 实测 parent = **`/tmp/shared-workspace`**（共享盘，违反隔离目标）；实现后同一调用 parent = **`/dev/shm`**。先写的新 API 单测最初报“导出不存在”属 import 级失败，不能作为 TDD 证据，已用上述基线行为红/绿补齐。
- 已更新既有 export 集成测试：两个 case 的导出目录必须位于**同一个批次 export root**（`dirname` 集合为 1）且互不相同、批次 root 与 case 子目录都在批次结束后删除、`TMPDIR` 下不得出现 `rust-export-*`；Linux 额外断言批次 root 不落在共享 TMPDIR。失败/超时/双失败清理用例也补了批次 root 删除断言。
- 逐文件进程外 10 秒 deadline 结果：`rust-test-case-supervisor` 6/6、`-export` 2/2、`-export-timeout` 1/1、`-export-stopped-launcher` 1/1、`-export-double-failure` 1/1、`-export-failure` 1/1、`-lifecycle` 10/10、`full-regression-execution` 28/28。
- 用正式密封 engine 二进制、真实 `session::snapshot::export_bindings_snapshot`，`TS_RS_EXPORT_DIR` 设在 `/dev/shm/rust-export-*`：3 连跑均 0.02s、wall 0.17–0.21s，写出 12 文件；批次目录已清理，无残留。
- 未启动 `all`。

## 独立审核

未参与实施的 Sol high 审核者只读审查完整 diff。

- 首轮：**需修改**。(1) 非 Linux 的 `createExportRoot(env)` 与测试期望不一致；(2) 集成测试未证明两个 case 共用同一批次 root、也未证明批次 root 删除；(3) 唯一红态是 import 缺失，不满足 TDD 的红灯要求。
- 已修复：统一 `exportRootParent({ env })` 实现与测试；补齐同批 root、root 删除断言（成功、双失败、launcher 停顿超时路径）；补基线 `createExportRoot()` 行为红/绿证据（共享 TMPDIR → `/tmp/shared-workspace`，当前 → `/dev/shm`）。
- 复审：**可合并，未发现其他必须修复项**。审核者未运行 `all`，未修改文件。

## 复审追加：lifecycle 故障注入 fixture 的 export root 泄漏

复审发现 `scripts/rust-test-case-supervisor-lifecycle.test.mjs` 的“导出目录清理失败必须使成功批次显式失败”用例只 `rm(dir)`，注入 `cleanupExportRoot` 抛错后，launcher 实际创建的 `/dev/shm/rust-export-*` 不会被删除。

现场身份确认（清理前）：`/dev/shm` 下 5 个空目录 `rust-export-1257572-*`、`-1266487-*`、`-1277202-*`、`-1282355-*`、`-1336116-*`，uid 1005（本用户），mtime 分别为 17:51 / 17:53 / 17:56 / 17:58 / 18:04，对应本轮反复运行该测试的时间；对应 pid 均已退出，无进程或打开句柄引用；该前缀只由本 worktree 的新实现产生。确认归属后以 `rmdir`（只删空目录）逐个清理，未删除任何其他路径。

修复：launcher 在 `initialize` 时**先**把 `m.exportRoot` 写入 fixture 目录内的 `export-root.txt`，**再**创建 tmpfs root；这样 receipt 写入失败时尚未创建 root，而 mkdir 或后续失败时 finally 已能拿到准确路径。测试仍保留原抛错断言，并新增“清理故障后批次 root 仍存在”断言；finally 读取实际 root，先断言其 parent 等于 `exportRootParent({ env: { ...process.env, TMPDIR: dir } })` 且 basename 为 `rust-export-*`，再删除 root 和 fixture 目录。读取/删除失败会直接抛出，不吞错。

验证：修复后 lifecycle 10/10 通过，运行前后 `/dev/shm/rust-export-*` 均为 0；相关 8 个测试文件逐文件进程外 10 秒 deadline 共 50/50 通过，结束后仍为 0 泄漏。
