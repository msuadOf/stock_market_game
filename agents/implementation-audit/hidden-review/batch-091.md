# 批次 091：Engine foundation 与 pipeline 旧复核重审

## 基线与读取

- 计划：`scan-plan.json` batch 91，owner 1。三条主工作区来源路径、SHA-256 与行数均逐项匹配计划；依次 `cat` 从首行连续读至 EOF，实读 22、22、22 行。未用搜索片段替代全文读取。
- 读取约束：`.worktree/implementation-reaudit/AGENTS.md` 与 `docs/principles.md`。审计产品基线为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`；未改产品代码、未运行测试/构建、未做 Git 写操作或官方规则查询。
- 源文件及主题族：`engine-foundation-03.md`（中文化版本绑定记录）；`engine-foundation-04.md`（中文化版本绑定记录）；`engine-pipeline-01.md`（中文化及 lane patch 时点绑定记录）。来源本身不是原始模块语义审查；按其中链接另外读取了三份 canonical/original reviews，以及相关原始模块记录，避免将“译文通过”误当作当前代码审计。

## 当前调用与结论

- **foundation-03 / 市场、订单簿、Money、观察与 evidence 投影。** 旧 canonical review 的结论是归档与领域描述通过，但明确保留底层风险：订单簿后续撮合/索引失败可遗留部分状态、delta 与 filled 恢复不是事务、公开 last-price 写口可绕过正值约束；它没有声称调用方或恢复契约已复核。当前实现仍由 `Market` 拥有单股 `OrderBook`，session/pipeline 调用市场撮合，结算位于 session/account 边界；旧结论的对象归属和风险限界未见反证。当前代码位置：`packages/engine/src/market.rs`、`orderbook.rs`（由对应模块记录所列方法核对）；验证投影状态在 `verification_evidence.rs`。未将基础模块中潜在底层失败面提升成新的产品候选，亦未从 `Market`/`OrderBook` 的结构推导交易语义改变。
- **foundation-04 / phase timing 证据生命周期。** 原 unit-037 复核指出 test-only helper 将操作错误映射为 `NoCommittedTick`、生产路径保留 `StepFatal`，panic 时没有 unwind 清理。当前代码对应 `packages/engine/src/verification_evidence/phase_timing.rs:372-382,468-478`；与记录相符。公开 timing 入口仅附着已提交 tick 证据，不持有权威市场状态。无抽取或修复必要性；panic recovery 政策仍不是此处已确认需求。
- **pipeline-01 / A01 `AccountBudget` lane patch。** 原设计的唯一窄候选，是把快照装载、预算更新及 Cash/Shares lane patch 采用统一留在 `AccountBudget`，同时保留 `AccountValidationState` 对验证、调度和 round 提交的所有权。43b1aa5 当前实现已包含这些 API：`account_validation.rs:393` 开始准备 worker round；`:481`/`:488` 分别采用 cash/share lane；`:683-704` 为账户/可卖股份快照惰性装载；`:770-777` 应用预算更新；`:1110-1154` 定义预算对象及上述操作。`prepare_account_round` 仍在 disposable round-local staging 中合并，`:274` 提交点调用 `commit_round`，`:320` 才把成功 round 并入累计状态。故候选状态为**已实现，无需再抽取**。旧报告关于局部 staging 可先写而后因错误丢弃、整轮成功后才提交累计状态的时点判断，在当前调用链仍成立。

## G/Q、ADR 与取代关系

- G01–G68/Q01–Q23 对照当前总账 `agents/implementation-audit/implementation-audit-2026-10-02.md`、`coverage-index.md` 与 Engine/pipeline re-audit：三份来源未证明、实现或核销其中任何 G/Q。特别是 G16 是历史 `PlanBook` 复制工作量缺口，与 P3 账户预算 owner 无关；Q02/Q11 是公开历史经历记录与更正/违约 cause 分发边界，也与本批 lane patch 无关。保留总账状态，不增删编号。G27 原有核销亦不受本批影响。
- ADR-0018 对长期 immutable timeline 仍是 proposed；其已有局部冲突/受理规则约束预算 lane 的资源竞争语义，但不将整套历史仓储、COW 或恢复目标视为已接受。ADR-0019 禁止以任意请求数/挂单数配额替代性能优化。ADR-0017 的 escrow/结算语义与 `docs/trading-rules.md` 仍支配资金和股份的分离、保留与结算；本批只是既有 P3 预算对象封装，不构成 A 股制度变更。没有发现后续 ADR 明文废止 A01 的窄对象边界；但由于代码已实现，当前产品状态取代历史“待评估候选”的状态。其他旧 agent 操作指令只作为历史材料，不执行。
- OOP 对象抽取不能作为产品缺陷修复、G/Q 核销或性能改善证据。三篇报告的历史范围/身份绑定结论可保留；foundation-03 与 foundation-04 没有迁移候选，pipeline-01 的 A01 建议由当前实现接续为已实现状态。没有新的候选反证需要升级。

## 完整性与门禁

三篇指定来源全部 EOF；计划行数/哈希一致。已追到当前调用链与相关正式决策/总账。大 A 语义仅核对现行工程中的概念边界，没有重新查询交易所规则，也未主张规则改变。未运行测试或构建，故不提供运行时验证结论。
