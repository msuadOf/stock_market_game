# 隐藏审计批次 003

## 范围与完整性

- 调查基线：`43b1aa5f25226c72976ca172d32f4a8eeb2272ad`；caller worktree `HEAD` 与之相同。只读检查，未运行测试/构建，未改产品代码或执行 Git 写操作。
- `scan-plan.json` batch `id=3` 的三个源条目均与源 root 文件逐一匹配：

| 源文件 | 行数 | SHA-256 | EOF |
|---|---:|---|---|
| `agents/oop-refactor-audit/challenge-2026-10-03/final-review.md` | 370 | `3d29e6b9c24eac464e09b04f81e874a805953f7577ba3a5d2f0e79a9081f1796` | 是，分段连续读到第370行；章节 1–9 均读完，FR-final-01 至 05 及绑定清单已核。 |
| `agents/oop-refactor-audit/challenge-2026-10-03/frontend/final-review.md` | 179 | `ef4da0ce9746008d2d9051f616e04db447d60e213ec7dd14b014efd7c5e4c7e5` | 是，分段连续读到第179行；产物/覆盖、F01–F04、三道门禁、42项处置及终表均读完。 |
| `agents/oop-refactor-audit/challenge-2026-10-03/frontend/reader-instructions.md` | 13 | `e9098c435fff13a0823de10c47eea33709a245dfd8dda871ab4d550d756a8b0d` | 是，13行全文；源码读取、去重、证据、输出和禁止事项均读完。 |

源文件清单字段：每项 aliases=1，未发现别名路径需要另行复核。以上文件由 `source_root=/data1/baiyifan/workplace/stock_market_game` 读取；行数与 SHA 均为实算值。

## 当前规范与基线

已读 caller worktree 的 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`、`docs/architecture.md`，及适用决策 ADR-0010、0017、0018 §7/相关承接段、0021、0025、0026、0027、0028。ADR-0017 的旧来源排序由 ADR-0018 §7/§11 修订；NPC 仓位遵 ADR-0021，账户/策略经历遵 ADR-0026，存档边界遵 ADR-0025；ADR-0027/0028 固定部署和发布范围。未用旧 reader instructions 取代当前仓库指令。此批没有改变交易制度，未以对象抽取重新解释 A 股单位、交易时序或存档语义。

## 代码链与候选核验

对照 43b1aa5 的 caller 代码，出现关键的时间/状态错位：最终复核称 128 候选“全部尚未实施”，但若干候选在当前基线已有同一字段、行为与真实 caller 的 owner。它们是调查材料候选，且多为低优先级/可选项，不等于用户批准的实施任务；因此“owner 已存在”不构成遗漏修复，也不应另做重复包装。

| 候选/源结论 | caller 代码、调用链与实际状态 | G 关联及结论 |
|---|---|---|
| `hosts-R2-N18` / FR-final-01，资源会话；候选被列为 new | `scripts/performance/market-ui-report.mjs:233-313` 的 `MarketUiReportRun` 已拥有 config、server、browser、client、profile 和资源取得/清理方法；`main():316-416` 调用 `startServerIfNeeded`→`launchBrowser`→`connectPage`，并在 finally 调 `close`。与候选 `PerformanceReportSession` 的字段、方法、caller、单次释放次序实质相同。FR-final-01 修订后的“不幂等、按 client→browser→server→profile 单次清理”与代码一致。该提案在基线已覆盖，new 标签应回核，而非声称代码漏做。 | 关联已登记 `G60/G61`（`implementation-audit-2026-10-02.md:128-129`）：G60 是性能工具不能越过生产启动选择；G61 是缺外部 deadline/异常清理不能收敛。现行调用链代码证据见 `exhaustive-review/luna48.md:21-33`，包括 close 任一步失败会跳过余项。对象化 N18 不修复/核销 G60、G61；G61 的实际缺口仍成立。此为已有 G，不新造重复项。 |
| `domain-R2-N28` / FR-final-02，`RetailPositionDecisionContext`；候选 new | `packages/engine/src/behavior/heuristics.rs:55-204` 已有具名 `RetailPositionDecisionContext`、`from_observations`、`target_for` 与 `apply_experience_confidence`；`behavior/decision.rs:11-618` 保留风险与优先级纯决策链并调用 context，`behavior/mod.rs:69-119` 是公开入口，`strategy/zi_noise.rs` 是外层 caller。现实现精确具名了持仓输入并集中目标股数、整手、可卖量/T+1 输出。原报告仍以 `current_position_inputs` 元组和九参数 `decision_for_action` 为当前代码证据，已不符合本基线。 | 与 ADR-0021 的 100 股买入手数/目标仓位语义、ADR-0026 个体经历边界相邻；有关 G06–G09/G38 分别是行为/分类调用链缺口，不能由此 context 声称核销。当前 context 符合本批范围，没有新遗漏证据；wide policy 已按 FR-final-02 明确拒绝。 |
| `frontend-R2-N04`，`CompanyRequestRegistry`；候选 new/optional | `apps/web/src/host/company-request-registry.ts:1-23` 已实现 requestSequence、activeRequests、`begin/matches/finishIfCurrent/clear`；`company-query-coordinator.ts:1-3,46-70,86-140` 组合该 owner，page 与 by-id 两入口使用 ticket，baseline/dispose 清除，force 替换保留旧 finally guard。报告所提对象和方法已实际落地在同名独立文件。 | 无对应 G 项。与 ADR-0010 的宿主/前端协议局部责任相容，不涉及公共披露期限或交易事实。候选应标为已实现/covered，非新遗漏。 |
| `domain-R2-N39` / FR-final-03，阶段账本；候选 new、低优先级可选 | `phase_timing.rs:22-92` 已定义阶段身份、ALL、rank/name/index 及 `TickPhase` 映射；`:224-319` 的 `PhaseTimingLedger` 拥有 accumulator slots、采样/溢出、前置阶段完整性、固定顺序 records；`:334-380` Collector 驱动；`:405-420` 只在 committed tick 捕获。提案主体 owner 及数据/验证语义已存在，剩余可能只是将阶段 enum 映射进一步折入 ledger 的位置调整，没有已批准功能承诺。 | `G39` 是 K7 同受理事实验收缺口，与 recorder 的阶段账本对象化不同；N39 不能核销 G39。FR-final-03 的 registry 配置容量说明符合代码 `current_runnable_threads() = rayon::current_num_threads()`，不应标作 OS runnable/CPU 使用率。没有遗漏交易实现的证据。 |
| `FR-final-04` 当前三栏显示修正 | 该 finding 自述的是调查输出中把 rejected 项留作建议的问题；其关闭通过核对统一展示/coverage 修复，不是产品需求或产品代码差异。 | 不对应产品 G；闭合只代表审计材料一致。 |
| `FR-final-05` / `FractionUnits` 单位展示 | `packages/engine/src/accounting/amount.rs:95-112` 的累计基点路径以 10,000 为分母；bank `loans.rs:23,31,361-392` 与 real-estate `loans.rs:15,182-211` 的 ACT/365F 用 3,650,000。模块注释已区分 1/3,650,000 分；源码算法与既有单位没有发现漂移。 | 不对应 G，不改变沪深 A 股交易单位或费用规则。审计提案只能澄清不同算法的余数单位，不能推出统一分母/算法。 |

### G 关联边界

- G60、G61 是本批唯一被直接触及的已登记生产/工具缺口。N18 是资源所有权整理，不代表正式 UI 性能旅程可达，也不提供长验证进程外 deadline。G61 已有检查依据 `agents/implementation-audit/exhaustive-review/luna48.md:27-33`；相关结构性清理缺陷可由实际串行 await 顺序证明，但本轮没运行故障注入或浏览器。
- G39 与 N39 的阶段计时结构相邻但语义不同：报告不可以用 `PhaseTimingLedger` 的存在声称 K7 的跨 worker 验收已修。
- N28 没有相关 G 债务可由纯仓位目标上下文核销；A 股手数/T+1 语义在当前 code 中仍按既有行为表达，本次没有重验官方交易所法源。

## 发现

1. **审计清单状态过时（确认）。** 最终复核的“全部候选尚未实施”与基线中 `MarketUiReportRun`、`RetailPositionDecisionContext`、`CompanyRequestRegistry`、`PhaseTimingLedger` 等实际 owner 不相符。至少前三项与候选字段/方法/生产调用链高度重合，N39 主体也已存在。应将候选标记重新按 43b1aa5 复核为已覆盖/冗余可选结构建议；这是审计状态/去重问题，不是产品行为缺失。源文件自身明确低优先级或 optional，不能将其倒推出先前实施批准。
2. **G60/G61 残余仍在（确认，沿用已有 G）。** N18 不能结案 G60 启动门不可达，亦不能结案 G61 长 deadline/异常清理。close 的 browser/server/profile 在 client 或 browser 清理失败时可能被跳过，且性能命令没有所述进程外总期限；依据既有审查文档与静态实际控制流。未运行性能旅程，故不声称观察到具体浏览器失败，也不将它记为新 G。
3. 未发现本批文档记录的交易语义被候选迁移遗漏或跨层改变。对 G39、A 股法源及没有实际运行证据的性能结果均不作通过声明。

## 限制

本审计只读了源清单三份文件与用于核对的有关报告段落/当前 caller 和决策；不代表重新读取全仓或复验源报告中所有 128 候选。未运行测试、构建、性能脚本、外部市场规则查询。文件哈希和行数绑定上述精确源版本。批次状态：**complete（限制如上）**。
