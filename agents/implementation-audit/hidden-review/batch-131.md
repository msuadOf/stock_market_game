# 隐藏复核 batch 131（owner=1）

## 范围与来源

复核基线为 `.worktree/implementation-reaudit`，对应产品 caller commit `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。依 `scan-plan.json` batch 131 的三项冻结来源，逐篇连续读取至 EOF；精确路径、SHA-256、计划/实测行数和 `read_to_eof` 记录见配套 JSON。已阅读 worktree `AGENTS.md` 与 `docs/principles.md`。复核当前实现时只追踪来源提出的主要对象边界，并对照 G01–G68/Q 总账、当前生产调用方、相关 accepted ADR；没有改产品代码、运行测试/构建或执行 Git 写操作。

## 历史材料结论

- `final-review.md` 的范围是对当时 OOP 可行性报告及其两份证据材料的文档复核。它认可的是条件性架构建议和诚实披露，明确不能独立认证 1,272 文件覆盖、源码兼容性、性能收益或 A 股规则；没有批准对象迁移或宣称产品实现完成。
- `report.md` 将 Account/Position、Web 会话生命周期、决策只读上下文、候选事务状态等作为渐进式设计建议，并要求未来保留资金/股数单位、T+1、成交与委托意图区别、候选失败原子性和既有受理顺序。其正文声明只做静态阅读，建议并非可直接替换实现；未跑测试或官方规则复核。
- `summary-review.md` 只复核汇总文档忠实度，重复了未验证源码语义、未重新认证覆盖计数等限制。新增历史复核没有反过来构成生产契约。

## 当前调用链与反证

- Web 生命周期候选已有窄 owner：`apps/web/src/app/useSessionHostLifecycle.ts:19-35,218-232` 定义生命周期 ports 并由 hook 建立/释放 lifecycle；`App.tsx:287` 接入该 hook。存档替换流程仍在 `useSaveCommands.ts:95-143,161-240`，`handleLoad` 与 `handleLoadFile` 各自负责代次 gate、验证日终档、等待日终写入、调用 `host.load`、重置 UI，并显式报告错误。因此历史“可组合成统一 AppSessionController / 合并替换流程”仍是可能的候选，而非已完全实现或已确认缺口；这些分散流程是否值得合并需按当前调用契约单独决策。
- Account/Position 的旧边界建议已经显著落地：`packages/engine/src/account.rs:95-112` 的 `Account` 持有私有 `AccountState`，账户字段通过方法读取；`Position` 字段私有（`:609-618`），并保留明确恢复入口。不能再把“公开字段/可绕过修改”照抄为当前事实。总账 G49 是成本与浮盈跨层舍入差异，不因聚合封装存在而消失；应沿 G49 处理，不将旧 OOP 建议当作其修复证据。
- 决策个人状态/观察已有明确 owner/context 路线（`decision_chain.rs` 的 `RootReadContext`、`PlanPersonalState` 与 `InstitutionDecisionRoot::observe_personal` 等调用；当前调用点可见 `decision_chain.rs:3504-3518`）。旧报告提出的宽泛只读上下文不能直接等同于全部候选已合并，也不能扩大成新的 G；现行总账 Q02、Q11 的语义边界须独立保留。
- 候选事务镜像已收拢到 `CommittableSessionState`：`packages/engine/src/session.rs:1131-1135,1328-1341` 显示 shadow clone/commit 委托给该状态对象。仍需以总账中的具体行为事实判断缺口；“状态聚合已存在”不自动证明失败原子性、跨字段同步或所有 tick 行为均已验收。

## 总账、ADR 与判定

当前 `implementation-audit-2026-10-02.md:25-28` 记载 G01–G68 中 G27 已核销、其余 67 项仍有未完成部分；`:133-161` 将 Q01–Q23 的问题与待裁决边界分开。没有找到把这三份历史可行性/汇总复核材料映射为 G 项，或据其关闭 G/Q 的有效依据。G/Q 状态保持总账当前记录，不由 OOP 抽取自动核销，也不把历史候选追加成新产品缺口。

后续契约进一步约束候选的应用范围：ADR-0025 明确仅自然日日结产生持久存档并保留恢复所需个人状态；ADR-0026 明确机构个人经历及风险记忆边界，不改变 A 股交易制度；ADR-0019 保持单局开发范围并反对任意请求/挂单配额。它们没有把 OOP 报告提案升级为必须创建特定类的承诺。`docs/open-questions.md` 中 Q11 的机构行为方向已由 ADR-0026 补充，但当前总账仍区分生产分发缺口与尚待裁决细节；不可把旧材料或 ADR 的存在当作 Q11/G 项整体关闭。

结论：未发现这三份材料错误宣称 OOP 候选已实施/修复，亦未发现被其错误关闭的 G/Q。当前生产代码对若干历史建议已有实质演进；保留的设计可能性不等于需求或缺口。未发现新的交易语义判断，不提供交易规则合规认证。OOP 提取不等于修复；本复核不构成全量源码审计、测试通过或完整 G/Q 再验。

## 来源全文凭证

章节族状态：`final-review.md` 覆盖通过/问题/限制及阅读凭证；`report.md` 覆盖范围与方法、建议边界、组合/接口、保留现状及实施验证限制；`summary-review.md` 覆盖汇总结论与完整阅读凭证。三篇均从首行连续阅读到 EOF，实测行数和 SHA-256 与计划一致，详见配套 JSON。
