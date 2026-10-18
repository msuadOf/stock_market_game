# 磁盘与陈旧 worktree 只读盘点（任务 #63 前置）

- 扫描时间：2026-10-17
- 扫描方式：只读（df / du / git worktree list / git status / git merge-base），**未删除、未修改任何文件**
- 原始数据：`.tmp/disk-worktree-scan/wt-meta.tsv`（104 行逐项元数据）、`root-du-level1.txt`、`branches-merged-main.txt`、`scan-worktrees.sh`（扫描脚本）

## 1. 磁盘概况

| 项 | 数值 |
|---|---|
| 文件系统 | /dev/bcache0 挂载 /data1，3.7T 总量 |
| 已用 | 3.6T（**99%**），仅剩 **66G** |
| 仓库根总占用 | **1.5T** |
| 仓库外附属 | /tmp/n3-review-baseline 5.2G；~/.codex/worktrees/7d2e 0.05G |

## 2. 仓库根一级目录占用（du -h --max-depth=1）

| 大小 | 目录 | 说明 |
|---|---|---|
| 582G | `.worktree/` | 89 个实现/实验 worktree |
| 438G | `.tmp/`（仓库根） | build-cache 208G、ready-market-progress-20260926 96G、market-cleanup-20260926 89G、ci-validation-20261001 16G、cargo-target 7.8G、order-cap-removal-20260925 7.6G、checklist-wave4 4.3G、concurrency-research-20260925 3.2G 等 |
| 241G | `target/`（仓库根） | debug 229G、release 5.5G、build-cache 4.7G |
| 189G | `.claude/` | 几乎全部为 `.claude/worktrees/`（agent/wf 自动 worktree） |
| 2.4G | `agents/` | 工作文件台账 |
| <1G | node_modules 261M、apps 137M、.git 89M、其余 | 正常 |

> 这四项合计 ≈1.45T，即磁盘水位压力几乎全部来自 worktree 副本、实验产物（.tmp）与 Rust 构建缓存（target），与源码本身无关。

## 3. worktree 总体判定

`git worktree list` 共 **104 个登记项**（含主仓库本身；其中 21 个是登记在别的 worktree `.tmp/` 内的嵌套树）。

判定标准：

- **可安全删**：HEAD 已并入 main（`git merge-base --is-ancestor`）+ 无未提交改动（dirty=0 且 untracked=0）+ 未加锁 + 非活跃工作区 + 最后提交 ≤2026-10-10
- **需确认**：有未提交改动 / HEAD 有未并入 main 的独立提交 / 被锁 / 近期仍在动 / 是冻结证据树
- **保留**：主仓库（main）、`.worktree/std-thread-model`（当前迁移工作区）、`.worktree/engine-max-performance`（1000x 性能主线台账所在，memory/project_engine_perf_1000x）

| 类别 | 数量 | 合计占用 | 其中 target | 其中 .tmp |
|---|---|---|---|---|
| 可安全删 | **24** | **≈134G** | 31G | ≈100G |
| 需确认 | 77 | ≈468G | 75G | ≈370G |
| 保留 | 2（当前区+性能主线） | ≈189G | 94G | ≈94G |
| （主仓库本体） | 1 | 1.5T 含上述 | 241G | 438G |

## 4. 可安全删清单（24 项，≈134G，仍须用户确认后执行）

全部位于 `.claude/worktrees/`（agent/wf 自动 worktree）及 1 个 `.worktree/ci-fix`；分支均已并入 main、工作区干净、9 天以上未动。大头实际是各树内的 gitignored 实验产物（.tmp/target）。

| 大小 | worktree | 分支 | 最后提交 |
|---|---|---|---|
| 32.1G | .claude/worktrees/agent-ab41856784a78cc9c | fix/engine-repurchase-share-units | 2026-10-09 |
| 29.0G | .claude/worktrees/agent-a34848a39507c5176 | test/cleanup-tooling-and-isolation | 2026-10-10 |
| 27.6G | .claude/worktrees/agent-a9b38446b5112eb51 | test/clear-baseline-failure-fixtures | 2026-10-09 |
| 17.7G | .claude/worktrees/agent-a06d9ddfb23cddf1f | test/migrate-test-contracts | 2026-10-09 |
| 8.6G* | .claude/worktrees/agent-a5f92437af100ea6d | feat/host-company-contract-matrix | 2026-10-10 |
| 5.7G | .claude/worktrees/wf_58e8fbc7-359-1 | worktree-wf_58e8fbc7-359-1 | 2026-10-08 |
| 4.0G | .claude/worktrees/wf_fc2c5f31-d83-2 | worktree-wf_fc2c5f31-d83-2 | 2026-10-06 |
| 2.5G | .claude/worktrees/agent-a0cf2217697ea6033 | fix/engine-repurchase-registry | 2026-10-09 |
| 1.9G | .worktree/ci-fix | fix/ci-workflows | 2026-10-01 |
| 1.2G | .claude/worktrees/wf_20860509-c0a-1 | worktree-wf_20860509-c0a-1 | 2026-10-08 |
| 其余 14 个 | 各 0.08–0.4G | 均已并入 main | 2026-10-06~10 |

*备注 a5f92437：其 `.tmp/pristine-check`（8.3G，detached、未并入 main）是嵌套登记 worktree，删父树前需先 `git worktree remove` 该嵌套树（它本身归入"需确认"）。

## 5. 需确认清单要点（77 项，≈468G；按占用排序的前几名）

| 大小 | worktree | 需确认原因 |
|---|---|---|
| 93.9G | .worktree/spine-simplify-01 | 10-15 仍活跃 + 有未提交改动 + 其 .tmp 内嵌 20 个冻结重建树（Round1-6 证据，见下） |
| 66.6G | .worktree/unified-book-single-owner-ad4aa88f | 157 个文件未提交，分支未并入 main |
| 57.0G | .worktree/arch14-single-pool-recovery-20261009-01 | 80 个文件未提交，未并入 main |
| 25.4G | .claude/worktrees/wf_ff9d1264-f14-2 | detached、未并入 main |
| 23.7G | .worktree/arch-fix-14-single-owner | 66 个文件未提交 |
| 23.3G | .worktree/arch-fix-91011 | 16 个文件未提交 |
| 21.7G | .worktree/arch-fix-company-single-owner | 53 个文件未提交 |
| 21.0G | .worktree/arch-fix-6-single-owner | 69 个文件未提交 |
| 17.3G | .worktree/arch-fix-910-single-owner | 15 个文件未提交 |
| 15.6G | .worktree/arch-fix-4-single-owner | 16 个文件未提交 |
| 10.7G | .claude/worktrees/wf_08fc61fc-57b-1 | 36 个文件未提交 |
| 9.2G | .worktree/arch23-p2-hardening | 11 个文件未提交 |
| 5.2G | /tmp/n3-review-baseline（仓库外） | detached 5bdf0953 未并入 main；/tmp 重启即失 |
| 其余 | 0.05–3.4G | 多为 wf 树少量 dirty、或 arch-fix/bseries 系列未提交 |

判定说明：

1. **arch-fix-*/arch14/bseries 系列未提交改动**：多为 2026-10-08 架构修复（任务 #54/#55 时期）的中间态；成果疑已通过 spine 路线进入 main，但 diff 本身未合并。若确认无需保留补丁文本，可整批释放 ≈180G。
2. **spine-simplify-01/.tmp 内 20 个嵌套登记树**（rebuild*/pre16/post27/gen-wt8 等，各 0.09–3.4G）：是 Round1-6 增量冻结的重建验证树。冻结证据（patch/manifest/index）是否在树外有副本需先核实，不可直接删。
3. **被锁 worktree**：agent-afc56ec75b2f12651（3.6G，干净且已并入 main）被 Claude 工具加锁——解锁后即可转"可安全删"。
4. **仓库外**：/tmp/n3-review-baseline 5.2G、~/.codex/worktrees/7d2e 0.05G。

## 6. 独立于 worktree 删除的可清理大项（target / .tmp）

- 各非当前 worktree 的 `target/` 合计 **≈108G**（其中 31G 在"可安全删"树内随树释放；其余 77G 可对保留树单独 cargo clean）。
- 仓库根 `target/debug` **229G**（可 `cargo clean` 重建，影响下次全量编译时间）。
- 仓库根 `.tmp/` 旧任务产物 **≈420G 可逐项清**：build-cache 208G、ready-market-progress-20260926 96G、market-cleanup-20260926 89G、ci-validation-20261001 16G 等（均为 9 月底/10 月初任务产物）。

## 7. 预计释放空间汇总（均须用户确认后执行）

| 动作 | 预计释放 |
|---|---|
| 删 24 个"可安全删" worktree | ≈134G |
| 确认后删 arch-fix/bseries 等未提交实验树 | 至多 ≈200G |
| 清仓库根 .tmp 旧任务产物 | 至多 ≈420G |
| cargo clean 根 target/debug + 各保留树 target | ≈300G |
| 合计上限 | **≈1.0T+**（当前全盘仅剩 66G，清理后可回到安全水位） |

## 8. 嵌套 worktree 删除顺序注意

`.worktree/spine-simplify-01/.tmp/**`（20 个）与 `.claude/worktrees/agent-a5f92437af100ea6d/.tmp/pristine-check`（1 个）是登记在其它 worktree `.tmp/` 内的嵌套树。直接 `rm -rf` 父目录会留下悬空登记，须先 `git worktree remove`（或事后 `git worktree prune`）逐个注销。

## 9. 全部 104 项明细（大小 | 分支 | 最后提交 | dirty/untracked | 已并入main | 判定 | 原因）

| 大小 | 分支 | 最后提交 | 脏/未跟踪 | 并入main | 判定 | 原因 |
|---|---|---|---|---|---|---|
| 1446.6G | refs/heads/main | 2026-10-10 | 4/21 | yes | 保留 | 主仓库,含下述全部子目录 |
| 17.7G | refs/heads/test/migrate-test-contracts | 2026-10-09 | 0/0 | yes | 可安全删 |  |
| 2.5G | refs/heads/fix/engine-repurchase-registry | 2026-10-09 | 0/0 | yes | 可安全删 |  |
| 0.3G | refs/heads/test/web-wasm-acceptance-guard | 2026-10-09 | 0/0 | yes | 可安全删 |  |
| 0.1G | refs/heads/docs/test-health-handoff | 2026-10-09 | 0/0 | yes | 可安全删 |  |
| 0.4G | refs/heads/worktree-agent-a1fb819e0b002019c | 2026-10-10 | 0/0 | yes | 可安全删 |  |
| 0.1G | refs/heads/docs/company-system-checklist-close | 2026-10-10 | 0/0 | yes | 可安全删 |  |
| 29.0G | refs/heads/test/cleanup-tooling-and-isolation | 2026-10-10 | 0/0 | yes | 可安全删 |  |
| 0.3G | refs/heads/feat/stock-bonus-differential-tax | 2026-10-10 | 0/0 | yes | 可安全删 |  |
| 8.6G | refs/heads/feat/host-company-contract-matrix | 2026-10-10 | 0/0 | yes | 可安全删 |  |
| 8.3G | 3b9e091ec19694e6f49a3b3c0f3c755f29837a3f | 2026-10-09 | 0/0 | no | 需确认 | 未并入main; |
| 27.6G | refs/heads/test/clear-baseline-failure-fixtures | 2026-10-09 | 0/0 | yes | 可安全删 |  |
| 0.1G | refs/heads/worktree-agent-aa88876b0d940c34d | 2026-10-08 | 0/2 | yes | 需确认 | 2未跟踪; |
| 32.1G | refs/heads/fix/engine-repurchase-share-units | 2026-10-09 | 0/0 | yes | 可安全删 |  |
| 3.6G | refs/heads/feat/dedupe-roe-reporting | 2026-10-10 | 0/0 | yes | 需确认 | 锁; |
| 0.3G | refs/heads/worktree-wf_0108822e-586-1 | 2026-10-08 | 3/0 | yes | 需确认 | 3文件改动; |
| 1.2G | refs/heads/worktree-wf_03f05b63-580-1 | 2026-10-08 | 4/0 | yes | 需确认 | 4文件改动; |
| 10.7G | refs/heads/worktree-wf_08fc61fc-57b-1 | 2026-10-08 | 36/2 | yes | 需确认 | 36文件改动;2未跟踪; |
| 0.3G | refs/heads/worktree-wf_08fc61fc-57b-2 | 2026-10-08 | 2/0 | yes | 需确认 | 2文件改动; |
| 1.2G | refs/heads/worktree-wf_08fc61fc-57b-3 | 2026-10-08 | 4/0 | yes | 需确认 | 4文件改动; |
| 0.1G | refs/heads/worktree-wf_08fc61fc-57b-4 | 2026-10-08 | 2/0 | yes | 需确认 | 2文件改动; |
| 0.5G | refs/heads/worktree-wf_08fc61fc-57b-5 | 2026-10-08 | 2/0 | yes | 需确认 | 2文件改动; |
| 0.1G | refs/heads/worktree-wf_08fc61fc-57b-6 | 2026-10-08 | 4/0 | yes | 需确认 | 4文件改动; |
| 0.3G | refs/heads/worktree-wf_0e6af0a3-0bb-1 | 2026-10-08 | 2/0 | no | 需确认 | 2文件改动;未并入main; |
| 0.3G | refs/heads/worktree-wf_10f2d092-94c-1 | 2026-10-07 | 51/0 | yes | 需确认 | 51文件改动; |
| 1.2G | refs/heads/worktree-wf_20860509-c0a-1 | 2026-10-08 | 0/0 | yes | 可安全删 |  |
| 0.3G | refs/heads/worktree-wf_295d21c5-185-1 | 2026-10-07 | 0/0 | yes | 可安全删 |  |
| 0.3G | refs/heads/worktree-wf_3a5f021f-720-1 | 2026-10-08 | 3/0 | yes | 需确认 | 3文件改动; |
| 0.3G | refs/heads/worktree-wf_50ee2b6c-18e-1 | 2026-10-08 | 2/0 | yes | 需确认 | 2文件改动; |
| 0.3G | refs/heads/worktree-wf_50ee2b6c-18e-2 | 2026-10-08 | 2/0 | yes | 需确认 | 2文件改动; |
| 0.4G | refs/heads/worktree-wf_5835e503-dc5-1 | 2026-10-08 | 0/0 | yes | 可安全删 |  |
| 5.7G | refs/heads/worktree-wf_58e8fbc7-359-1 | 2026-10-08 | 0/0 | yes | 可安全删 |  |
| 0.3G | refs/heads/worktree-wf_611ff3c3-839-1 | 2026-10-06 | 0/0 | yes | 可安全删 |  |
| 0.3G | refs/heads/fix/web-worker-error-transport | 2026-10-09 | 0/0 | yes | 可安全删 |  |
| 0.3G | refs/heads/worktree-wf_62c55998-34b-2 | 2026-10-08 | 4/0 | yes | 需确认 | 4文件改动; |
| 0.3G | refs/heads/worktree-wf_722872a1-cae-1 | 2026-10-08 | 0/0 | yes | 可安全删 |  |
| 0.1G | refs/heads/f-fixround | 2026-10-08 | 0/0 | yes | 可安全删 |  |
| 0.1G | refs/heads/worktree-wf_7d5713c4-666-1 | 2026-10-07 | 0/0 | yes | 可安全删 |  |
| 0.3G | refs/heads/worktree-wf_7ecbf73c-66f-1 | 2026-10-08 | 4/0 | yes | 需确认 | 4文件改动; |
| 0.1G | refs/heads/worktree-wf_8c01b0d5-05f-1 | 2026-10-07 | 1/1 | yes | 需确认 | 1文件改动;1未跟踪; |
| 0.3G | refs/heads/worktree-wf_90e7d2a3-5ba-1 | 2026-10-07 | 11/0 | no | 需确认 | 11文件改动;未并入main; |
| 0.1G | refs/heads/worktree-wf_95bfe91e-44b-1 | 2026-10-08 | 0/1 | yes | 需确认 | 1未跟踪; |
| 0.3G | refs/heads/worktree-wf_a9ae480c-bdf-1 | 2026-10-08 | 0/0 | yes | 可安全删 |  |
| 1.6G | refs/heads/worktree-wf_b10ab0b3-01c-1 | 2026-10-07 | 48/0 | yes | 需确认 | 48文件改动; |
| 0.3G | refs/heads/worktree-wf_b10ab0b3-01c-2 | 2026-10-07 | 41/7 | yes | 需确认 | 41文件改动;7未跟踪; |
| 0.3G | refs/heads/worktree-wf_b290d027-742-1 | 2026-10-08 | 2/0 | yes | 需确认 | 2文件改动; |
| 0.1G | refs/heads/worktree-wf_b7b5f8b2-289-1 | 2026-10-07 | 46/0 | yes | 需确认 | 46文件改动; |
| 0.9G | refs/heads/worktree-wf_c4129e87-ca9-1 | 2026-10-07 | 0/1 | yes | 需确认 | 1未跟踪; |
| 0.7G | refs/heads/worktree-wf_e3bd45f0-102-1 | 2026-10-08 | 34/3 | yes | 需确认 | 34文件改动;3未跟踪; |
| 0.3G | refs/heads/worktree-wf_e3bd45f0-102-2 | 2026-10-08 | 0/2 | yes | 需确认 | 2未跟踪; |
| 0.3G | refs/heads/worktree-wf_eb0a457b-037-1 | 2026-10-08 | 0/0 | yes | 可安全删 |  |
| 3.9G | refs/heads/worktree-wf_fc2c5f31-d83-2 | 2026-10-06 | 0/0 | yes | 可安全删 |  |
| 0.4G | refs/heads/worktree-wf_ff1b399a-014-1 | 2026-10-07 | 51/0 | yes | 需确认 | 51文件改动; |
| 5.4G | refs/heads/worktree-wf_ff9d1264-f14-1 | 2026-10-08 | 16/1 | yes | 需确认 | 16文件改动;1未跟踪; |
| 25.4G | 3d5bbd2a28a941ef16ddd5abca625e0f1ae0ba76 | 2026-10-08 | 16/0 | no | 需确认 | 16文件改动;未并入main; |
| 23.7G | refs/heads/fix/arch-14-single-compute-context | 2026-10-08 | 66/0 | no | 需确认 | 66文件改动;未并入main; |
| 0.1G | refs/heads/expl/arch-fix-23 | 2026-10-08 | 10/2 | no | 需确认 | 10文件改动;2未跟踪;未并入main; |
| 15.6G | refs/heads/expl/arch-fix-4-single-owner | 2026-10-08 | 16/0 | no | 需确认 | 16文件改动;未并入main; |
| 0.1G | refs/heads/expl/arch-fix-45 | 2026-10-08 | 13/0 | no | 需确认 | 13文件改动;未并入main; |
| 21.0G | refs/heads/fix/arch-fix-6-single-owner | 2026-10-08 | 69/1 | no | 需确认 | 69文件改动;1未跟踪;未并入main; |
| 3.0G | refs/heads/arch-fix-8-single-owner | 2026-10-08 | 66/1 | no | 需确认 | 66文件改动;1未跟踪;未并入main; |
| 17.3G | refs/heads/fix/arch-fix-910-single-owner-b82a92efa6 | 2026-10-08 | 15/0 | no | 需确认 | 15文件改动;未并入main; |
| 23.3G | refs/heads/expl/arch-fix-91011 | 2026-10-08 | 16/0 | no | 需确认 | 16文件改动;未并入main; |
| 21.7G | refs/heads/fix/arch-fix-company-single-owner | 2026-10-08 | 53/0 | no | 需确认 | 53文件改动;未并入main; |
| 0.1G | refs/heads/fix/arch14-single-pool-correction-20261009 | 2026-10-08 | 73/0 | no | 需确认 | 73文件改动;未并入main; |
| 57.0G | refs/heads/fix/arch14-single-pool-recovery-20261009-01 | 2026-10-08 | 80/0 | no | 需确认 | 80文件改动;未并入main; |
| 9.2G | refs/heads/fix/arch23-p2-hardening | 2026-10-08 | 11/3 | no | 需确认 | 11文件改动;3未跟踪;未并入main; |
| 0.1G | 8fd7b6e70e4a1f1149584b2fbf508ca806a0c44a | 2026-10-08 | 0/0 | no | 需确认 | 未并入main; |
| 0.1G | 8fd7b6e70e4a1f1149584b2fbf508ca806a0c44a | 2026-10-08 | 3/0 | no | 需确认 | 3文件改动;未并入main; |
| 0.1G | 8fd7b6e70e4a1f1149584b2fbf508ca806a0c44a | 2026-10-08 | 3/0 | no | 需确认 | 3文件改动;未并入main; |
| 0.1G | 8fd7b6e70e4a1f1149584b2fbf508ca806a0c44a | 2026-10-08 | 6/0 | no | 需确认 | 6文件改动;未并入main; |
| 0.1G | refs/heads/expl/bug-stocks6 | 2026-10-07 | 0/0 | no | 需确认 | 未并入main; |
| 1.9G | refs/heads/fix/ci-workflows | 2026-10-01 | 0/0 | yes | 可安全删 |  |
| 90.1G | refs/heads/fix/engine-max-performance | 2026-10-08 | 22/516 | no | 保留 | 性能主线台账(memory) |
| 29.2G | refs/heads/codex/implementation-audit-final | 2026-10-04 | 5/1 | yes | 需确认 | 5文件改动;1未跟踪; |
| 0.1G | refs/heads/docs/implementation-reaudit | 2026-10-04 | 2/1 | yes | 需确认 | 2文件改动;1未跟踪; |
| 3.4G | refs/heads/expl/parallel-matching | 2026-10-08 | 0/0 | no | 需确认 | 未并入main; |
| 2.2G | refs/heads/expl/same-price-cross-tick-study | 2026-10-07 | 1/4 | no | 需确认 | 1文件改动;4未跟踪;未并入main; |
| 0.4G | refs/heads/expl/same-price-owner-59b07a53de | 2026-10-07 | 0/0 | no | 需确认 | 未并入main; |
| 0.1G | refs/heads/expl/semantic-relax | 2026-10-08 | 0/0 | no | 需确认 | 未并入main; |
| 93.9G | refs/heads/temp/spine-endstate-snapshot | 2026-10-15 | 7/3 | no | 需确认 | 7文件改动;3未跟踪;未并入main;近期活跃; |
| 0.1G | 55b898c72ae37892e8f621fa2fe7cc84278c1ab9 | 2026-10-08 | 265/31 | no | 需确认 | 265文件改动;31未跟踪;未并入main;冻结证据树; |
| 0.1G | 55b898c72ae37892e8f621fa2fe7cc84278c1ab9 | 2026-10-08 | 262/27 | no | 需确认 | 262文件改动;27未跟踪;未并入main;冻结证据树; |
| 0.1G | 55b898c72ae37892e8f621fa2fe7cc84278c1ab9 | 2026-10-08 | 265/31 | no | 需确认 | 265文件改动;31未跟踪;未并入main;冻结证据树; |
| 0.1G | 55b898c72ae37892e8f621fa2fe7cc84278c1ab9 | 2026-10-08 | 262/27 | no | 需确认 | 262文件改动;27未跟踪;未并入main;冻结证据树; |
| 0.1G | 55b898c72ae37892e8f621fa2fe7cc84278c1ab9 | 2026-10-08 | 276/9 | no | 需确认 | 276文件改动;9未跟踪;未并入main;冻结证据树; |
| 0.1G | 55b898c72ae37892e8f621fa2fe7cc84278c1ab9 | 2026-10-08 | 45/1 | no | 需确认 | 45文件改动;1未跟踪;未并入main;冻结证据树; |
| 0.1G | 55b898c72ae37892e8f621fa2fe7cc84278c1ab9 | 2026-10-08 | 291/0 | no | 需确认 | 291文件改动;未并入main;冻结证据树; |
| 0.1G | 55b898c72ae37892e8f621fa2fe7cc84278c1ab9 | 2026-10-08 | 276/10 | no | 需确认 | 276文件改动;10未跟踪;未并入main;冻结证据树; |
| 0.1G | 55b898c72ae37892e8f621fa2fe7cc84278c1ab9 | 2026-10-08 | 276/10 | no | 需确认 | 276文件改动;10未跟踪;未并入main;冻结证据树; |
| 0.1G | 55b898c72ae37892e8f621fa2fe7cc84278c1ab9 | 2026-10-08 | 276/10 | no | 需确认 | 276文件改动;10未跟踪;未并入main;冻结证据树; |
| 1.8G | 55b898c72ae37892e8f621fa2fe7cc84278c1ab9 | 2026-10-08 | 278/10 | no | 需确认 | 278文件改动;10未跟踪;未并入main;冻结证据树; |
| 0.1G | 55b898c72ae37892e8f621fa2fe7cc84278c1ab9 | 2026-10-08 | 278/10 | no | 需确认 | 278文件改动;10未跟踪;未并入main;冻结证据树; |
| 0.1G | 55b898c72ae37892e8f621fa2fe7cc84278c1ab9 | 2026-10-08 | 292/0 | no | 需确认 | 292文件改动;未并入main;冻结证据树; |
| 0.1G | 55b898c72ae37892e8f621fa2fe7cc84278c1ab9 | 2026-10-08 | 292/0 | no | 需确认 | 292文件改动;未并入main;冻结证据树; |
| 0.1G | 55b898c72ae37892e8f621fa2fe7cc84278c1ab9 | 2026-10-08 | 264/30 | no | 需确认 | 264文件改动;30未跟踪;未并入main;冻结证据树; |
| 3.0G | 55b898c72ae37892e8f621fa2fe7cc84278c1ab9 | 2026-10-08 | 266/32 | no | 需确认 | 266文件改动;32未跟踪;未并入main;冻结证据树; |
| 3.4G | 55b898c72ae37892e8f621fa2fe7cc84278c1ab9 | 2026-10-08 | 270/35 | no | 需确认 | 270文件改动;35未跟踪;未并入main;冻结证据树; |
| 0.1G | 55b898c72ae37892e8f621fa2fe7cc84278c1ab9 | 2026-10-08 | 276/9 | no | 需确认 | 276文件改动;9未跟踪;未并入main;冻结证据树; |
| 0.1G | 55b898c72ae37892e8f621fa2fe7cc84278c1ab9 | 2026-10-08 | 277/9 | no | 需确认 | 277文件改动;9未跟踪;未并入main;冻结证据树; |
| 98.5G | refs/heads/exp/std-thread-model | 2026-10-17 | 1/0 | no | 保留 | 当前迁移工作区 |
| 66.6G | refs/heads/task53/unified-book-ad4aa88f | 2026-10-08 | 157/11 | no | 需确认 | 157文件改动;11未跟踪;未并入main; |
| 0.1G | 3f477ea0fa194a985d593d8e56a0a8a223ac3cbe | 2026-10-01 | 0/0 | no | 需确认 | 未并入main; |
| 5.2G | 5bdf095375bd943fe21959a85c31f9dbc9a6692c | 2026-10-07 | 0/0 | no | 需确认 | 未并入main; |

（行序=git worktree list 原始顺序；完整机器可读数据见 disk-worktree-scan/wt-meta.tsv）
