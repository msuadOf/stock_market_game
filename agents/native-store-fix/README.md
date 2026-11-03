# native-store archives 物理重开 case 10 秒超时调查

基线：独立 worktree `.worktree/branch-convergence-final-20261102`，HEAD `c32263a12dd343cba96de8c6c13a832a7e48df72`。
目标 case：`packages/native-store test:archives case explicit_slot_selection_physically_reopens_without_changing_economic_payloads`。
正式证据：`agents/branch-convergence-final-validation/README.md` f4（82.691 s）与 f7（74.554 s）两次单批 `all` 均因该 case 撞 10 秒每 case 上限失败；隔离 2.38 / 3.49 / 3.31 s。

## 测量记录

机器状态：128 核，load≈50；`/data1` 为 bcache，100% 已用；主机存在其他用户的高 CPU / 高 IO 任务（isla-runner 等）。

| 轮次 | 命令 | 结果 | CPU | 说明 |
|---|---|---|---|---|
| iso-r1 | 直接运行 `archives-c1946bc236dcd2fc --exact <case> --test-threads=1` | wall **7.51 s** | user 1.49 s / sys 0.08 s | 未用进程外 deadline |
| iso-r2 | 同上 | wall **2.67 s** | user 1.54 s | 未用进程外 deadline |
| iso-r3 | 同上 | wall **17.63 s** | user 1.61 s | **违反 10 秒普通测试硬限；已记录，不作为通过证据，之后所有运行改为进程外 10 秒 deadline** |
| strace-r1 | `strace -f -c` 同 case | wall 4.43 s | user 1.55 s / sys 0.08 s | 20 次 `fsync` 合计 0.0108 s，全部 syscall 0.017 s；本轮 fsync 不是瓶颈 |
| time-r1 | `/usr/bin/time -v` 同 case | wall 3.88 s | user 1.70 s / sys 0.11 s | involuntary context switches 687；RSS 56.9 MB |
| time-r2 | 同二进制纯内存 case `slot_management_is_transactional_and_does_not_reload_the_game` | wall 1.21 s | user 1.18 s / sys 0.03 s | 对照：fixture + 内存 SQLite 本身约 1.2 s user |
| stress-r1 | 32 个同 case 并发（仅测量，进程外未加 deadline） | 3.07–4.13 s / 批 4.18 s | — | 全部通过；但并发同一重 fixture 过度争用资源，按指示不再使用该压力方式 |

初步归因：wall 与 CPU 的差额主要来自 CPU 调度饥饿（687 次 involuntary context switch），本轮 fsync 延迟很小；需要继续分阶段定位 case 内部 CPU（fixture、capture、save、copy、各次 load/restore、serde 比较）占比。

## 约束

- 普通测试每条命令、每个 case 均以 10000ms 为硬上限，后续所有运行（含压力）使用进程外 deadline 监督。
- 10 秒期限不扩大；断言不弱化；保留真实物理 close→reopen 语义与完整经济 payload 比较。
- 完成后由未参与实施的 subagent 独立审核完整 diff；不跑 `all`。

## 分阶段定位（私有 probe，进程外 10 秒 deadline）

probe 在 `.tmp/native-store-fix/`（未进入仓库源码），完整复刻候选、save/copy/select、两次 load 比较、物理 reopen、delete、selection 与 current 检查。

仓库 `.tmp`（bcache，100% 已用）上的红态：

| 轮次 | 结果 | 关键阶段 |
|---|---|---|
| probe-r1 | **10 秒 deadline 耗尽** | candidate 296ms，open **5445ms**，随后被终止 |
| probe-r2 | **10 秒 deadline 耗尽** | open **2636ms**，save **4736ms**，随后被终止 |
| probe-r3 | **10 秒 deadline 耗尽** | open 2775ms、save 3066ms、copy 1506ms、select 1397ms |

同一 probe 改到 `/dev/shm` 后，3 轮均通过（外层 wall 1.77 / 2.02 / 1.75 s）：open 1.6–3.7ms、save 177–225ms、copy 243–303ms、select 0.2–0.4ms、三次 load 各 237–287ms。case 内 CPU 主要来自 3 次 `load`（每次含 `ProtocolSession::restore`）、fixture/candidate 约 300ms 与 copy 的 decode。

直接文件系统复测（同机同目录，1MB 写入 + `fsync`）：

| 目标 | 单次结果 |
|---|---|
| 仓库 `.tmp`（bcache） | 4KB：559 / 1624 / 858 / 793 / 1339 / 1009 ms；1MB：1180 / 1323 / 798 / 590 / 1517 / 402 ms |
| `/dev/shm`（tmpfs） | 约 1ms |

结论：该 case 约 20 次 `fsync`，共享满载 bcache 的**单次 fsync 0.2–1.6s** 是超时主因；早先一次 strace 的 20 次 fsync 合计 10ms 只是短暂快窗口，不能作为常态。

## 修复

`packages/native-store/tests/archives.rs`：

1. 新增 `physical_database_directory()`：Linux 上使用 tmpfs `/dev/shm/stock-market-game-native-store-tests`；`/dev/shm` 不存在或不是目录时**显式断言失败**，不静默回退到共享磁盘。目录创建或数据库写入失败（包括空间不足）直接由 `unwrap`/`StoreError` 暴露。非 Linux 平台使用系统临时目录。
2. 四个真实文件用例统一使用该目录：`real_archive_reopens_and_restores_complete_day_end`、`existing_unrecognized_schema_is_not_migrated`、`explicit_slot_selection_physically_reopens_without_changing_economic_payloads`、`independent_physical_open_and_inode_alias_cannot_create_second_writer`。它们都依赖真实文件、close→reopen、selection、文件锁或 inode 别名语义；tmpfs 均支持，且都不验证掉电持久性。将来若新增验证 `fsync`/掉电/崩溃恢复的用例，不得迁移到 tmpfs。
3. 目标用例中 `serde_json::to_value(&slot)` 由循环内两次提为一次 `expected`；断言语义不变，仅消除了同值重复序列化。

未改动：10 秒每 case 期限、正式并发策略、生产代码、任何断言内容、测试结束时的 DB 清理（`remove_file`）。

## 修复后验证（全部进程外 `run-with-deadline.mjs 10000`）

| 轮次 | 结果 |
|---|---|
| 目标 case 连跑 1–3 | 通过，case 1.91 / 1.53 / 1.53 s，外层 wall 2.08 / 1.67 / 1.68 s |
| 目标 case 4 路并发 | 4/4 通过，case 1.47–1.53 s，外层 wall 1.60–1.74 s |
| `archives` 全部 17 个普通 case（4 路并发逐 case 监督） | 17/17 通过，最慢 `deleting_active_slot...` 1.77 s；`exclusive_file_lock_child` 为 ignored helper，按预期 0 跑 |

修复后 `/dev/shm/stock-market-game-native-store-tests` 无遗留文件。测试使用真实 SQLite 文件与真实 close→reopen；本组断言所依赖的 reopen、文件锁与 inode 语义在 tmpfs 上保持，但 tmpfs 不证明稳定介质上的掉电耐久性，本组用例也未断言该性质。进程被强杀时 tmpfs 下可能残留数据库文件，清理责任已写入 `docs/architecture.md`。

## 独立审核

未参与实施的 Sol high 审核者只读审查了完整 diff（含 `docs/architecture.md` 与 `agents/native-store-fix/`）。

- 首轮结论：**需修改**。tmpfs 方向可接受，四处一起迁移合理，生产代码、`journal_mode=delete`、`synchronous=FULL`、close→reopen、inode/跨进程锁均未弱化；但 `docs/architecture.md` 的 `.tmp/` 全量约定与 helper 冲突，工作记录两处措辞过强。
- 已修复：`docs/architecture.md` 登记窄例外（仅 native-store 文件行为测试、不验证掉电耐久性、明确成功与强杀残留清理责任）；工作记录改为“断言语义不变，仅消除同值重复序列化”“不证明稳定介质掉电耐久性”。
- 第二轮结论：**可合并，未发现其他必须修复项**。审核者未运行测试或 `all`，未修改文件。

补充验证：`cargo fmt --all --check` 通过；`cargo clippy -p native-store --tests` 仅报告 engine 基线已有的 4 条 dead-code 警告（`-D warnings` 下才会失败），本次改动文件无新警告。
