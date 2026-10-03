# 全文审计覆盖索引

## 固定范围与阅读方法

本索引用于核对来源是否覆盖以及结论依据何在；功能状态和开发缺口统一见 [实现缺口总账](implementation-audit-2026-10-02.md)。

当前源码基线为主工作区已提交的 `08e4fc7`，已合入 `.worktree/implementation-reaudit`；产品代码与 `2247f4f` 相同，新增main验收与test Release结果已全文核对。初次审计的52份R/S/H记录和后续8份代码记录保留原基线，本轮按当前caller重新核实旧缺口及已实现/取代项，并补扫后续工作记录。G27继续核销，原其余38项维持；新G40–G68共29项归入原分组，现行67项缺口。新增候选合并、降为Q或核销的依据见 [裁定记录](exhaustive-review/resolution.md)。

本轮来源集合包含 **229个跟踪Markdown路径**、**3份未跟踪历史草稿**、**2份删除文档最后版本**，共234个来源路径；`CLAUDE.md` 是AGENTS别名，保留路径不重复算独立正文。原147个来源全部重新读完，新增87份OOP/发布/main工作记录也全文核对。主批50个独立 `gpt-6-luna medium` 扫描任务各读1–3篇；后续29个工作记录任务及1个最新验收记录任务同样每人1–3篇、连续读至EOF，截图/截断补读与乱码限制写在逐篇记录中。主控再用搜索工具清点文件、漏实现标记、当前caller和反证，反查原文；搜索用于全文之后复核。所有来源行数、字节数、SHA-256和记录映射固定在 [机器清单](exhaustive-review/source-index.json)。

“生产已接”仅对记录中的契约负责，不保证整个模块无缺陷；测试源码不代表本轮运行通过。实现遗漏、待定范围、明确未来、文档漂移及验收债分别归类，历史失败须结合后续决定核销。本轮未修改游戏代码，只做源码差异和文档静态核对；未运行游戏测试、构建、完整回归、浏览器、联网法源或GitHub验证。发布和工作流4个短测文件通过属于上一基线复核；其他任务的OOP/发布验收结果也只作有基线的历史证据，不记为本轮重跑通过。

覆盖范围是本地可达的正式/历史Markdown及后续工作记录，含上述两份已删除最后版本和三份已读草稿；不把每篇全部Git修订、外部聊天、未入库来源、依赖/构建副本、原始TXT/JSON日志、图片或参考HTML都算作已读需求。被旧记录引用而当前树缺失的action-index等证据注明不可独立核验，不猜造内容；源码和可用新记录仍逐项核对。新发布结果属于来源记载的历史事实，本轮没有请求GitHub；主工作区未由本任务改写。

## 当前与草稿来源

| 来源路径（仓库根相对） | 逐项记录 |
|---|---|
| `.github/pull_request_template.md` | [S01](coverage/s01.md)；[本轮全文](exhaustive-review/luna21.md) |
| `.omo/drafts/escrow-parallel-engine.md` | [S31](coverage/s31.md)；[本轮全文](exhaustive-review/luna49.md) |
| `.omo/drafts/k7-deterministic-multicore-utilization.md` | [S31](coverage/s31.md)；[本轮全文](exhaustive-review/luna49.md) |
| `.omo/drafts/resolve-blockers-wayland.md` | [S30](coverage/s30.md)；[本轮全文](exhaustive-review/luna49.md) |
| `.omo/evidence/company-information-npc-intentions/compatibility-removal.md` | [S01](coverage/s01.md)；[本轮全文](exhaustive-review/luna21.md) |
| `.omo/evidence/company-information-npc-intentions/notepad-recovery.md` | [S02](coverage/s02.md)；[本轮全文](exhaustive-review/luna22.md) |
| `.omo/evidence/company-information-npc-intentions/task-1-review.md` | [S02](coverage/s02.md)；[本轮全文](exhaustive-review/luna22.md) |
| `.omo/evidence/company-information-npc-intentions/task-10-review.md` | [S02](coverage/s02.md)；[本轮全文](exhaustive-review/luna22.md) |
| `.omo/evidence/company-information-npc-intentions/task-11-review.md` | [S03](coverage/s03.md)；[本轮全文](exhaustive-review/luna23.md) |
| `.omo/evidence/company-information-npc-intentions/task-12-review.md` | [S03](coverage/s03.md)；[本轮全文](exhaustive-review/luna23.md) |
| `.omo/evidence/company-information-npc-intentions/task-13-review.md` | [S03](coverage/s03.md)；[本轮全文](exhaustive-review/luna23.md) |
| `.omo/evidence/company-information-npc-intentions/task-14-review.md` | [S04](coverage/s04.md)；[本轮全文](exhaustive-review/luna24.md) |
| `.omo/evidence/company-information-npc-intentions/task-15-review.md` | [S04](coverage/s04.md)；[本轮全文](exhaustive-review/luna24.md) |
| `.omo/evidence/company-information-npc-intentions/task-16-review.md` | [S04](coverage/s04.md)；[本轮全文](exhaustive-review/luna24.md) |
| `.omo/evidence/company-information-npc-intentions/task-17-review.md` | [S05](coverage/s05.md)；[本轮全文](exhaustive-review/luna25.md) |
| `.omo/evidence/company-information-npc-intentions/task-19-review.md` | [S05](coverage/s05.md)；[本轮全文](exhaustive-review/luna25.md) |
| `.omo/evidence/company-information-npc-intentions/task-2-review.md` | [S05](coverage/s05.md)；[本轮全文](exhaustive-review/luna25.md) |
| `.omo/evidence/company-information-npc-intentions/task-20-review.md` | [S06](coverage/s06.md)；[本轮全文](exhaustive-review/luna26.md) |
| `.omo/evidence/company-information-npc-intentions/task-21-review.md` | [S06](coverage/s06.md)；[本轮全文](exhaustive-review/luna26.md) |
| `.omo/evidence/company-information-npc-intentions/task-22-review.md` | [S06](coverage/s06.md)；[本轮全文](exhaustive-review/luna26.md) |
| `.omo/evidence/company-information-npc-intentions/task-24-review.md` | [S07](coverage/s07.md)；[本轮全文](exhaustive-review/luna27.md) |
| `.omo/evidence/company-information-npc-intentions/task-25-review.md` | [S07](coverage/s07.md)；[本轮全文](exhaustive-review/luna27.md) |
| `.omo/evidence/company-information-npc-intentions/task-26-review.md` | [S07](coverage/s07.md)；[本轮全文](exhaustive-review/luna27.md) |
| `.omo/evidence/company-information-npc-intentions/task-27-manual-continuation.md` | [S08](coverage/s08.md)；[本轮全文](exhaustive-review/luna28.md) |
| `.omo/evidence/company-information-npc-intentions/task-27-review.md` | [S08](coverage/s08.md)；[本轮全文](exhaustive-review/luna28.md) |
| `.omo/evidence/company-information-npc-intentions/task-28-review.md` | [S08](coverage/s08.md)；[本轮全文](exhaustive-review/luna28.md) |
| `.omo/evidence/company-information-npc-intentions/task-29-review.md` | [S09](coverage/s09.md)；[本轮全文](exhaustive-review/luna29.md) |
| `.omo/evidence/company-information-npc-intentions/task-3-review.md` | [S09](coverage/s09.md)；[本轮全文](exhaustive-review/luna29.md) |
| `.omo/evidence/company-information-npc-intentions/task-30-review.md` | [S09](coverage/s09.md)；[本轮全文](exhaustive-review/luna29.md) |
| `.omo/evidence/company-information-npc-intentions/task-33-review.md` | [S10](coverage/s10.md)；[本轮全文](exhaustive-review/luna30.md) |
| `.omo/evidence/company-information-npc-intentions/task-34-review.md` | [S10](coverage/s10.md)；[本轮全文](exhaustive-review/luna30.md) |
| `.omo/evidence/company-information-npc-intentions/task-36-review.md` | [S10](coverage/s10.md)；[本轮全文](exhaustive-review/luna30.md) |
| `.omo/evidence/company-information-npc-intentions/task-4-review.md` | [S11](coverage/s11.md)；[本轮全文](exhaustive-review/luna31.md) |
| `.omo/evidence/company-information-npc-intentions/task-7-review.md` | [S11](coverage/s11.md)；[本轮全文](exhaustive-review/luna31.md) |
| `.omo/evidence/company-information-npc-intentions/task-8-review.md` | [S11](coverage/s11.md)；[本轮全文](exhaustive-review/luna31.md) |
| `.omo/evidence/company-information-npc-intentions/task-9-review.md` | [S12](coverage/s12.md)；[本轮全文](exhaustive-review/luna32.md) |
| `.omo/evidence/company-information-npc-intentions/worktree-baseline.md` | [S12](coverage/s12.md)；[本轮全文](exhaustive-review/luna32.md) |
| `.omo/evidence/escrow-parallel-engine/F3/manual-qa/README.md` | [S12](coverage/s12.md)；[本轮全文](exhaustive-review/luna32.md) |
| `.omo/evidence/escrow-parallel-engine/task-10/divergence-audit.md` | [S13](coverage/s13.md)；[本轮全文](exhaustive-review/luna33.md) |
| `.omo/evidence/escrow-parallel-engine/task-11/execution-log.md` | [S13](coverage/s13.md)；[本轮全文](exhaustive-review/luna33.md) |
| `.omo/evidence/escrow-parallel-engine/task-12/validation.md` | [S13](coverage/s13.md)；[本轮全文](exhaustive-review/luna33.md) |
| `.omo/evidence/escrow-parallel-engine/task-3-8-smoke.md` | [S14](coverage/s14.md)；[本轮全文](exhaustive-review/luna34.md) |
| `.omo/evidence/escrow-parallel-engine/task-3/d6-d7/README.md` | [S14](coverage/s14.md)；[本轮全文](exhaustive-review/luna34.md) |
| `.omo/evidence/escrow-parallel-engine/task-3/d6-d7/structured-comparison.md` | [S14](coverage/s14.md)；[本轮全文](exhaustive-review/luna34.md) |
| `.omo/evidence/escrow-parallel-engine/task-8/acceptance-map.md` | [S15](coverage/s15.md)；[本轮全文](exhaustive-review/luna35.md) |
| `.omo/evidence/escrow-parallel-engine/task-9/corpus-diff.md` | [S15](coverage/s15.md)；[本轮全文](exhaustive-review/luna35.md) |
| `.omo/evidence/escrow-parallel-engine/task-9/historical-witness-audit.md` | [S15](coverage/s15.md)；[本轮全文](exhaustive-review/luna35.md) |
| `.omo/HANDOFF.md` | [S01](coverage/s01.md)；[本轮全文](exhaustive-review/luna21.md) |
| `.omo/notepads/company-information-npc-intentions/decisions.md` | [S16](coverage/s16.md)；[本轮全文](exhaustive-review/luna36.md) |
| `.omo/notepads/company-information-npc-intentions/issues.md` | [S16](coverage/s16.md)；[本轮全文](exhaustive-review/luna36.md) |
| `.omo/notepads/company-information-npc-intentions/learnings.md` | [S16](coverage/s16.md)；[本轮全文](exhaustive-review/luna36.md) |
| `.omo/notepads/company-information-npc-intentions/problems.md` | [S17](coverage/s17.md)；[本轮全文](exhaustive-review/luna37.md) |
| `.omo/plans/company-information-npc-intentions.md` | [R14](coverage/r14.md)；[本轮全文](exhaustive-review/luna03.md) |
| `.omo/plans/escrow-parallel-engine.md` | [R02](coverage/r02.md)；[本轮全文](exhaustive-review/luna20.md) |
| `.omo/plans/resolve-blockers-wayland.md` | [R20](coverage/r20.md)；[本轮全文](exhaustive-review/luna37.md) |
| `AGENTS.md` | [H01](coverage/h01.md)；[本轮全文](exhaustive-review/luna37.md) |
| `apps/web/README.md` | [S18](coverage/s18.md)；[本轮全文](exhaustive-review/luna39.md) |
| `CLAUDE.md` | [H01](coverage/h01.md)；[本轮全文](exhaustive-review/luna38.md) |
| `CONTRIBUTING.md` | [S17](coverage/s17.md)；[本轮全文](exhaustive-review/luna38.md) |
| `DESIGN.md` | [R17](coverage/r17.md)；[本轮全文](exhaustive-review/luna02.md) |
| `design/ui/mobile/qa/README.md` | [R17](coverage/r17.md)；[本轮全文](exhaustive-review/luna02.md) |
| `docs/actions-cache.md` | [S18](coverage/s18.md)；[工具现行政策](reaudit-tools.md)；[本轮全文](exhaustive-review/luna12.md) |
| `docs/architecture.md` | [S18](coverage/s18.md)；[本轮全文](exhaustive-review/luna39.md) |
| `docs/build-and-deployment.md` | [S19](coverage/s19.md)；[工具现行政策](reaudit-tools.md)；[本轮全文](exhaustive-review/luna12.md) |
| `docs/causal-diagnostics.md` | [S19](coverage/s19.md)；[本轮全文](exhaustive-review/luna13.md) |
| `docs/ci-build-fixes.md` | [S19](coverage/s19.md)；[本轮全文](exhaustive-review/luna39.md) |
| `docs/company-accounting.md` | [R03](coverage/r03.md)；[本轮全文](exhaustive-review/luna04.md) |
| `docs/company-actions-design.md` | [R03](coverage/r03.md)；[本轮全文](exhaustive-review/luna04.md) |
| `docs/decisions/0000-template.md` | [S20](coverage/s20.md)；[本轮全文](exhaustive-review/luna40.md) |
| `docs/decisions/0001-record-architecture-decisions.md` | [S20](coverage/s20.md)；[本轮全文](exhaustive-review/luna40.md) |
| `docs/decisions/0002-engine-rust-wasm.md` | [S20](coverage/s20.md)；[本轮全文](exhaustive-review/luna40.md) |
| `docs/decisions/0003-backend-rust.md` | [S21](coverage/s21.md)；[本轮全文](exhaustive-review/luna41.md) |
| `docs/decisions/0004-frontend-state-redux-toolkit.md` | [S21](coverage/s21.md)；[本轮全文](exhaustive-review/luna41.md) |
| `docs/decisions/0005-unified-engine-three-deployments.md` | [R19](coverage/r19.md)；[本轮全文](exhaustive-review/luna01.md) |
| `docs/decisions/0006-npc-strategy-module.md` | [R04](coverage/r04.md)；[本轮全文](exhaustive-review/luna05.md) |
| `docs/decisions/0007-three-deployment-frontend-framework.md` | [R19](coverage/r19.md)；[本轮全文](exhaustive-review/luna41.md) |
| `docs/decisions/0008-gpu-and-compute-offload.md` | [R18](coverage/r18.md)；[本轮全文](exhaustive-review/luna10.md) |
| `docs/decisions/0009-call-auction-and-intraday-axis.md` | [R06](coverage/r06.md)；[本轮全文](exhaustive-review/luna07.md) |
| `docs/decisions/0010-unified-host-protocol-and-local-refresh.md` | [R19](coverage/r19.md)；[本轮全文](exhaustive-review/luna01.md) |
| `docs/decisions/0011-market-time-observations-and-position-risk.md` | [R05](coverage/r05.md)；[本轮全文](exhaustive-review/luna06.md) |
| `docs/decisions/0012-retail-observation-to-target-position-loop.md` | [R05](coverage/r05.md)；[本轮全文](exhaustive-review/luna06.md) |
| `docs/decisions/0013-retail-experience-memory.md` | [R05](coverage/r05.md)；[本轮全文](exhaustive-review/luna06.md) |
| `docs/decisions/0014-closing-call-auction.md` | [R06](coverage/r06.md)；[本轮全文](exhaustive-review/luna07.md) |
| `docs/decisions/0015-parent-order-execution.md` | [R06](coverage/r06.md)；[本轮全文](exhaustive-review/luna08.md) |
| `docs/decisions/0016-fundamental-factor-model.md` | [R03](coverage/r03.md)；[本轮全文](exhaustive-review/luna03.md) |
| `docs/decisions/0017-escrow-parallel-tick.md` | [R02](coverage/r02.md)；[本轮全文](exhaustive-review/luna08.md) |
| `docs/decisions/0018-long-running-immutable-timeline.md` | [R01](coverage/r01.md)；[本轮全文](exhaustive-review/luna10.md) |
| `docs/decisions/0019-draft-market-scope-and-capacity.md` | [S21](coverage/s21.md)；[本轮全文](exhaustive-review/luna08.md) |
| `docs/decisions/0020-native-allocator-for-concurrent-ticks.md` | [R18](coverage/r18.md)；[本轮全文](exhaustive-review/luna10.md) |
| `docs/decisions/0021-strategy-position-choice-and-noise-pricing.md` | [R04](coverage/r04.md)；[本轮全文](exhaustive-review/luna05.md) |
| `docs/decisions/0022-symbolic-limit-prices.md` | [R16](coverage/r16.md)；[本轮全文](exhaustive-review/luna07.md) |
| `docs/decisions/0023-synthetic-history-and-matching-only.md` | [S22](coverage/s22.md)；[本轮全文](exhaustive-review/luna11.md) |
| `docs/decisions/0024-shrinking-investor-cash-pool.md` | [S22](coverage/s22.md)；[本轮全文](exhaustive-review/luna11.md) |
| `docs/decisions/0025-day-end-only-persistence.md` | [R01](coverage/r01.md)；[本轮全文](exhaustive-review/luna11.md) |
| `docs/decisions/0026-individual-institution-experience.md` | [R04](coverage/r04.md)；[本轮全文](exhaustive-review/luna05.md) |
| `docs/decisions/0027-runtime-deployment-and-build-targets.md` | [S22](coverage/s22.md)；[工具现行政策](reaudit-tools.md)；[本轮全文](exhaustive-review/luna01.md) |
| `docs/decisions/0028-tagged-release-and-static-pages.md` | [S23](coverage/s23.md)；[工具现行政策](reaudit-tools.md)；[本轮全文](exhaustive-review/luna12.md) |
| `docs/diagnostics.md` | [S23](coverage/s23.md)；[本轮全文](exhaustive-review/luna13.md) |
| `docs/error-handling.md` | [S23](coverage/s23.md)；[本轮全文](exhaustive-review/luna14.md) |
| `docs/git/AGENTS.md` | [S24](coverage/s24.md)；[本轮全文](exhaustive-review/luna42.md) |
| `docs/git/daily-workflow.md` | [S24](coverage/s24.md)；[本轮全文](exhaustive-review/luna42.md) |
| `docs/git/initialization.md` | [S24](coverage/s24.md)；[本轮全文](exhaustive-review/luna42.md) |
| `docs/implementation-gaps.md` | [S25](coverage/s25.md)；[本轮全文](exhaustive-review/luna43.md) |
| `docs/naming-conventions.md` | [S25](coverage/s25.md)；[本轮全文](exhaustive-review/luna43.md) |
| `docs/naming-refactor-validation.md` | [S25](coverage/s25.md)；[本轮全文](exhaustive-review/luna43.md) |
| `docs/open-questions.md` | [S26](coverage/s26.md)；[本轮全文](exhaustive-review/luna44.md) |
| `docs/price-volume-simulation-gap-checklist.md` | [R16](coverage/r16.md)；[本轮全文](exhaustive-review/luna13.md) |
| `docs/principles.md` | [S26](coverage/s26.md)；[本轮全文](exhaustive-review/luna44.md) |
| `docs/roadmap.md` | [S26](coverage/s26.md)；[本轮全文](exhaustive-review/luna44.md) |
| `docs/simulation-calendar.md` | [S27](coverage/s27.md)；[本轮全文](exhaustive-review/luna04.md) |
| `docs/superpowers/2026-09-13-company-information-archive.md` | [R20](coverage/r20.md)；[本轮全文](exhaustive-review/luna45.md) |
| `docs/superpowers/plans/2026-06-29-account.md` | [R07](coverage/r07.md)；[本轮全文](exhaustive-review/luna15.md) |
| `docs/superpowers/plans/2026-06-29-initial-positions.md` | [R12](coverage/r12.md)；[本轮全文](exhaustive-review/luna17.md) |
| `docs/superpowers/plans/2026-06-29-market.md` | [R09](coverage/r09.md)；[本轮全文](exhaustive-review/luna18.md) |
| `docs/superpowers/plans/2026-06-29-money-fixed-point.md` | [R13](coverage/r13.md)；[本轮全文](exhaustive-review/luna17.md) |
| `docs/superpowers/plans/2026-06-29-orderbook.md` | [R08](coverage/r08.md)；[本轮全文](exhaustive-review/luna16.md) |
| `docs/superpowers/plans/2026-06-29-session.md` | [R10](coverage/r10.md)；[本轮全文](exhaustive-review/luna19.md) |
| `docs/superpowers/plans/2026-06-29-strategy-impl.md` | [R11](coverage/r11.md)；[本轮全文](exhaustive-review/luna19.md) |
| `docs/superpowers/plans/2026-09-10-company-information-npc-intentions.md` | [R14](coverage/r14.md)；[本轮全文](exhaustive-review/luna03.md) |
| `docs/superpowers/plans/2026-09-13-resolve-blockers-wayland.md` | [R20](coverage/r20.md)；[本轮全文](exhaustive-review/luna45.md) |
| `docs/superpowers/plans/2026-09-24-single-world-multithreading.md` | [R15](coverage/r15.md)；[本轮全文](exhaustive-review/luna09.md) |
| `docs/superpowers/plans/2026-09-25-production-entry-and-thread-pool.md` | [R15](coverage/r15.md)；[本轮全文](exhaustive-review/luna09.md) |
| `docs/superpowers/plans/2026-09-26-ready-receipt-and-thread-boundary.md` | [R15](coverage/r15.md)；[本轮全文](exhaustive-review/luna09.md) |
| `docs/superpowers/plans/2026-09-26-sparse-continuous-book-feedback.md` | [R16](coverage/r16.md)；[本轮全文](exhaustive-review/luna20.md) |
| `docs/superpowers/README.md` | [S27](coverage/s27.md)；[本轮全文](exhaustive-review/luna45.md) |
| `docs/superpowers/specs/2026-06-29-account-design.md` | [R07](coverage/r07.md)；[本轮全文](exhaustive-review/luna15.md) |
| `docs/superpowers/specs/2026-06-29-gameconfig-design.md` | [R12](coverage/r12.md)；[本轮全文](exhaustive-review/luna15.md) |
| `docs/superpowers/specs/2026-06-29-initial-positions-design.md` | [R12](coverage/r12.md)；[本轮全文](exhaustive-review/luna17.md) |
| `docs/superpowers/specs/2026-06-29-market-design.md` | [R09](coverage/r09.md)；[本轮全文](exhaustive-review/luna18.md) |
| `docs/superpowers/specs/2026-06-29-money-fixed-point-design.md` | [R13](coverage/r13.md)；[本轮全文](exhaustive-review/luna18.md) |
| `docs/superpowers/specs/2026-06-29-orderbook-design.md` | [R08](coverage/r08.md)；[本轮全文](exhaustive-review/luna16.md) |
| `docs/superpowers/specs/2026-06-29-session-design.md` | [R10](coverage/r10.md)；[本轮全文](exhaustive-review/luna19.md) |
| `docs/superpowers/specs/2026-06-29-strategy-impl-design.md` | [R11](coverage/r11.md)；[本轮全文](exhaustive-review/luna20.md) |
| `docs/superpowers/specs/2026-09-11-company-information-handoff.md` | [S27](coverage/s27.md)；[本轮全文](exhaustive-review/luna46.md) |
| `docs/superpowers/specs/2026-09-13-company-information-issues.md` | [S28](coverage/s28.md)；[本轮全文](exhaustive-review/luna46.md) |
| `docs/superpowers/specs/2026-09-13-company-information-learnings.md` | [S28](coverage/s28.md)；[本轮全文](exhaustive-review/luna46.md) |
| `docs/superpowers/specs/2026-09-13-company-information-problems.md` | [S28](coverage/s28.md)；[本轮全文](exhaustive-review/luna47.md) |
| `docs/tech-stack.md` | [R18](coverage/r18.md)；[本轮全文](exhaustive-review/luna47.md) |
| `docs/test-cleanup-checklist.md` | [S29](coverage/s29.md)；[本轮全文](exhaustive-review/luna14.md) |
| `docs/testing.md` | [S29](coverage/s29.md)；[工具现行政策](reaudit-tools.md)；[本轮全文](exhaustive-review/luna14.md) |
| `docs/trading-rules.md` | [S29](coverage/s29.md)；[本轮全文](exhaustive-review/luna16.md) |
| `docs/work-status.md` | [S30](coverage/s30.md)；[本轮全文](exhaustive-review/luna47.md) |
| `README.md` | [S17](coverage/s17.md)；[本轮全文](exhaustive-review/luna38.md) |
| `scripts/performance/README.md` | [S30](coverage/s30.md)；[本轮全文](exhaustive-review/luna48.md) |
| `UX-CONTRACT.md` | [R17](coverage/r17.md)；[本轮全文](exhaustive-review/luna02.md) |
| `agents/main-release-validation/scripts-isolated-final-results.md` | [本轮全文](exhaustive-review/sweep81.md)；后续工作记录，按当前生产源码核对 |
| `agents/main-release-validation/summary.md` | [本轮全文](exhaustive-review/sweep81.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/domain/accounting-result.md` | [本轮全文](exhaustive-review/luna51.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/domain/accounting-review.md` | [本轮全文](exhaustive-review/luna51.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/domain/bank-result.md` | [本轮全文](exhaustive-review/luna52.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/domain/bank-review.md` | [本轮全文](exhaustive-review/luna52.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/domain/coordination.md` | [本轮全文](exhaustive-review/luna52.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/domain/diagnostics-result.md` | [本轮全文](exhaustive-review/luna53.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/domain/diagnostics-review.md` | [本轮全文](exhaustive-review/luna53.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/domain/experience-result.md` | [本轮全文](exhaustive-review/luna53.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/domain/experience-review.md` | [本轮全文](exhaustive-review/luna54.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/domain/final-summary.md` | [本轮全文](exhaustive-review/luna54.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/domain/industrial-result.md` | [本轮全文](exhaustive-review/luna54.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/domain/industrial-review.md` | [本轮全文](exhaustive-review/luna55.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/domain/insurance-result.md` | [本轮全文](exhaustive-review/luna55.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/domain/insurance-review.md` | [本轮全文](exhaustive-review/luna55.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/domain/operations-result.md` | [本轮全文](exhaustive-review/luna56.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/domain/operations-review.md` | [本轮全文](exhaustive-review/luna56.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/domain/orderbook-result.md` | [本轮全文](exhaustive-review/luna56.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/domain/orderbook-review.md` | [本轮全文](exhaustive-review/luna57.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/domain/real-estate-result.md` | [本轮全文](exhaustive-review/luna57.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/domain/real-estate-review.md` | [本轮全文](exhaustive-review/luna57.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/final-review/final-review.md` | [本轮全文](exhaustive-review/luna58.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/final-review/initial-boundary.md` | [本轮全文](exhaustive-review/luna58.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/final-review/progress-01.md` | [本轮全文](exhaustive-review/luna58.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/frontend/app.md` | [本轮全文](exhaustive-review/luna59.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/frontend/chart.md` | [本轮全文](exhaustive-review/luna59.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/frontend/host.md` | [本轮全文](exhaustive-review/luna59.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/frontend/mobile.md` | [本轮全文](exhaustive-review/luna60.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/frontend/remote.md` | [本轮全文](exhaustive-review/luna60.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/frontend/request-owners.md` | [本轮全文](exhaustive-review/luna60.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/frontend/review.md` | [本轮全文](exhaustive-review/luna61.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/frontend/verification.md` | [本轮全文](exhaustive-review/luna61.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/frontend/wasm.md` | [本轮全文](exhaustive-review/luna61.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/hosts/account-callers.md` | [本轮全文](exhaustive-review/luna62.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/hosts/actors/status.md` | [本轮全文](exhaustive-review/luna62.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/hosts/coordination.md` | [本轮全文](exhaustive-review/luna62.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/hosts/deadline/status.md` | [本轮全文](exhaustive-review/luna63.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/hosts/fixtures1/status.md` | [本轮全文](exhaustive-review/luna63.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/hosts/fixtures2/status.md` | [本轮全文](exhaustive-review/luna63.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/hosts/performance/status.md` | [本轮全文](exhaustive-review/luna64.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/hosts/review-account.md` | [本轮全文](exhaustive-review/luna64.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/hosts/review-actors.md` | [本轮全文](exhaustive-review/luna64.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/hosts/review-build.md` | [本轮全文](exhaustive-review/luna65.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/hosts/review-deadline.md` | [本轮全文](exhaustive-review/luna65.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/hosts/review-fixtures1.md` | [本轮全文](exhaustive-review/luna65.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/hosts/review-fixtures2.md` | [本轮全文](exhaustive-review/luna66.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/hosts/review-performance.md` | [本轮全文](exhaustive-review/luna66.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/hosts/review-production-entry-caller.md` | [本轮全文](exhaustive-review/luna66.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/hosts/review-server.md` | [本轮全文](exhaustive-review/luna67.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/hosts/server/status.md` | [本轮全文](exhaustive-review/luna67.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/pipeline/auction/implementation.md` | [本轮全文](exhaustive-review/luna67.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/pipeline/continuous/implementation.md` | [本轮全文](exhaustive-review/luna68.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/pipeline/core-implementation.md` | [本轮全文](exhaustive-review/luna68.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/pipeline/projections/implementation.md` | [本轮全文](exhaustive-review/luna68.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/pipeline/review-auction.md` | [本轮全文](exhaustive-review/luna69.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/pipeline/review-continuous-core.md` | [本轮全文](exhaustive-review/luna69.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/pipeline/review-resources-projections.md` | [本轮全文](exhaustive-review/luna69.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/pipeline/review-stream-callers.md` | [本轮全文](exhaustive-review/luna70.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/pipeline/state-callers0.md` | [本轮全文](exhaustive-review/luna70.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/pipeline/state-callers1.md` | [本轮全文](exhaustive-review/luna70.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/pipeline/stream/implementation.md` | [本轮全文](exhaustive-review/luna71.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/README.md` | [本轮全文](exhaustive-review/luna51.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/session/caller-review.md` | [本轮全文](exhaustive-review/luna71.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/session/core-review.md` | [本轮全文](exhaustive-review/luna71.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/session/implementation.md` | [本轮全文](exhaustive-review/luna72.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/session/leaf-review.md` | [本轮全文](exhaustive-review/luna72.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/session/lifecycle-review.md` | [本轮全文](exhaustive-review/luna72.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/session/lifecycle.md` | [本轮全文](exhaustive-review/luna73.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/session/persistence-snapshot.md` | [本轮全文](exhaustive-review/luna73.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/session/plans-strategy.md` | [本轮全文](exhaustive-review/luna73.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/session/roots-callers.md` | [本轮全文](exhaustive-review/luna74.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/session/roots-review.md` | [本轮全文](exhaustive-review/luna74.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/session/roots.md` | [本轮全文](exhaustive-review/luna74.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/summary.md` | [本轮全文](exhaustive-review/luna75.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-refactor-implementation/validation/rust-short-plan.md` | [本轮全文](exhaustive-review/luna75.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-release-validation/build-only-change.md` | [本轮全文](exhaustive-review/luna75.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-release-validation/build-only-review.md` | [本轮全文](exhaustive-review/luna76.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-release-validation/company-report-selection-fix.md` | [本轮全文](exhaustive-review/luna76.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-release-validation/publish-tag-sha-tdd.md` | [本轮全文](exhaustive-review/luna76.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-release-validation/release-process-checklist.md` | [本轮全文](exhaustive-review/luna77.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-release-validation/review.md` | [本轮全文](exhaustive-review/luna77.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-release-validation/scripts-final-results.md` | [本轮全文](exhaustive-review/luna77.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-release-validation/scripts-results.md` | [本轮全文](exhaustive-review/luna78.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-release-validation/scripts-supervised-results.md` | [本轮全文](exhaustive-review/luna78.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-release-validation/scripts-unsandboxed-results.md` | [本轮全文](exhaustive-review/luna78.md)；后续工作记录，按当前生产源码核对 |
| `agents/oop-release-validation/summary.md` | [本轮全文](exhaustive-review/luna79.md)；后续工作记录，按当前生产源码核对 |

完整来源与逐篇阅读记录如上；对于代码更新后的实现状态，可按领域继续查阅下列复核记录。它们承接原文映射，作为代码复核证据；Luna全文记录已经上表逐路径映射，不重复计算为需求来源。

| 代码复核范围 | 记录 |
|---|---|
| G01–G05、G18–G20；三宿主实际消费链 | [宿主](reaudit-host.md) |
| G06–G09、G16、G28、G35–G38；Q02/Q11；策略与个人状态 | [Engine](reaudit-engine.md) |
| G10–G14、G22–G25、G30–G34；Q04/Q07/Q08 | [UI](reaudit-ui.md) |
| G21/G26/G39、已核销G27及Q05；构建发布及验证工具 | [工具](reaudit-tools.md) |
| G15/G17/G29、Q01/Q03/Q06/Q09；日历/配置/指标接缝 | [基础](reaudit-foundations.md) |
| 已实现 Account/Position/OrderBook/Market 契约 | [核心](reaudit-core-contracts.md) |
| 已实现候选、提交、受理、结算、日界与持久化契约 | [流水线](reaudit-pipeline-contracts.md) |
| 已实现分录/余额/报告/行业状态契约及底层失败边界 | [会计](reaudit-accounting-contracts.md) |

## 删除历史补充

| 已删除路径 | 读取版本 | 逐项记录 |
|---|---|---|
| `scripts/simulation/escrow-corpus.md` | `21479883a9aa1ba6cda34df7c41e02018dec67d3^` | [H01](coverage/h01.md)；[本轮全文](exhaustive-review/luna50.md) |
| `packages/engine/tests/preserved-test-inventory.md` | `f4ad7b4d1ffeeaf8d3bca9a11c599ba1fee0a905^` | [H01](coverage/h01.md)；[本轮全文](exhaustive-review/luna50.md) |

现行文件以审计起点路径集合固定，删除历史以 `git log --all --diff-filter=D -- '*.md'` 与该集合求差；别名历史不重复算删除文档。详细核销依据及不能继承的旧验收结论见 H01。

## 未跟踪草稿内容指纹

以下只记录阅读来源，不把草稿复制进 Git，也不将其升级为 accepted 决定。

| 草稿 | 行数 | SHA-256 |
|---|---|---|
| `.omo/drafts/resolve-blockers-wayland.md` | 58 | `40406fb24b014bd532de855c3801b7d6f02e702f8c785fc19dd57185dd1f8824`；[本轮全文](exhaustive-review/luna49.md) |
| `.omo/drafts/k7-deterministic-multicore-utilization.md` | 167 | `3c3c76946bcd95699fd9dfafb37acbecc3b48456604a64409ca35192bae52045`；[本轮全文](exhaustive-review/luna49.md) |
| `.omo/drafts/escrow-parallel-engine.md` | 173 | `04cf8bb40a8e90a12bc96a5fdf5bcfd3b63cb0136a778402fc3dd3b9bacdc4ac`；[本轮全文](exhaustive-review/luna49.md) |

## 静态检查记录

- 初次全文审计：142 个现存来源路径全部映射；R20 + S31 + H01 共 52 份记录齐备，缺失来源/记录均为 0。
- 全部审计 Markdown 的相对链接检查通过；具体代码路径存在性检查通过。4 个明确标为已删除/历史未建立的工具路径单独豁免，不伪称其当前存在。
- 文档检查使用 10 秒进程外期限；来源/链接检查与 diff 空白检查以两个独立进程并行执行，实际均不足 1 秒。
- 首次 Node 内调用 Git 的检查遇到 `spawnSync git EPERM`，未计为通过；改由 shell 提供 Git 清单、Node 只做文件校验后通过。`git diff --check` 和暂存区检查通过。
- 没有运行游戏单元测试、构建、完整回归、浏览器、性能矩阵或线上检查；此次检查不修改游戏代码和交易语义。
- OOP基线复核：当时39个唯一G编号全部登记，8份代码记录齐备；10个改动文档的相对链接和 diff 空白检查通过。文档校验与 diff 检查以两个进程并行，分别设10秒外部期限，均不足1秒。

- 上一基线核对：G27公开前重查已实现并核销；发布/工作流4个测试文件在10秒case/进程树期限内、并发4运行通过，耗时约1.53秒。具体命令与覆盖边界见 [工具记录](reaudit-tools.md)。
- `08e4fc7` 全文复核：234个来源映射到80份Luna扫描记录；旧38项重新证实、新增29项，共67项待完成，G27单独核销。扫描源与引用版本指纹已记录；测试源码、历史通过与本轮静态检查分别说明，不以样本通过保证整模块。
- 上一批5份文档的相对链接、38项唯一待办与G27核销完整性，以及diff空白检查通过；两进程并行、各10秒期限，整批不足1秒。首次检查器误识别三级标题，修正定位后通过，没有改待办迎合检查。
- 本轮扩大范围的来源/内容指纹、80份当前映射记录、67项唯一G及G27核销、234个来源索引、163份改动Markdown相对链接与diff空白检查均通过。三条独立校验命令并行，各有10秒进程外期限，实际均不足1秒；没有运行游戏回归。首轮检查发现luna78报告落在主工作区而未进入审计worktree，保留全文移入正确目录后重验通过，没有删除来源或弱化检查。

## 独立复核记录

- 初次全文审计由非作者 subagent Poincare（`01a0fd52-c059-73f0-92a4-cd8ad76796e7`）确认完整阅读该批55份文档，并合并核对暂存内容与工作树修订；最终门禁通过，无未解决的 must-fix。
- 大 A 语义：区分真实规则、已登记简化、法源待核和未来范围，不把静态边界夸大为默认游戏已发生的故障。
- 必要性：改动限于审计文档；补充边界归入 G28/G36，不恢复已退役工具、不扩张 CI、不新增重复编号。
- 跨层与遗漏：已修正 S04/S16 的冲击公告消费链、S04 月/年封账接线判定和 S01 PR 模板行号；G35 的具体期末业务缺口与已接入的封账钩子分开。
- 复核没有运行测试、构建或回归；全文覆盖不构成程序无未知缺陷的保证。

OOP代码更新复核由非作者 `/root/independent_review` 完整审阅8份代码记录及总账/index修改，并反查关键新旧调用链；三项门禁通过，无剩余 must-fix。复核确认未把已有局部能力算成新修复、未把旧底层失败面误报为回归，且区分了 Session 回滚与底层方法的原子边界。审阅范围是该批报告，不冒充对494个变化文件全部重读；官方规则有效性和未运行场景不作额外背书。上一批发布差异由 `/root/latest_release_review` 完整审阅8份文档diff，并反查相关生产改动；确认当时G27核销与38项待办、Q05收窄及发布仅构建政策一致，三项门禁通过，无剩余必须修复项。

上一批增量核对由非作者 `/root/current_commit_audit_review` 完整审阅5份文档diff，并反查至 `2247f4f` 的新增源码差异及G27/G37/G39入口。复核发现核对期间主工作区HEAD前进，已合入新提交并修正基线、变化文件数与未提交范围，随后再次复核通过。三项门禁结论为：沿用既有A股语义，费用守恒及内部checkpoint不改变日终保存约定；改动仅同步审计且融入原章节；未把测试源码/历史通过记录冒充本轮运行结果，无剩余必须修复项。复核未运行游戏测试或联网核验法源。

本轮全文复核由未实施改动的 `/root/latest_release_review` 连续读完暂存区164文件完整diff，包括luna01–79、sweep01–81、总账、索引、裁定和来源指纹；截断输出逐段补读。独立核对234来源及229/3/2分类、80份当前映射与每人1–3来源、163份Markdown链接、67个唯一现行G及G27核销，全部通过。Node检查遇spawnSync git EPERM后明确改用10秒进程外期限的只读校验，约1.6秒完成，不把首次失败计为通过。

三项门禁通过：交易确认/入队/成交和分/股概念保持一致，未新增或冒充官方规则；范围限定审计目录，旧38项与新29项准确归类；Remote请求终态、保险保障期、Rust/Web成本、deadline等关键caller已反查，原始候选分歧由裁定记录清楚收口，无剩余必须修复项。独立审查未修改文件、未执行Git写操作，未运行游戏测试、回归或联网法源核验；产品源码未由本任务改动。
