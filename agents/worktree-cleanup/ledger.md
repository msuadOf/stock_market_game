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

## 第二轮回收（2026-10-21）

用户批准后回收。**原则：先验证零未合并提交 + 干净工作区，再删；有未归档产物的先补档入库。**

### 已合并分支 worktree（5 个，约 107G）

全部核验 `git rev-list --count main..HEAD == 0` 且 `status --short` 干净：

| worktree | 分支 | 体积 |
|---|---|---|
| dayk-fix | feat/day-boundary-kline-fix | 60G |
| reint-b4b | feat/reintegration-b4b-session-assembly | 34G |
| reint-b4a | feat/reintegration-b4a-company-domain | 7.5G |
| recovery-gaps | fix/branch-gap-recovery | 7.1G |
| w3-bridge-fix | fix/w3-ingress-bridge-serial-crash | 863M |

`git worktree remove --force` + `git branch -d`（普通删除，非 -D）。

### std-thread-model（51G，exp/std-thread-model）

**未直接删除**——发现 24 项未跟踪的终验收证据（25M）。先补档入库两个提交再做：

- `3f4020bf` closing-independent：独立复核 review.md、终态 final-status.json、
  取证对照 forensic-comparison.json、变异测试结果、探针源码与四组输出日志。
- `6c379917` v5-p1-final + v5-repair-recheck：P1 终轮与修复再核验的探针/
  回归脚本（run_probes.py 等）、构建与用例 jsonl、失败清单。

`.log` 文件按仓库既有惯例（`.gitignore:44`）不入库，与主区既有 reviews
目录口径一致（60 个文件入库 31 个，其余均为日志）。补档后复核：无剩余
非日志未跟踪文件、无已跟踪脏文件，方执行 `git worktree remove --force` +
`git branch -D`（该分支内容已随 #102 压缩重做入 main，diff 显示 main 侧
净删 89081 行，即 main 为后续演化版本）。

### 结果

- 磁盘：76% → **72%**（可用 909G → 1.1T，回收约 158G）
- worktree：14 → **9**（剩 reint-b2 29G 未合并 126 提交、reint-b4c/wasm-r4a
  在途、wasm-p0 1.1G、wasm-toolchain 4.9G）
- `git worktree prune` 清掉 1 个 gitdir 指向失效位置的 `n3-review-baseline` 残留

### 未处理（如实登记）

`.claude/worktrees/` 下三个 harness 托管 worktree（agent-a448 95M / agent-abb
145M / agent-afc 3.8G）。三者均零未合并提交（agent-afc 分支已 merged），但
该目录归 Claude Code harness 管理，是否复用在协调者可见范围外，未经确认
不动。

### 追加：reint-b2（29G）

**先验证再删**（该分支 `git rev-list --count main..HEAD` = 126，看起来"未合并"，
不能仅凭此判死）：

1. `fe9e2ff5` 确为**单父提交**（`rev-list --parents` 输出 parents: 1）——
   重集成战役的既定"单父重联"模式：内容以复制方式进入 main，分支历史不
   作为第二父保留。
2. B2 五件套逐项在 main 树上锚定存在：`FeeComponents`（lib.rs）、
   `MinuteBarFact`（session.rs）、`RetainedDaySnapshot`（session/failure.rs）、
   `history_reads`（experience.rs）、`DeskTradeFact`（orderbook.rs）；
   台账 `b2-side-effects-ledger.md` 亦在 main。
3. `git diff fe9e2ff5..分支` 的 113 文件/8207 行差异方向为**分支侧缺失**——
   即 main 在 B2 之后又长出 native-store 等新内容，不是 B2 有未落地工作。
4. 工作区：已跟踪脏文件 0，仅 `node_modules` 未跟踪（构建依赖，可再生）。

结论：可回收。`git worktree remove --force` + `git branch -D`。

**回收后 worktree 共 4 个，全部必要**：reint-b4c 11G（B4c 在途）、
wasm-r4a 18G（R4a 阶段 2 在途）、wasm-toolchain 4.9G（待并入 foundation）、
wasm-p0 1.1G（P0 已合并，留作 wasm 线现场参照）。

---

## 附：LICENSE 被清空事件的根因与风险核实（2026-10-21/22）

### 现象
`.worktree/wasm-r4a/LICENSE` 两次被清空为 0 字节（20:33、21:56），均为
`git checkout --` 恢复，**未进任何提交**。全仓排查：主区与另三个 worktree 的
LICENSE 始终完好，范围未扩散。

### 根因（由 R4a 复现，非手误）
`wasm-pack 0.13.1` 的 "Copying license from crate" 步骤：当 `--out-dir` 传
**源路径与目标路径解析到同一个文件**时触发，与"绝对路径 / crate 目录外"无关。

wasm-pack v0.13.1 源码（经 `gh` 核对固定版本）：
- `src/command/build.rs:221`：`out_dir = crate_path.join(argument).clean()`
- `src/license.rs:73`：源 = `crate_path.join(license_file)`，目标 = `out_dir.join(license_file)`，随后 `fs::copy`

本仓库 `license_file = "../../LICENSE"`（相对 crate 目录 `/repo/apps/web-wasm`）。
令仓库根为 `/repo`：

| out-dir 参数 | 目标 LICENSE | 与源相同？ |
|---|---|---|
| `/repo/.tmp/pkgB` | `/repo/LICENSE` | **是 ⇒ 触发** |
| `../../.tmp/pkgB` (**相对路径**) | `/repo/LICENSE` | **也是 ⇒ 触发** |
| `/repo/.tmp/deeper/pkgB` | `/repo/.tmp/LICENSE` | 否 |

**"相对路径正常 / 仅 crate 外触发"的归因错误**——上表第二行是其反例。
**仅把 out-dir 改成相对路径无法解决**（取决于拼接结果是否等于源）。

### 纪律收获
两次事故都因「显式 pathspec 提交、禁用 `git add -A`」而未被裹挟进提交历史。
该纪律已再次向全体实现 agent 重申，并追加一条：构建后
`git status --short | grep -v '^??'` 自查，非本批范围内的文件被改动即红灯。

### Luna 复核批注（2026-10-22）与证据补齐

Luna 检查结论：**通过**，但对上节"生产不受影响"标了范围限定——
> 仓库里没有附上该次构建的原始命令和完整日志，因此我只能确认台账有实测记录，
> 不能独立复核那次历史运行；"CI 不受影响"也未见独立 CI 运行记录。

该批注成立（证据链"不可回填"纪律）。补齐如下。

**复跑命令（主区 main `74ed65ab`，2026-10-22）**：

```bash
cd /data1/baiyifan/workplace/stock_market_game
before=$(sha256sum LICENSE | cut -c1-16); echo "前: $before  size=$(stat -c %s LICENSE)"
# 命令取自 node scripts/frontend-build.mjs --dry-run 的实际出参（绝对路径 out-dir）
export RUSTUP_TOOLCHAIN=nightly-2026-09-05
wasm-pack build apps/web-wasm --target web --release \
  --out-dir /data1/baiyifan/workplace/stock_market_game/apps/web-wasm/wasm-pack-output/frontend/wasm/pkg
after=$(sha256sum LICENSE | cut -c1-16); echo "后: $after  size=$(stat -c %s LICENSE)"
```

**实测输出**：

```
前: e57d1f88b21c0b41  size=1062
warning: `web-wasm` (lib) generated 1 warning (run `cargo fix --lib -p web-wasm` to apply 1 suggestion)
    Finished `release` profile [optimized] target(s) in 1m 24s
[INFO]: ⬇️  Installing wasm-bindgen...
[INFO]: ✨   Done in 1m 30s
[INFO]: 📦   Your wasm pkg is ready to publish at .../apps/web-wasm/wasm-pack-output/frontend/wasm/pkg.
后: e57d1f88b21c0b41  size=1062   ✅ 未受影响
```

**仍如实保留的限定**：
- 这是**本机**单次运行（wasm-pack 0.13.1、nightly-2026-09-05），不是 CI 流水线运行；
  "CI 不受影响"仍是**由本机实测 + 路径形态（crate 目录内）推断**，未见 CI 实跑记录。
- 若要彻底消除该不确定性，应加 **post-build 校验门**（构建后断言 LICENSE 非空且
  sha 不变）。当前未做，登记为待办，不声称已解决。

### Sol 复核（2026-10-22）与 D 项改写

**机制归因已更正**（见上节）：触发条件是**源=目标同文件**，不是"绝对路径且
crate 外"。Sol 给出反例：`../../.tmp/pkgB` 这样的**相对** out-dir 同样会触发，
故"改成相对路径"不是解法。

**另两处 Sol 指出的不足，如实登记为待办（未做）**：

1. **产物 LICENSE 缺位**：生产 out-dir 下 LICENSE 的实际目标是
   `apps/web-wasm/wasm-pack-output/frontend/LICENSE`（已存在、hash 与源一致），
   而 **`frontend/wasm/pkg/LICENSE` 缺失**。因此"源 LICENSE hash 不变"**不能**
   证明 package 的许可文件被正确复制。待办：验证实际 pkg 中的 LICENSE 内容。
2. **应当加的门禁（当前未实现，不声称已解决）**：
   - 构建前/后校验源 LICENSE 非空且 hash 不变；
   - **源/目标别名检查**（同文件即拒绝）；
   - 异常必须**阻断**构建，而不是仅告警。

**仍保留的限定**：本机单次实测只支持"这次构建未截断源 LICENSE"，**不支持**
无条件的"生产与 CI 不会清空"；CI 未实测。
