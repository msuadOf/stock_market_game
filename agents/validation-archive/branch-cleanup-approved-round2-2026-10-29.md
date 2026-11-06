# 旧 approved 工作树第二批清理（2026-10-29）

## 固定范围与保护

续接用户清理已合并、废弃分支授权，只处理 root 指定的本批固定名称，不扩大范围。main、根、活任务、共享 runner、`fix/synthetic-history-policy` 保留；只用普通 `git worktree remove` 与 `git branch -d`，拒绝即保留，不 force、不改配置、不 prune、不远端操作。

本批无产品代码或交易语义变更，无需产品回归。原 commit 保存在 Git，以下完整 OID 可供追溯。根不暂存、不提交；本批只写中文小工作记录。

## 删除前可信核对

- main：`a560723a1a1cd7a1c76a413b65375fb10a675952`，main-merge 工作树 clean；根 HEAD `af0fdeb6c952f294e3069f031c38f4d8ad0fd6b4`。
- 根原四项 tracked 修改（ROLE.md、coordinator-status、string-free-inventory、memory/MEMORY.md）的 diff SHA256：`6a3304fc3e3190d2495c389fca1ce446d7cd57f9c4c80f44e56f0901993de3b8`；index 空。root 协调入口与 untracked 工作记录允许并发写，不以其改变扩展审批。
- 9 个存在候选的真实 HEAD 与 branch ref 一致，均为 main ancestor；完整 status 含 all untracked / ignored 均空，无嵌套 .git、无遍历错误、无 branch 配置，也没有其他本地 branch 以候选作 local upstream。
- 指定 `exempt-setup-approved-2026-10-26` 目录、登记和 ref 不存在，未对该名称执行删除。root 随后明确修正固定选集为 `exempt-setup-approved-2026-10-25` / `integration/exempt-setup-approved-2026-10-25`；执行者按相同标准窄核完整 OID `ca16014731b142e401b7e79534585f39188b0bd9`，clean、ancestor、ref、无嵌套 repo、无 local upstream 消费者、无 branch 配置、可读 /proc 无 cwd 命中（该次 2934 个 cwd PermissionError），通过后才纳入。
- 可读 /proc 无候选 cwd / cmdline 命中；2655 个 cwd PermissionError，cmdline 无权限错误，不声称全机完备扫描。
- root 已给当前 owner 闭包：d3-p1、Host impl、Tax prod、calendar-final-fixes、wasm-production-stop、codex outer/runtime override 新树继续保留；当前 control 仅消费 main helper、runtime-sync-e1、funds collectors、wasm 小模块，不消费本批旧 approved 树。历史 asof 引用不当作消费者。

## 固定选集与结果

下表工作树位于 `.worktree/`，对应 branch 均为 `integration/<工作树名称>`。

| 工作树名称 | 原 OID | 结果 |
|---|---|---|
| `distributions-r2-approved-2026-10-26` | `b4a3f35e910c8901d56ed04cf6a69807e8565168` | remove 与 branch -d 均 exit 0，已删除 |
| `exempt-setup-approved-2026-10-26` | 不适用 | 原指定不存在，未执行删除 |
| `exempt-setup-approved-2026-10-25` | `ca16014731b142e401b7e79534585f39188b0bd9` | remove 与 branch -d 均 exit 0，已删除 |
| `issue120-approved-2026-10-26` | `1afece4c88fca7688305a7bfb713d9f107d4b2ba` | remove 与 branch -d 均 exit 0，已删除 |
| `legacy-approved-2026-10-25` | `b172c1ab1a69daa44dc57dddc887db255d7f6ce7` | remove 与 branch -d 均 exit 0，已删除 |
| `mixed-c1a-approved-2026-10-26` | `54ceb70928936ae94b482c985b89740bc317e6aa` | remove 与 branch -d 均 exit 0，已删除 |
| `mixed-c2a-approved-2026-10-26` | `94ecbffa74b4f62efe0eddd8940f913290bdb121` | remove 与 branch -d 均 exit 0，已删除 |
| `mixed-calendar-c0-approved-2026-10-26` | `b02e4f550c931b6e118fbb43c579af8228b20ee5` | remove 与 branch -d 均 exit 0，已删除 |
| `plan-identity-approved-2026-10-26` | `bba4721d29599094db27db64ab4e9650c06aeb03` | remove 与 branch -d 均 exit 0，已删除 |
| `r3a-fixture-fixed-approved-2026-10-26` | `b94b3b1026a70c22a9c5ac463f34bfdfd4afaa3c` | remove 与 branch -d 均 exit 0，已删除 |
| `registry-dayend-r1-approved-2026-10-26` | `92e57e384f1aeb77e69f02cdf425444b9959725c` | remove 与 branch -d 均 exit 0，已删除 |

## 执行与 poststate

root 内部放行现有 9 项并明确修正第 10 项后，实际执行 10 次普通 worktree remove 和 10 次普通 branch -d，全部 exit 0，无拒绝、无强制命令。删除命令按 Git 共享登记顺序执行，期间有正常文件系统 I/O 等待。

执行者实际 poststate 核对全部通过：

- 实际选集 10 个目录、ref 与登记均不存在；10 个原 OID 仍由 main 可达。
- 完整 worktree 登记精确等于执行前移除固定 10 段后的预期值，其他工作树登记未变。
- main 执行前后均为 `a560723a1a1cd7a1c76a413b65375fb10a675952`，保持 clean；root HEAD 未变。
- 根原四项 tracked diff SHA256 仍为 `6a3304fc3e3190d2495c389fca1ce446d7cd57f9c4c80f44e56f0901993de3b8`，index 仍空。未触碰协调记录的并发写入。

未暂存或提交根工作区，独立实际 poststate 交 root 复核；本段不冒称 root 已复核通过。



## root实际复核

root已逐项核对本批十目录及对应精确integration ref不存在；原四项tracked diff SHA256仍为6a3304fc3e3190d2495c389fca1ce446d7cd57f9c4c80f44e56f0901993de3b8，index为空。用户授权仅本地清理及独立小文本证据上传，产品未push。
