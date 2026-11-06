# 旧 approved 工作树清理（2026-10-29）

## 范围与保护

续接用户清理已合并、废弃分支的授权，本批仅处理下表固定 10 个旧 approved 工作树及同名 integration branch。根 checkout、main、活任务、共享 runner 和 `fix/synthetic-history-policy` 保留；不 force、不 -D、不改配置、不 prune、不远端操作。普通命令拒绝即逐项保留。

本批不改变产品代码、交易语义或正式文档。所有原 commit 均由 main 可达，保存下表完整 OID 即可从 Git 追溯；不复制历史 raw。无产品改动，不启动 Cargo 或产品回归。

## 删除前核对

- main：`21208e22d485d1152549d712f47dabd52d89f781`，对应 `.worktree/main-merge-2026-10-25` clean。
- 根：`af0fdeb6c952f294e3069f031c38f4d8ad0fd6b4`（`fix/multi-group-roster-offset`）；原 4 项 tracked 修改为 ROLE.md、agents/coordinator-status-2026-10-22.md、agents/engine-max-performance/string-free-inventory.md、memory/MEMORY.md；index 空。原 tracked diff SHA256：`6a3304fc3e3190d2495c389fca1ce446d7cd57f9c4c80f44e56f0901993de3b8`。
- 执行者独立二核真实 HEAD、branch ref、main ancestor；10/10 完整 `git status --porcelain=v1 --untracked-files=all --ignored=matching` 均空。递归遍历无嵌套 .git，无遍历错误；10 个 branch 无 branch 配置。
- 可读取的 /proc 范围无候选 cwd / cmdline 命中；2093 个 cwd PermissionError，cmdline 无权限错误。本证据不代表全机完备扫描，当前 owner 闭包确认补足不可读部分。
- integration owner 直接确认：当前 runner/control 消费 d3-p1 本树、main-merge helper、runtime-sync-e1 guard、funds-production-resume collector、wasm 小模块及 Host joint-native-and-export-02 handoff，均不消费这 10 树。历史 asof 提及不作为活动依赖。
- root 内部放行固定选集：main-merge 空闲但保护；d3-p1、company-write-impl、r4-tax-prod、calendar-final-fixes 及 v11 测试树均保留，当前 owner 不使用这 10 树。

## 固定选集与实际结果

表中工作树均位于仓库 `.worktree/`。每项执行前再次核对 OID、branch 与完整 status；从 clean main 工作树执行普通 `git worktree remove <path>`，成功后执行 `git branch -d -- <branch>`。

| 工作树名称 | branch | 原 OID | 结果 |
|---|---|---|---|
| `116-approved-2026-10-25` | `integration/116-approved-2026-10-25` | `f046075972bb3da42aa1fb4891bee83efd863cb7` | remove 与 branch -d 均 exit 0，已删除 |
| `admin-d1a-approved-2026-10-26` | `integration/admin-d1a-approved-2026-10-26` | `3b535043860117a4e82122c3a8cc38564307b94b` | remove 与 branch -d 均 exit 0，已删除 |
| `approved-save-2026-10-25` | `integration/approved-save-2026-10-25` | `f58202911b8db6c042fe1816d08277c16381f2d2` | remove 与 branch -d 均 exit 0，已删除 |
| `cargo-budget-approved-2026-10-26` | `integration/cargo-budget-approved-2026-10-26` | `d7a26186d40ec38ffce901f48a935917b5d29641` | remove 与 branch -d 均 exit 0，已删除 |
| `company-capability-approved-2026-10-25` | `integration/company-capability-approved-2026-10-25` | `b0dce789e12984ae769bcde2029433b557d6979c` | remove 与 branch -d 均 exit 0，已删除 |
| `company-catalog-a-approved-2026-10-26` | `integration/company-catalog-a-approved-2026-10-26` | `2886006e45ebd66546e49ac138ed5fc7c90bea79` | remove 与 branch -d 均 exit 0，已删除 |
| `company-pure-inputs-approved-2026-10-26` | `integration/company-pure-inputs-approved-2026-10-26` | `18c7620e6ae92be9e7fb3d1909c6de7513d18eca` | remove 与 branch -d 均 exit 0，已删除 |
| `company-read-views-approved-2026-10-25` | `integration/company-read-views-approved-2026-10-25` | `791858d468113d9ac8cdf9794a237b171cd41673` | remove 与 branch -d 均 exit 0，已删除 |
| `dayend-d0-approved-2026-10-26` | `integration/dayend-d0-approved-2026-10-26` | `9065ac1a72b74e2eba6c17d362df243c381b746f` | remove 与 branch -d 均 exit 0，已删除 |
| `diagnostics-feature-approved-2026-10-25` | `integration/diagnostics-feature-approved-2026-10-25` | `cde4755c6cde28b73725ace25418fe6c4d9955cb` | remove 与 branch -d 均 exit 0，已删除 |

## 执行后核对

10 个普通 worktree remove 与 10 个普通 branch -d 全部 exit 0，无失败、无升级命令。执行者实际 poststate 检查：

- 10 个精确 branch ref 均不存在；10 个目录与 worktree 登记均不存在；10 个原 OID 仍全部是 main 祖先。
- 完整 worktree 登记精确等于执行前移除这 10 个段落后的预期值，其他所有工作树登记不变。
- 根与 main HEAD 均未变，main 仍 clean；root index 仍为空，原 4 项 tracked diff SHA256 不变。
- 首次执行前保护断言发现 root 刚新增 agents/coordination/current-work.md 的协调入口修改，未执行任何删除即停止。root 确认其所有权后刷新已知保护基线，实际执行前后完整 tracked diff SHA256 均为 `49bc4806317e8d4c203f37e5ede44b1ade0a5c4045dee8c726a6386f20959e01`。根现有 5 项 tracked dirty 均保留。
- root 指示不另派复核票，将亲自独立检查真实 ref、worktree 和既有 diff。本记录只记执行者实际检查，不冒称 root 已通过。

本批只新增此中文工作记录，未暂存或提交根目录既有修改。按 root 只读根工作区边界，不在根创建提交；工作记录的本地保存由 root 协调。


## root 独立实际复核

root于本批执行后逐项核对10个精确路径及ref均不存在；根HEAD仍为af0fdeb6，本地main仍为21208e22，index为空，原四项tracked diff SHA256仍为6a3304fc3e3190d2495c389fca1ce446d7cd57f9c4c80f44e56f0901993de3b8。此小文本获用户证据归档授权，上传仅限独立evidence分支，不推送本地产品提交。
