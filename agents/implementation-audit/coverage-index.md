# 全文审计覆盖索引

## 固定范围与阅读方法

- 源码基线 `4e64dad8efac7a762108fe4aeb91c7e84f51f6bb`；本轮起点 `9ca1d4ec1decf225f9f8d4f746119cf067b50628` 与之仅审计文档不同。
- 当前需求/历史文档 **142 个 Git 跟踪 Markdown 路径**，其中 `CLAUDE.md` 是 `AGENTS.md` 别名；额外纳入 **3 份未跟踪历史 draft** 和 **2 份已删除历史文档的最后版本**。共 **147 个来源路径**，不计本次审计产物自身。
- 使用 20 个并发审计 subagent；分批每个任务分配 1–3 篇原文，必须连续全文读到 EOF，再搜索生产代码核查。R01–R20 是主要需求组，S01–S31 是补充逐篇组，H01 是主控删除历史/根规则核销。这里的 S 编号是本轮具体阅读记录，不是总账 §7 旧版 S01–S06 六类概述。
- 每份记录给出原文范围、生产调用/消费、已有测试源码及判定。模板/流程/历史验收记录不虚构产品调用链；历史任务失败对照后续决定核销。搜索用于定位代码和复核，不替代原文全文阅读。
- “生产已接”只对所述契约负责，不保证整模块无缺陷；测试源码不是本轮运行通过。实现遗漏、未定范围、明确未来、文档漂移、验收证据分别归类。
- 这是本地可达文档集合的静态覆盖，不等于遍历每篇文档的每个 Git 修订，也不包含外部聊天、未入库文档、依赖/构建产物及 `.worktree/` 副本。另有 150 个跟踪 TXT 路径，均位于历史 evidence 下，属于原始测试/工具日志；未声称将 TXT/JSON/图片/参考 HTML 全部逐字重验。
- 主控逐项复核新增候选及原文适用关系；未运行游戏测试、构建、完整回归或联网制度/线上状态核验。总结果入口：[实现缺口总账](implementation-audit-2026-10-02.md)。

## 当前与草稿来源

| 来源路径（仓库根相对） | 逐项记录 |
|---|---|
| `.github/pull_request_template.md` | [S01](coverage/s01.md) |
| `.omo/drafts/escrow-parallel-engine.md` | [S31](coverage/s31.md) |
| `.omo/drafts/k7-deterministic-multicore-utilization.md` | [S31](coverage/s31.md) |
| `.omo/drafts/resolve-blockers-wayland.md` | [S30](coverage/s30.md) |
| `.omo/evidence/company-information-npc-intentions/compatibility-removal.md` | [S01](coverage/s01.md) |
| `.omo/evidence/company-information-npc-intentions/notepad-recovery.md` | [S02](coverage/s02.md) |
| `.omo/evidence/company-information-npc-intentions/task-1-review.md` | [S02](coverage/s02.md) |
| `.omo/evidence/company-information-npc-intentions/task-10-review.md` | [S02](coverage/s02.md) |
| `.omo/evidence/company-information-npc-intentions/task-11-review.md` | [S03](coverage/s03.md) |
| `.omo/evidence/company-information-npc-intentions/task-12-review.md` | [S03](coverage/s03.md) |
| `.omo/evidence/company-information-npc-intentions/task-13-review.md` | [S03](coverage/s03.md) |
| `.omo/evidence/company-information-npc-intentions/task-14-review.md` | [S04](coverage/s04.md) |
| `.omo/evidence/company-information-npc-intentions/task-15-review.md` | [S04](coverage/s04.md) |
| `.omo/evidence/company-information-npc-intentions/task-16-review.md` | [S04](coverage/s04.md) |
| `.omo/evidence/company-information-npc-intentions/task-17-review.md` | [S05](coverage/s05.md) |
| `.omo/evidence/company-information-npc-intentions/task-19-review.md` | [S05](coverage/s05.md) |
| `.omo/evidence/company-information-npc-intentions/task-2-review.md` | [S05](coverage/s05.md) |
| `.omo/evidence/company-information-npc-intentions/task-20-review.md` | [S06](coverage/s06.md) |
| `.omo/evidence/company-information-npc-intentions/task-21-review.md` | [S06](coverage/s06.md) |
| `.omo/evidence/company-information-npc-intentions/task-22-review.md` | [S06](coverage/s06.md) |
| `.omo/evidence/company-information-npc-intentions/task-24-review.md` | [S07](coverage/s07.md) |
| `.omo/evidence/company-information-npc-intentions/task-25-review.md` | [S07](coverage/s07.md) |
| `.omo/evidence/company-information-npc-intentions/task-26-review.md` | [S07](coverage/s07.md) |
| `.omo/evidence/company-information-npc-intentions/task-27-manual-continuation.md` | [S08](coverage/s08.md) |
| `.omo/evidence/company-information-npc-intentions/task-27-review.md` | [S08](coverage/s08.md) |
| `.omo/evidence/company-information-npc-intentions/task-28-review.md` | [S08](coverage/s08.md) |
| `.omo/evidence/company-information-npc-intentions/task-29-review.md` | [S09](coverage/s09.md) |
| `.omo/evidence/company-information-npc-intentions/task-3-review.md` | [S09](coverage/s09.md) |
| `.omo/evidence/company-information-npc-intentions/task-30-review.md` | [S09](coverage/s09.md) |
| `.omo/evidence/company-information-npc-intentions/task-33-review.md` | [S10](coverage/s10.md) |
| `.omo/evidence/company-information-npc-intentions/task-34-review.md` | [S10](coverage/s10.md) |
| `.omo/evidence/company-information-npc-intentions/task-36-review.md` | [S10](coverage/s10.md) |
| `.omo/evidence/company-information-npc-intentions/task-4-review.md` | [S11](coverage/s11.md) |
| `.omo/evidence/company-information-npc-intentions/task-7-review.md` | [S11](coverage/s11.md) |
| `.omo/evidence/company-information-npc-intentions/task-8-review.md` | [S11](coverage/s11.md) |
| `.omo/evidence/company-information-npc-intentions/task-9-review.md` | [S12](coverage/s12.md) |
| `.omo/evidence/company-information-npc-intentions/worktree-baseline.md` | [S12](coverage/s12.md) |
| `.omo/evidence/escrow-parallel-engine/F3/manual-qa/README.md` | [S12](coverage/s12.md) |
| `.omo/evidence/escrow-parallel-engine/task-10/divergence-audit.md` | [S13](coverage/s13.md) |
| `.omo/evidence/escrow-parallel-engine/task-11/execution-log.md` | [S13](coverage/s13.md) |
| `.omo/evidence/escrow-parallel-engine/task-12/validation.md` | [S13](coverage/s13.md) |
| `.omo/evidence/escrow-parallel-engine/task-3-8-smoke.md` | [S14](coverage/s14.md) |
| `.omo/evidence/escrow-parallel-engine/task-3/d6-d7/README.md` | [S14](coverage/s14.md) |
| `.omo/evidence/escrow-parallel-engine/task-3/d6-d7/structured-comparison.md` | [S14](coverage/s14.md) |
| `.omo/evidence/escrow-parallel-engine/task-8/acceptance-map.md` | [S15](coverage/s15.md) |
| `.omo/evidence/escrow-parallel-engine/task-9/corpus-diff.md` | [S15](coverage/s15.md) |
| `.omo/evidence/escrow-parallel-engine/task-9/historical-witness-audit.md` | [S15](coverage/s15.md) |
| `.omo/HANDOFF.md` | [S01](coverage/s01.md) |
| `.omo/notepads/company-information-npc-intentions/decisions.md` | [S16](coverage/s16.md) |
| `.omo/notepads/company-information-npc-intentions/issues.md` | [S16](coverage/s16.md) |
| `.omo/notepads/company-information-npc-intentions/learnings.md` | [S16](coverage/s16.md) |
| `.omo/notepads/company-information-npc-intentions/problems.md` | [S17](coverage/s17.md) |
| `.omo/plans/company-information-npc-intentions.md` | [R14](coverage/r14.md) |
| `.omo/plans/escrow-parallel-engine.md` | [R02](coverage/r02.md) |
| `.omo/plans/resolve-blockers-wayland.md` | [R20](coverage/r20.md) |
| `AGENTS.md` | [H01](coverage/h01.md) |
| `apps/web/README.md` | [S18](coverage/s18.md) |
| `CLAUDE.md` | [H01](coverage/h01.md) |
| `CONTRIBUTING.md` | [S17](coverage/s17.md) |
| `DESIGN.md` | [R17](coverage/r17.md) |
| `design/ui/mobile/qa/README.md` | [R17](coverage/r17.md) |
| `docs/actions-cache.md` | [S18](coverage/s18.md) |
| `docs/architecture.md` | [S18](coverage/s18.md) |
| `docs/build-and-deployment.md` | [S19](coverage/s19.md) |
| `docs/causal-diagnostics.md` | [S19](coverage/s19.md) |
| `docs/ci-build-fixes.md` | [S19](coverage/s19.md) |
| `docs/company-accounting.md` | [R03](coverage/r03.md) |
| `docs/company-actions-design.md` | [R03](coverage/r03.md) |
| `docs/decisions/0000-template.md` | [S20](coverage/s20.md) |
| `docs/decisions/0001-record-architecture-decisions.md` | [S20](coverage/s20.md) |
| `docs/decisions/0002-engine-rust-wasm.md` | [S20](coverage/s20.md) |
| `docs/decisions/0003-backend-rust.md` | [S21](coverage/s21.md) |
| `docs/decisions/0004-frontend-state-redux-toolkit.md` | [S21](coverage/s21.md) |
| `docs/decisions/0005-unified-engine-three-deployments.md` | [R19](coverage/r19.md) |
| `docs/decisions/0006-npc-strategy-module.md` | [R04](coverage/r04.md) |
| `docs/decisions/0007-three-deployment-frontend-framework.md` | [R19](coverage/r19.md) |
| `docs/decisions/0008-gpu-and-compute-offload.md` | [R18](coverage/r18.md) |
| `docs/decisions/0009-call-auction-and-intraday-axis.md` | [R06](coverage/r06.md) |
| `docs/decisions/0010-unified-host-protocol-and-local-refresh.md` | [R19](coverage/r19.md) |
| `docs/decisions/0011-market-time-observations-and-position-risk.md` | [R05](coverage/r05.md) |
| `docs/decisions/0012-retail-observation-to-target-position-loop.md` | [R05](coverage/r05.md) |
| `docs/decisions/0013-retail-experience-memory.md` | [R05](coverage/r05.md) |
| `docs/decisions/0014-closing-call-auction.md` | [R06](coverage/r06.md) |
| `docs/decisions/0015-parent-order-execution.md` | [R06](coverage/r06.md) |
| `docs/decisions/0016-fundamental-factor-model.md` | [R03](coverage/r03.md) |
| `docs/decisions/0017-escrow-parallel-tick.md` | [R02](coverage/r02.md) |
| `docs/decisions/0018-long-running-immutable-timeline.md` | [R01](coverage/r01.md) |
| `docs/decisions/0019-draft-market-scope-and-capacity.md` | [S21](coverage/s21.md) |
| `docs/decisions/0020-native-allocator-for-concurrent-ticks.md` | [R18](coverage/r18.md) |
| `docs/decisions/0021-strategy-position-choice-and-noise-pricing.md` | [R04](coverage/r04.md) |
| `docs/decisions/0022-symbolic-limit-prices.md` | [R16](coverage/r16.md) |
| `docs/decisions/0023-synthetic-history-and-matching-only.md` | [S22](coverage/s22.md) |
| `docs/decisions/0024-shrinking-investor-cash-pool.md` | [S22](coverage/s22.md) |
| `docs/decisions/0025-day-end-only-persistence.md` | [R01](coverage/r01.md) |
| `docs/decisions/0026-individual-institution-experience.md` | [R04](coverage/r04.md) |
| `docs/decisions/0027-runtime-deployment-and-build-targets.md` | [S22](coverage/s22.md) |
| `docs/decisions/0028-tagged-release-and-static-pages.md` | [S23](coverage/s23.md) |
| `docs/diagnostics.md` | [S23](coverage/s23.md) |
| `docs/error-handling.md` | [S23](coverage/s23.md) |
| `docs/git/AGENTS.md` | [S24](coverage/s24.md) |
| `docs/git/daily-workflow.md` | [S24](coverage/s24.md) |
| `docs/git/initialization.md` | [S24](coverage/s24.md) |
| `docs/implementation-gaps.md` | [S25](coverage/s25.md) |
| `docs/naming-conventions.md` | [S25](coverage/s25.md) |
| `docs/naming-refactor-validation.md` | [S25](coverage/s25.md) |
| `docs/open-questions.md` | [S26](coverage/s26.md) |
| `docs/price-volume-simulation-gap-checklist.md` | [R16](coverage/r16.md) |
| `docs/principles.md` | [S26](coverage/s26.md) |
| `docs/roadmap.md` | [S26](coverage/s26.md) |
| `docs/simulation-calendar.md` | [S27](coverage/s27.md) |
| `docs/superpowers/2026-09-13-company-information-archive.md` | [R20](coverage/r20.md) |
| `docs/superpowers/plans/2026-06-29-account.md` | [R07](coverage/r07.md) |
| `docs/superpowers/plans/2026-06-29-initial-positions.md` | [R12](coverage/r12.md) |
| `docs/superpowers/plans/2026-06-29-market.md` | [R09](coverage/r09.md) |
| `docs/superpowers/plans/2026-06-29-money-fixed-point.md` | [R13](coverage/r13.md) |
| `docs/superpowers/plans/2026-06-29-orderbook.md` | [R08](coverage/r08.md) |
| `docs/superpowers/plans/2026-06-29-session.md` | [R10](coverage/r10.md) |
| `docs/superpowers/plans/2026-06-29-strategy-impl.md` | [R11](coverage/r11.md) |
| `docs/superpowers/plans/2026-09-10-company-information-npc-intentions.md` | [R14](coverage/r14.md) |
| `docs/superpowers/plans/2026-09-13-resolve-blockers-wayland.md` | [R20](coverage/r20.md) |
| `docs/superpowers/plans/2026-09-24-single-world-multithreading.md` | [R15](coverage/r15.md) |
| `docs/superpowers/plans/2026-09-25-production-entry-and-thread-pool.md` | [R15](coverage/r15.md) |
| `docs/superpowers/plans/2026-09-26-ready-receipt-and-thread-boundary.md` | [R15](coverage/r15.md) |
| `docs/superpowers/plans/2026-09-26-sparse-continuous-book-feedback.md` | [R16](coverage/r16.md) |
| `docs/superpowers/README.md` | [S27](coverage/s27.md) |
| `docs/superpowers/specs/2026-06-29-account-design.md` | [R07](coverage/r07.md) |
| `docs/superpowers/specs/2026-06-29-gameconfig-design.md` | [R12](coverage/r12.md) |
| `docs/superpowers/specs/2026-06-29-initial-positions-design.md` | [R12](coverage/r12.md) |
| `docs/superpowers/specs/2026-06-29-market-design.md` | [R09](coverage/r09.md) |
| `docs/superpowers/specs/2026-06-29-money-fixed-point-design.md` | [R13](coverage/r13.md) |
| `docs/superpowers/specs/2026-06-29-orderbook-design.md` | [R08](coverage/r08.md) |
| `docs/superpowers/specs/2026-06-29-session-design.md` | [R10](coverage/r10.md) |
| `docs/superpowers/specs/2026-06-29-strategy-impl-design.md` | [R11](coverage/r11.md) |
| `docs/superpowers/specs/2026-09-11-company-information-handoff.md` | [S27](coverage/s27.md) |
| `docs/superpowers/specs/2026-09-13-company-information-issues.md` | [S28](coverage/s28.md) |
| `docs/superpowers/specs/2026-09-13-company-information-learnings.md` | [S28](coverage/s28.md) |
| `docs/superpowers/specs/2026-09-13-company-information-problems.md` | [S28](coverage/s28.md) |
| `docs/tech-stack.md` | [R18](coverage/r18.md) |
| `docs/test-cleanup-checklist.md` | [S29](coverage/s29.md) |
| `docs/testing.md` | [S29](coverage/s29.md) |
| `docs/trading-rules.md` | [S29](coverage/s29.md) |
| `docs/work-status.md` | [S30](coverage/s30.md) |
| `README.md` | [S17](coverage/s17.md) |
| `scripts/performance/README.md` | [S30](coverage/s30.md) |
| `UX-CONTRACT.md` | [R17](coverage/r17.md) |

## 删除历史补充

| 已删除路径 | 读取版本 | 逐项记录 |
|---|---|---|
| `scripts/simulation/escrow-corpus.md` | `21479883a9aa1ba6cda34df7c41e02018dec67d3^` | [H01](coverage/h01.md) |
| `packages/engine/tests/preserved-test-inventory.md` | `f4ad7b4d1ffeeaf8d3bca9a11c599ba1fee0a905^` | [H01](coverage/h01.md) |

现行文件以审计起点路径集合固定，删除历史以 `git log --all --diff-filter=D -- '*.md'` 与该集合求差；别名历史不重复算删除文档。详细核销依据及不能继承的旧验收结论见 H01。

## 未跟踪草稿内容指纹

以下只记录阅读来源，不把草稿复制进 Git，也不将其升级为 accepted 决定。

| 草稿 | 行数 | SHA-256 |
|---|---|---|
| `.omo/drafts/resolve-blockers-wayland.md` | 58 | `40406fb24b014bd532de855c3801b7d6f02e702f8c785fc19dd57185dd1f8824` |
| `.omo/drafts/k7-deterministic-multicore-utilization.md` | 167 | `3c3c76946bcd95699fd9dfafb37acbecc3b48456604a64409ca35192bae52045` |
| `.omo/drafts/escrow-parallel-engine.md` | 173 | `04cf8bb40a8e90a12bc96a5fdf5bcfd3b63cb0136a778402fc3dd3b9bacdc4ac` |

## 静态检查记录（2026-10-03）

- 142 个现存来源路径全部映射；R20 + S31 + H01 共 52 份记录齐备，缺失来源/记录均为 0。
- 全部审计 Markdown 的相对链接检查通过；具体代码路径存在性检查通过。4 个明确标为已删除/历史未建立的工具路径单独豁免，不伪称其当前存在。
- 文档检查使用 10 秒进程外期限；来源/链接检查与 diff 空白检查以两个独立进程并行执行，实际均不足 1 秒。
- 首次 Node 内调用 Git 的检查遇到 `spawnSync git EPERM`，未计为通过；改由 shell 提供 Git 清单、Node 只做文件校验后通过。`git diff --check` 和暂存区检查通过。
- 没有运行游戏单元测试、构建、完整回归、浏览器、性能矩阵或线上检查；此次检查不修改游戏代码和交易语义。

## 独立复核记录（2026-10-03）

- 非作者 subagent Poincare（`01a0fd52-c059-73f0-92a4-cd8ad76796e7`）确认完整阅读本批 55 份文档，并合并核对暂存内容与工作树修订；最终门禁通过，无未解决的 must-fix。
- 大 A 语义：区分真实规则、已登记简化、法源待核和未来范围，不把静态边界夸大为默认游戏已发生的故障。
- 必要性：改动限于审计文档；补充边界归入 G28/G36，不恢复已退役工具、不扩张 CI、不新增重复编号。
- 跨层与遗漏：已修正 S04/S16 的冲击公告消费链、S04 月/年封账接线判定和 S01 PR 模板行号；G35 的具体期末业务缺口与已接入的封账钩子分开。
- 复核没有运行测试、构建或回归；全文覆盖不构成程序无未知缺陷的保证。
