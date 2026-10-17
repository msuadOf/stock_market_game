# 性能 binary 历史构建绑定补证

生成日期：2026-10-18（Asia/Shanghai）。范围仅为历史构建来源核验，未重新测量性能，未修改、暂存或提交任何既有 tracked 文件。

## 三口径结果

| 口径 | 历史报告值 | binary | 账本原始纯 HEAD | 补丁树 `6dcc25b4` |
|---|---|---|---|---|
| A | 1409.2 t/s | `runtime_s5_probe` | `1f5863cd` 构建失败，退出码 101，无 binary SHA | 重建 SHA==账本 SHA |
| B300 | 302.7 t/s | `driver_semantics_probe` | 同上 | 重建 SHA==账本 SHA |
| B1000 | 501.5 t/s | `driver_semantics_probe` | 同上 | 重建 SHA==账本 SHA |

三口径只有两个不同的 binary，账本标注的提交点相同。因此对两个纯提交树各执行一次同时包含两个 example 的构建；B300/B1000 共享同一个 driver binary。每口径的 `rebuild-log.txt` 均保留两次构建的完整原文，不伪造三次独立构建。

账本 SHA256：

```text
6123db29b892f2e42632eae557de6e6ecfed95ead4cc60842ce29bef8848745a  driver_semantics_probe
ced474fac39be2a1c613b921553b8db481cc0b40997255798ed1b398154ee92d  runtime_s5_probe
```

## 源码树 → 构建 → SHA

1. 账本源文件是 `agents/std-thread-migration/evidence/perf-p2/perf-phase1-final.txt`（任务中的路径为该目录下的相对路径），原文副本见 `ledger.txt`。记录的 HEAD 为 `1f5863cda997e01fce1b57c0149e7bff5ac1e302`。
2. `.worktree/perf-bind-1f5863cd/` 是上述提交的 detached worktree。构建前后 `git status --porcelain=v1 --untracked-files=all` 均为空。实际构建出现三个 E0433（缺少 `RestingDeltaEntry`）及一个 E0425（缺少 `RestingFact`），退出码 101。原始纯 HEAD 不可编译的原因获得实证；不能声称账本 binary 来自该纯 HEAD。
3. `.worktree/perf-bind-6dcc25b4/` 是 `6dcc25b4174a112fca053a933affe5d1ec05a47a` 的 detached worktree，构建前后状态也为空。完整差异见 `source-difference.patch`：相对 `1f5863cd`，编译输入仅增加 `packages/engine/src/runtime/mod.rs` 的一行 `pub use protocol::{RestingDelta, RestingDeltaEntry, RestingFact};`；另外两项差异是 MANIFEST 与终测账本文档。不能把完整提交树差异描述成仅一行。
4. 在补丁树使用自身全新的 `target/` 完整重建，退出码 0；两个 binary 均与账本 SHA 逐位一致。`A/`、`B300/`、`B1000/` 的 `sha-compare.txt` 保留实际 `sha256sum` 及 `sha256sum --check` 输出，同时核对 target 原件及 `6dcc25b4/binaries/` 保留副本。
5. 两个提交分别保留 `source.tar`（完整 tracked 源码树、不含 `.git`/`target`）、`source-files.txt`（Git blob 清单）、`commit.txt`、`tree.txt`、源码归档 SHA 与校验原文。Git archive 按版本化 `.gitattributes` 应用 export 属性；两个 detached worktree 与源码归档均保留。独立 bare Git 元数据通过 alternates 只读引用原仓库 objects；即使该引用以后不可用，源码归档与构建日志仍独立保留。

可复核的结论是：**`1f5863cd` 原始纯 HEAD 构建失败；补齐历史 re-export 的纯提交树 `6dcc25b4` 在本次环境重建 SHA==账本 SHA。** 这建立了账本 SHA 与可恢复编译源码树的绑定，不把本次重建伪称历史工作树的原始快照，也不证明缺失的历史每轮 stdout。

## 构建命令、参数与环境

账本只写了运行命令，没有写 build 命令或 `CARGO_PROFILE_RELEASE_DEBUG`。既有 `binary-provenance-closure.md`（副本 `prior-closure.md`）写的是默认 release 构建。本次按这一可查记录构建，并显式增加离线/lockfile 固定、并发及完整命令日志参数：

```sh
cargo build --offline --locked --release --example driver_semantics_probe --example runtime_s5_probe -p engine -j 16 -vv
```

未设置 `CARGO_PROFILE_RELEASE_DEBUG`、`RUSTFLAGS` 或其他 profile override。不能声称“照抄了账本中不存在的构建参数”。P2-4a 文档中 `CARGO_PROFILE_RELEASE_DEBUG=1` 与 `RUSTFLAGS=-C force-frame-pointers=yes` 明确用于专用画像 binary，不作为终测 binary 的构建参数。旁证 `existing-binary-fingerprints.txt` 对应既存同 SHA binary 的实际 inode，显示 features=[]、rustflags=[]；本次独立重建 SHA 一致是主要证据。

- `rustc 1.96.1 (31fca3adb 2026-06-26)`，LLVM 22.1.2；`cargo 1.96.1`，host `x86_64-unknown-linux-gnu`；完整输出见 `host-toolchain.txt`。
- 源码版本化 `[profile.release]`：`lto="thin"`、`codegen-units=16`；lockfile 与配置原文见 `source-configs.txt`，完整源码归档含同一配置。
- 每个 Cargo 构建显式 `-j 16`，两提交同时构建，总上限 32 Cargo jobs；实际 rustc/native 构建命令与 `NUM_JOBS=16` 均在 `-vv` 日志中。未使用其他人的 target，未运行测试或性能探针。
- 时间与退出码位于各构建日志首尾；完整补丁树 release 构建约 2m53s。编译原有 warnings 原样保留。

## Worktree 元数据隔离

首次在原仓库执行 `git worktree add --detach .worktree/perf-bind-1f5863cd 1f5863cd` 因共享 `.git/worktrees` 只读而失败，原文见 `worktree-1f5863cd.txt`。没有请求提权或改动原仓库 Git 元数据。

随后以 `git clone --bare --shared /data1/baiyifan/workplace/stock_market_game/.git .tmp/perf-binding/source-repository.git` 创建本任务独立 Git 元数据，再通过 `git --git-dir=.tmp/perf-binding/source-repository.git worktree add --detach ...` 创建上述两个 worktree。创建日志见 `repository-clone.txt`、`worktree-1f5863cd-isolated.txt`、`worktree-6dcc25b4.txt`。原分支没有 checkout、reset、add、commit 或 push 操作。

## 产物索引与范围

- `A/`、`B300/`、`B1000/`：各自的 `rebuild-log.txt`、`sha-compare.txt`、`expected.sha256`。
- `1f5863cd/`、`6dcc25b4/`：纯源码归档、Git 身份与状态、原始构建日志、结果 JSON；后者另含两个 binary 副本。
- `initial-status.txt`、`initial-tracked.diff`：开始时已存在的 `SaveSlot.ts` 修改快照，仅作只读范围核验。
- `rebuild.py`、`publish-evidence.py`：实际执行的补证脚本；仅创建本任务新文件。
- 最终 tracked 状态核验、独立复核记录及摘要校验表在结束前另行生成于本目录。

本轮未改动交易实现、类型、存档、运行参数或 A 股规则。A 与 B 负载不同；B 报告值测的是历史 `GameSession::step()`，不能提升为生产 `step_frame()` 性能。B300 全处竞价申报窗、fees=0 的原始账本语义保持不变。性能数字继续作为历史测量报告值，本次仅补构建绑定。
