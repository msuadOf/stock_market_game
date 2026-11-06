# 已合并本地分支清理（2026-10-29）

## 范围与保护

用户已授权及时清理已经合并、废弃的分支。本批只删除下表 20 个已合并本地 branch；其中 2 个 clean worktree 只在冻结 runner 依赖核对及独立审查通过后移除。禁止 `--force`、`-D`、远端操作、未知 ref prune、修改历史或删除提交。已有工作区与 index 不暂存、不改写。

本批不改变 engine / web / server / desktop 产品文件和 A 股规则，故不涉及交易制度依据变更；合并祖先证明保证产品 commit 仍由 main 可达。无代码变更，不运行 Cargo 或产品测试，以 Git 可达性、完整 status、依赖核对与删除后身份检查验证。

main 与根 checkout、活跃任务目录、runtime-sync-e1 / info-runtime-staging 等共享闭包及仓库外应用管理工作树均保留。任何有 tracked 修改、未跟踪文件或 ignored 文件的候选均不移除。初筛 runtime-personal-full-validation、wasm-p0、api-contract-company-id、day-end-public-views、dayend-partial-start-fix、deadline-hardgate-fix、desktop-ipc-contract-fix、groups2-purse、holiday-gate 有 WIP，全部保留。

## 删除前基线

- main：`11cb97fa336c486b5e30526afce17b3e87e51533`；对应 `.worktree/main-merge-2026-10-25` status：`clean`。
- 根 HEAD：`af0fdeb6c952f294e3069f031c38f4d8ad0fd6b4`（`fix/multi-group-roster-offset`）。
- 根 tracked status（逐字保存；既有内容不动）：

```text
 M ROLE.md
 M agents/coordinator-status-2026-10-22.md
 M agents/engine-max-performance/string-free-inventory.md
 M memory/MEMORY.md
```

- `df -B1 .`：Used 3972092510208 bytes；Available 18183471104 bytes。共享磁盘存在其它任务写入，前后差值不认作本任务独占释放量。
- 两个候选 worktree 的 `git status --porcelain=v1 --untracked-files=all` 与追加 `--ignored=matching` 均空；可读取的 `/proc/*/{cwd,cmdline}` 范围内无两路径引用；此检查不是全机完备证明：独立 reviewer 扫描时有 2360 个 cwd / cmdline PermissionError，不可读部分由 root 当前 owner 与冻结 runner 闭包核对补足。

## 精确删除选集

各行 OID 均由 `git for-each-ref --merged=refs/heads/main` 选出；删除前逐行重验 `git merge-base --is-ancestor <OID> main`。无 worktree 分支无工作区 WIP 删除风险。

| branch | OID | worktree / 清洁证明 | 结果 |
|---|---|---|---|
| `codex/current-save-contract` | `b650dd7506417a3a09b8b5e9e4a0e5fdccea0cf9` | 未挂 worktree | branch 已删除 |
| `codex/merge-implementation-gaps` | `20e160b013c13ec8c017a3d1e7c2c7799cc24605` | 未挂 worktree | branch 已删除 |
| `codex/remaining-questions-and-features` | `ebf567b0e42d541a2016e843a71dc7ec0a4ab959` | 未挂 worktree | branch 已删除 |
| `docs/agents-md-single-source` | `19afee22a4e09f25659887efacbd300572bfedc5` | 未挂 worktree | branch 已删除 |
| `docs/company-system-checklist-close` | `2e4b77482c1f9410f8f29ad1b70113992efe6dfd` | 未挂 worktree | branch 已删除 |
| `docs/main-release-results` | `08e4fc75b52a71a3262a8a938c57b44f8b5b4960` | 未挂 worktree | branch 已删除 |
| `docs/test-health-handoff` | `a8a532d8be00b4af1981af24a95839b0f016a23d` | 未挂 worktree | branch 已删除 |
| `expl/bug-stocks6` | `7d6558c821fd1d1ddbc384fd6c99fa0ae615f894` | 未挂 worktree | branch 已删除 |
| `f-fixround` | `ce4680809e959e107f44d1a76bd49eaf22bff6ff` | 未挂 worktree | branch 已删除 |
| `feat/engine-local-admission-20260925` | `f4ad7b4d1ffeeaf8d3bca9a11c599ba1fee0a905` | 未挂 worktree | branch 已删除 |
| `feat/engine-ready-receipt-20260926` | `64aede4c2faa4a5d4198034aa33c01ad7da36a5e` | 未挂 worktree | branch 已删除 |
| `feat/engine-receipt-boundary-20260926` | `05a1705b5c468b57b075e051994338f0e5181573` | 未挂 worktree | branch 已删除 |
| `feat/engine-strategy-position-choice` | `b3e390a269a2feff0a65c7c9c10df0b624e730ae` | 未挂 worktree | branch 已删除 |
| `feat/host-company-contract-matrix` | `9793254e4bf244eb2cbad42e79a7654671d70da2` | 未挂 worktree | branch 已删除 |
| `feat/stock-bonus-differential-tax` | `c1561faf7aaf0caf4113489ae81566566b3ceb96` | 未挂 worktree | branch 已删除 |
| `feat/wasm-w1-env` | `fff1bc2911ea65a88acefe198479351fbaf223f4` | 未挂 worktree | branch 已删除 |
| `feat/web-reintegration-w3` | `b2f84b095e55e3e18d27c8ad4646e01ea641fd10` | 未挂 worktree | branch 已删除 |
| `fix/ci-workflows` | `7ef27ece5dc4ceb16fc5ba09ca74ae075f577aa8` | 未挂 worktree | branch 已删除 |
| `docs/luna-implementation-cost-2026-10-27` | `e4db69957d6304db30ffe5d5b658cf5efd4ae46c` | `/data1/baiyifan/workplace/stock_market_game/.worktree/luna-implementation-cost-2026-10-27`；完整 status 含 ignored 均空 | worktree 与 branch 均已删除 |
| `fix/finish-125-price-cage` | `35ac58a1a18cdea67c953758381bfff677404045` | `/data1/baiyifan/workplace/stock_market_game/.worktree/finish-125-price-cage`；完整 status 含 ignored 均空 | worktree 与 branch 均已删除 |

## 执行门禁与结果

- 冻结 runner / 活 owner 依赖确认：root 确认当前仅本轮 6 位 owner，18 个无树 ref 与 2 个旧树均无活 owner；对 restore / CivilE2 / Hostjoint02 / D3P1 / mixed 当前 runner 脚本、plan / manifest / freeze 批量检索，两旧树绝对路径 0 命中（root 工具证据 b14a25 / 94b241）。执行者对 5 个活树 agents 下 py / sh / mjs / cjs / json 再检索亦 0 命中。
- 未实施者独立审查：`/root/sol_branch_cleanup_29/sol_branch_cleanup_review_29`（实际 `gpt-6.1-sol`、`high`）已核实 20/20 OID 和 main 祖先关系、18 个未挂树、2 个完整 clean tree、main / 根身份和空 index；要求收窄 `/proc` 证据措辞，已修正并获删除前 `APPROVED`。
- 实际删除：已执行；只执行 `git worktree remove <精确路径>` 后从 clean main worktree 执行 `git branch -d -- <精确branch>`（只操作共享 ref，不改其 HEAD / index / 文件），失败即保留，不升级强制删除。
- 删除后执行者复核：20 个精确 ref 均不存在；2 个目录及 worktree 登记均不存在；root / main HEAD、root 4 个 tracked dirty 与空 index 完全一致，main 仍 clean。未触碰任何保护 worktree。未实施者删除后独立复核 `APPROVED`：20 个 ref、2 个目录和登记均不存在，原 OID 仍全部由 main 可达；main 与根身份、根原 4 项 tracked 修改、空 index 和两个共享保护树身份不变；无新增发现。

删除命令原始简报：

```text
worktree remove luna-implementation-cost-2026-10-27: exit=0 
worktree remove finish-125-price-cage: exit=0 
branch -d codex/current-save-contract: exit=0 Deleted branch codex/current-save-contract (was b650dd75).
branch -d codex/merge-implementation-gaps: exit=0 Deleted branch codex/merge-implementation-gaps (was 20e160b0).
branch -d codex/remaining-questions-and-features: exit=0 Deleted branch codex/remaining-questions-and-features (was ebf567b0).
branch -d docs/agents-md-single-source: exit=0 Deleted branch docs/agents-md-single-source (was 19afee22).
branch -d docs/company-system-checklist-close: exit=0 Deleted branch docs/company-system-checklist-close (was 2e4b7748).
branch -d docs/main-release-results: exit=0 Deleted branch docs/main-release-results (was 08e4fc75).
branch -d docs/test-health-handoff: exit=0 Deleted branch docs/test-health-handoff (was a8a532d8).
branch -d expl/bug-stocks6: exit=0 Deleted branch expl/bug-stocks6 (was 7d6558c8).
branch -d f-fixround: exit=0 Deleted branch f-fixround (was ce468080).
branch -d feat/engine-local-admission-20260925: exit=0 Deleted branch feat/engine-local-admission-20260925 (was f4ad7b4d).
branch -d feat/engine-ready-receipt-20260926: exit=0 Deleted branch feat/engine-ready-receipt-20260926 (was 64aede4c).
branch -d feat/engine-receipt-boundary-20260926: exit=0 Deleted branch feat/engine-receipt-boundary-20260926 (was 05a1705b).
branch -d feat/engine-strategy-position-choice: exit=0 Deleted branch feat/engine-strategy-position-choice (was b3e390a2).
branch -d feat/host-company-contract-matrix: exit=0 Deleted branch feat/host-company-contract-matrix (was 9793254e).
branch -d feat/stock-bonus-differential-tax: exit=0 Deleted branch feat/stock-bonus-differential-tax (was c1561faf).
branch -d feat/wasm-w1-env: exit=0 Deleted branch feat/wasm-w1-env (was fff1bc29).
branch -d feat/web-reintegration-w3: exit=0 Deleted branch feat/web-reintegration-w3 (was b2f84b09).
branch -d fix/ci-workflows: exit=0 Deleted branch fix/ci-workflows (was 7ef27ece).
branch -d docs/luna-implementation-cost-2026-10-27: exit=0 Deleted branch docs/luna-implementation-cost-2026-10-27 (was e4db6995).
branch -d fix/finish-125-price-cage: exit=0 Deleted branch fix/finish-125-price-cage (was 35ac58a1).
```

删除后实际 `df -B1 .`：

```text
Filesystem         1B-blocks          Used   Available Use% Mounted on
/dev/bcache0   4000787021824 3971079933952 19206467584 100% /data1
```

不以共享磁盘前后差值推算本任务独占释放量；两个旧树无 cache，分支清理主要减少本地 ref 与工作树数量。
