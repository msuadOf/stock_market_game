# worktree 磁盘回收台账（任务 #63）

- 日期：2026-10-19/20
- 执行前 df：/data1 已用 3.3T / 3.7T（90%，余 399G）；`.worktree/` 合计 582G
- 规程：删除前 `git worktree list` 核对 + 进程占用检查（/proc/*/cwd）；脏文件先在原分支冻结提交；分支一律不删；仅 `git worktree remove`（必要时 `--force` 仅限残留为 target/、node_modules 等可再生产物）
- 执行后 df：/data1 已用 2.7T / 3.7T（73%，余 1.1T），净回收约 740G
- 产出归档：.tmp/archive/engine-max-performance/（643M 文本证据）、.tmp/archive/spine-simplify-01/（788M 文本证据）
- 教训记录：①在无 .git 文件的尸体目录里跑 git 命令会向上解析到主仓 .git，把主仓 index 状态误读为该目录状态——.worktree/ 下无 .git 的目录一律视为删除残留直接 rm；②porcelain 对带空格文件名做 C-quote，cut 后带引号 add 会失败——untracked 冻结改用目录级 add + pathspec 排除；③git worktree remove 对大目录可能摘注册后因只读权限文件中断，目录本体残留由 chmod -R u+rwX + rm 收尾

## 已删除

规则：分支一律保留不删；脏文件先冻结提交（`chore: 冻结worktree残留再回收`）；detached HEAD 冻结后补 `frozen/<名>` 分支引用防 gc 丢失；`--force` 仅用于残留仅为 node_modules/target 等 ignored 可再生产物。

| worktree | 分支 | HEAD 变化 | 回收量 | 脏文件处置 |
|---|---|---|---|---|
| .worktree/cleanup-finish | feat/web-reintegration-w3（已并入 main ✓） | b2f84b09 | 7.2G | 干净 |
| .worktree/parallel-matching-study | expl/parallel-matching | 506ed2f2 | 3.5G | 干净 |
| .worktree/ci-fix | fix/ci-workflows | 7ef27ece | 2.0G | 干净 |
| .worktree/perf-baseline-5412fd10 | frozen/perf-baseline-5412fd10（补引用，另被 backup/pre-rewrite-main 含） | 5412fd10 | 1.3G | 干净 |
| .worktree/reint-b2-bench-base | frozen/reint-b2-bench-base（补引用，另被 backup/pre-rewrite-main 含） | 0d9f15a1 | 1.2G | 干净 |
| .worktree/same-price-owner-59b07a53de | expl/same-price-owner-59b07a53de | b81e6486 | 461M | 干净 |
| .worktree/bug-stocks6 | expl/bug-stocks6 | 7d6558c8 | 94M | 干净 |
| .worktree/semantic-relax | expl/semantic-relax | a42e704e | 94M | 干净 |
| .worktree/bseries-af555e50e2-b00 | 8fd7b6e7（被 expl/arch-fix-23 引用） | 8fd7b6e7 | 94M | 干净 |
| .worktree/same-price-cross-tick-study | expl/same-price-cross-tick-study | 25e4c509→0c25fad0 | 2.3G | 冻结提交（study 源码+测试） |
| .worktree/arch-fix-45 | expl/arch-fix-45 | 94808b9f→a546aea6 | 94M | 冻结提交 |
| .worktree/arch-fix-23 | expl/arch-fix-23 | 8fd7b6e7→9a08699a | 94M | 冻结提交 |
| .worktree/bseries-af555e50e2-b01 | frozen/bseries-af555e50e2-b01（原detached） | 8fd7b6e7→62c2a4f5 | 94M | 冻结提交 |
| .worktree/bseries-af555e50e2-b10 | frozen/bseries-af555e50e2-b10（原detached） | 8fd7b6e7→63b4abf5 | 94M | 冻结提交 |
| .worktree/bseries-af555e50e2-b11 | frozen/bseries-af555e50e2-b11（原detached） | 8fd7b6e7→d08c3abb | 94M | 冻结提交 |
| .worktree/implementation-reaudit | docs/implementation-reaudit | c0ab4299→cc8ec765 | 70M | 冻结提交 |
| .worktree/arch-fix-8-single-owner | arch-fix-8-single-owner | 55b898c7→322431b3 | 3.0G | 冻结提交 |
| .worktree/arch23-p2-hardening | fix/arch23-p2-hardening | 8fd7b6e7→29b805ae | 9.3G | 冻结提交 |
| .worktree/arch-fix-4-single-owner | expl/arch-fix-4-single-owner | 94808b9f→b19c580b | 16G | 冻结提交 |
| .worktree/arch-fix-910-single-owner | fix/arch-fix-910-single-owner-b82a92efa6 | 55b898c7→95b85925 | 18G | 冻结提交 |
| .worktree/arch-fix-6-single-owner | arch-fix-6-single-owner | 55b898c7→7b4a1ace | 21G | 冻结提交；--force=仅 node_modules |
| .worktree/arch-fix-company-single-owner | fix/arch-fix-company-single-owner | 55b898c7→912fec9c | 22G | 冻结提交 |
| .worktree/arch-fix-14-single-owner | fix/arch-14-single-compute-context | 55b898c7→0a6ccaf2 | 24G | 冻结提交 |
| .worktree/arch-fix-91011 | expl/arch-fix-91011 | 55b898c7→cc1a0ae5 | 24G | 冻结提交 |
| .worktree/arch14-single-pool-correction-20261009 | fix/arch14-single-pool-correction-20261009 | 55b898c7→635933ce | 94M | 冻结提交 |

另：清理了 spine-simplify-01 内 3 个卡死 6 天的孤儿测试进程、implementation-audit-final 内 15 个泄漏 15 天的 codex sandbox 孤儿进程（均 parent=1，kill 前核对 cwd/cmdline）。

### 大项与批量（第二批之后）

| worktree | 分支 | HEAD 变化 | 回收量 | 脏文件处置 |
|---|---|---|---|---|
| .worktree/implementation-audit-final | codex/implementation-audit-final | 7c207e05→03900d0d | 30G | 冻结提交；先清 15 天前 codex sandbox 泄漏孤儿进程 |
| .worktree/arch14-single-pool-recovery-20261009-01 | fix/arch14-single-pool-recovery-20261009-01 | 55b898c7→(冻结提交) | 58G | 冻结提交入分支；remove 摘注册后目录删除遇只读权限残留，按 coordinator 指示 chmod+rm 尸体 |
| .worktree/unified-book-single-owner-ad4aa88f | task53/unified-book-ad4aa88f | 55b898c7→122b911b | 68G | 同上（曾误判"并发迁移"，实为无 .git 尸体目录里 git 命令向上解析到主仓的观测假象，已结案） |
| .worktree/engine-max-performance | fix/engine-max-performance | 55b898c7→1d3eba5e→7839ef79 | 91G | 冻结提交（含带空格文件名补遗）；.tmp/ 文本证据 643MB 先归档至 .tmp/archive/engine-max-performance/；force=仅 node_modules |
| .worktree/spine-simplify-01 本体 | temp/spine-endstate-snapshot | 33027fcd→0ee7285d | 94G | 冻结提交；.tmp/ 文本证据 788MB 先归档至 .tmp/archive/spine-simplify-01/；先杀 3 个孤儿测试进程 |
| .worktree/spine-simplify-01/.tmp 内 19 个嵌套 worktree（ladder-verify/trees/{post27,pre16}、ladder4arm/trees/{post,pre}、merge-owner8-engine/{gen-wt8,o8-base,pre17-wt,rebuild8~14,17~21-wt}） | 各自 frozen/spine-*（原 detached，新建引用） | 55b898c7→各自冻结提交 | （含于上 94G） | 每个先冻结提交再删 |

### .claude/worktrees/ 批量（非 locked 共 57 个）

- 53 个 OK 冻结+删除（分支保留；detached 的建 frozen/ 引用：frozen/agent-a0a649ef01f1c318d、frozen/agent-abb7a96b599183e26、frozen/agent-ad9597b19c8a67693、frozen/pristine-check、frozen/wf_ff9d1264-f14-2）
- wf_62c55998-34b-1/-2：冻结提交 df994d3f 入各分支；remove 摘注册后目录遇只读 wasm-pkg 残留 → chmod+rm 尸体
- wf_e3bd45f0-102-1、wf_ff9d1264-f14-1：干净，force=仅 node_modules
- wf_95bfe91e-44b-1：干净（HEAD e5b3304d），force=仅 node_modules。注：处理中一度把名字误读为 44e-1，相关 stat/rm 均为不存在的路径（no-op 无损害），已核实纠正
- 保留：agent-afc56ec75b2f12651（locked，feat/dedupe-roe-reporting）

## 跳过项

| worktree | 大小 | 原因 |
|---|---|---|
| .worktree/reint-b2 | 29G | 前置核对未通过：feat/reintegration-b2-query-api 未并入 main（领先 main 126 commits、落后 147，双向分叉）；分支与提交保留原样，待上司决定合并后再回收 |
| .worktree/std-thread-model | 51G | 活跃 codex 运行时进程（app-server-broker/app-server/code-mode-host，2 天前启动）cwd 占用；待相关 codex 会话结束后再回收 |
| .worktree/reint-b4b、.worktree/w3-bridge-fix | 新增 | 清理期间外部新建的在途战役工作区，不属于本次候选，未触碰 |
