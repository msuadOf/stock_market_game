# 旧 approved 工作树第三批清理（2026-10-29）

## 范围与保护

续接用户及时清理已合并、废弃分支的授权，本批固定下表 10 项；不重做前两批 20 树及另 3 个 Web 临时候选。root 已逐项独立核对 OID / ref / 完整 clean / main ancestor，并授权在 integration 当前 runner 无消费者确认后执行。

只用普通 `git worktree remove` 和 `git branch -d`，拒绝即保留；不 force、不改配置、不 prune、不远端操作。根、main、所有 active 工作树、共享 helper 及 synthetic upstream 保留支均保护。无产品与 A 股语义改动，不运行 Cargo 或产品回归；原 commit 完整 OID 留表供 Git 追溯。

## 删除前核对与当前消费者

- main 核对值：`bc8a2addb547c188f965faa57123fd59922dc0c1`，main-merge 工作树 clean；根 `af0fdeb6c952f294e3069f031c38f4d8ad0fd6b4`。
- 原四项 tracked diff SHA256：`6a3304fc3e3190d2495c389fca1ce446d7cd57f9c4c80f44e56f0901993de3b8`；index 空。新增协调入口及 untracked 工作记录允许并发变化；根 hash.rs 的额外 test trace dirty 也保留，不触碰。
- 10/10 真实 HEAD 与 branch ref 一致、main ancestor、完整 status 含 all untracked / ignored 为空；无嵌套 .git、遍历错误、branch 配置或 local upstream 消费者。
- 可读 /proc 无候选 cwd；1655 个 cwd PermissionError，不声称全机完备扫描。
- integration owner 实际窄核 green50 plan.runnerSources 的 20 个直接 control 文件及路径，所列 10 名均无命中；后继 50r2 仅同源 owner02 机械差量，同样不消费。tax-k2a 旧树不是当前 source/control 输入。当前仅消费 integration 本人实际源码、main-merge helper、runtime-sync-e1 guard、funds collector 等直接输入。
- 执行者另窄核 green50 当前目录 28 个不大于 500KB 的 py / mjs / JSON control、plan、manifest 小文件，10 名均无命中；未扫描历史 raw 或大制品。
- 排除 deadline-v3（ignored .tmp）、tax-k1 与 tax-k2ab（ignored bindings），不清其内容。

## 固定选集与结果

所有目录均位于仓库 `.worktree/`。

| 工作树名称 | branch | 原 OID | 结果 |
|---|---|---|---|
| `monthly-interim-approved-2026-10-27` | `integrate/monthly-interim-approved-2026-10-27` | `a0af8b19b38edbaf49fe1670b76065aa80b6e33a` | remove 与 branch -d 均 exit 0，已删除 |
| `rights-r3b-approved-2026-10-26` | `integration/rights-r3b-approved-2026-10-26` | `12d40ac04e76ce5103f6212be616d71909a97da8` | remove 与 branch -d 均 exit 0，已删除 |
| `share-split-zero-approved-2026-10-26` | `integration/share-split-zero-approved-2026-10-26` | `a568584ffbdc2abbe971823c19647323d9e3f7dc` | remove 与 branch -d 均 exit 0，已删除 |
| `share-transform-r3a-approved-2026-10-26` | `integration/share-transform-r3a-approved-2026-10-26` | `d045f0dca231a2050b53e848be2986302023f52e` | remove 与 branch -d 均 exit 0，已删除 |
| `simple-preferences-approved-2026-10-25` | `integration/simple-preferences-approved-2026-10-25` | `83d958940515388d92ce0cdaeb1a62df9fb57c21` | remove 与 branch -d 均 exit 0，已删除 |
| `tax-k2a-approved-2026-10-27` | `integrate/tax-k2a-approved-2026-10-27` | `dcdca08bfd79a867a3de21c06546fadff64a8b64` | remove 与 branch -d 均 exit 0，已删除 |
| `wasm-license-approved-2026-10-25` | `integration/wasm-license-approved-2026-10-25` | `fff1bc2911ea65a88acefe198479351fbaf223f4` | remove 与 branch -d 均 exit 0，已删除 |
| `wasm-w0-approved-2026-10-26` | `integration/wasm-w0-approved-2026-10-26` | `c0138972c687a197164fca81d56a9bdc116b4517` | remove 与 branch -d 均 exit 0，已删除 |
| `wasm-w1-env-approved-2026-10-26` | `integration/wasm-w1-env-approved-2026-10-26` | `c6b99d164161e922405d483ca6b65f4472a87fdb` | remove 与 branch -d 均 exit 0，已删除 |
| `worker-reporter-approved-2026-10-26` | `integration/worker-reporter-approved-2026-10-26` | `a29d9be40a7bf59f5c8bdb9563ed70e8e3bed143` | remove 与 branch -d 均 exit 0，已删除 |

## 实际 poststate

integration 无消费者确认后，按 root 内部放行选集实际执行 10 次普通 worktree remove 与 10 次普通 branch -d，全部 exit 0；无拒绝、无强制或配置操作。执行前逐项再核真实 HEAD/ref、main ancestor、完整 status，全部符合预期。

执行者真实 poststate：

- 10 个目录、精确 ref 与 worktree 登记均不存在；10 个原 OID 仍全部由 main 可达。
- 完整 worktree 登记精确等于执行前仅移除固定 10 段后的预期值，其他工作树登记不变。
- main 执行前后均为 `bc8a2addb547c188f965faa57123fd59922dc0c1`，保持 clean；root HEAD 仍 `af0fdeb6c952f294e3069f031c38f4d8ad0fd6b4`。
- 原四项 tracked diff SHA256 仍为 `6a3304fc3e3190d2495c389fca1ce446d7cd57f9c4c80f44e56f0901993de3b8`，index 仍空。
- 根 `packages/engine/src/session/hash.rs` 既有 test trace diff SHA256 执行前后均为 `38782ba9136b42998c75a579f522892d9414915d35e72afa472d752e117e52d2`；root 协调记录的并发变化不阻断也不改写。

本批只新增此小 Markdown，未暂存或提交、未上传远端。交 root 独立核实际 poststate，本段不冒称其已通过。


root续核：固定10目录/ref/登记均不存在，表中原10OID均为main祖先；main clean，根原四dirty与额外hash.rs差量SHA保持、index空。证据摘要上传使用独立evidence分支，不包含产品代码。
