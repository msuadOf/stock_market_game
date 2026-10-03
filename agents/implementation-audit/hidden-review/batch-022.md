# 批次 022：全文核验记录

## 基线与范围

- 产品基线：`43b1aa5f25226c72976ca172d32f4a8eeb2272ad`，即计划所记 `43b1aa5`；现行代码位于 `.worktree/implementation-reaudit`。扫描源来自主工作区相对路径。
- 依据：主仓 `AGENTS.md` 与 `docs/principles.md` 已读。采用“生产 caller/owner/consumer 证据优先，最新 ADR 与退役决定优先；对象抽取或未运行测试不自动构成缺口”的口径。
- 文件逐行连续读取至 EOF；各自 `wc -l` 与 SHA-256 均匹配扫描计划。全文原文是短报告，不是源码文件。三个 JSON 附件 `03.json`、`04.json`、`05.json` 不在本批清单，未把其内容冒充为已读材料。

## 来源章节矩阵

| 来源（真实行数；SHA-256） | 全文章节/范围 | 复核与现行证据 |
|---|---|---|
| `agents/oop-refactor-audit/completeness-2026-10-03/session/parts/03.md`（11；`4e6d3905d8d2e4cec0a34b89f57ed575b5b28f319b0a18c5239802a16171292d`） | 1–3：五文件范围、OOP/state owner 裁定、A05/A07/U025 与改善项；4：逐文件及验证声明。 | 结论为不再增加持久聚合；适配器只拥有 ID 镜像、公司装配只负责新局、撤单协调既有状态。当前产品已拆分观察/单账户 root，但这些原 owner 边界仍成立。撤单改善项由来源指向未随批扫描的 `03.json`，本报告不臆测其具体测试。 |
| `agents/oop-refactor-audit/completeness-2026-10-03/session/parts/04.md`（37；`9b39d4ca78ddddd917a6ea86dbdd23bf936cda4296bebd10177a97870b4a57e5`） | 1–2：核销对象及既有类型；3–5：建议聚合对象、迁移边界与跨层契约；6：迁移步骤/验证边界。 | 核心是 `DecisionChainObservation`、窄 `RootReadContext`、单账户 `InstitutionDecisionRoot`；明确 PlanBook/订单/报价/lifecycle 属后续协调边界。来源报告声明未跑测试，不能当验证通过。 |
| `agents/oop-refactor-audit/completeness-2026-10-03/session/parts/05.md`（37；`4de8fc1a1edf4400eaff94231b42ad2de627925bd360558656bf87a46d15e5df`） | 1：结论与需收紧处；2：源码清单/历史审阅范围；3：六项逐项复核；4：对象职责与迁移次序；末段交易语义/验证约束。 | 收紧 RootReadContext 字段须由真实读取清单决定，不能把 `urgency_policy` 或后续 lifecycle/quote 资源塞进 root。其余条目反证支持：账户私有状态唯一 owner、共享观察代替散列字段、typed outcome 后重读 lifecycle、诊断发布留在 coordinator、纯算法/DTO/fixture 保持值职责。 |

## 最新生产 caller / owner / consumer 核对

- `packages/engine/src/session/decision_chain.rs:63` 定义一次共享观察（market、paths、technical、CivilInstant、exposed）；`:681-684` 的捕获入口在账户筛选后构造该对象。`packages/engine/src/session/plan_chain_candidates.rs:304-326` 只捕获一次 `RootReadContext` 与 observation，再把它们通过 `Arc` 共享给真实 Rayon root worker，`:371-378` 接收结果、安装个人状态并发布诊断。类型并非悬空提案。
- `packages/engine/src/session/decision_chain/roots.rs:6-29` 定义并捕获 root 只读上下文；`:188-221` 的 `InstitutionDecisionRoot::observe` 接收单账户 `PlanPersonalState`，返回该状态、typed operation batch 与 diagnostics。`:237-250` 展示账户持仓/现金消费。它没有可写 `GameSession`、市场簿或 OrderId/seq 所有权。
- `packages/engine/src/session/decision_chain.rs:754-770` 由 GameSession coordinator 记录 diagnostics；`plan_chain_candidates.rs:374-378` 是安装后的生产消费者。故诊断 DTO 与状态写入边界没有被 root 合并。
- `packages/engine/src/session/decision_chain.rs:840-848` 仍经 `PlanLifecycleReview::capture(...).collect(...)` 从当前输入组装动作；`decision_chain/lifecycle.rs:60-72,112-125` 表明 lifecycle 单独评估当前计划/账户事实。根观察不缓存 lifecycle command；迁移未造成额外 owner。
- A 股语义：相关记录声明未变更股数/资金/T+1/阶段边界；现行 `decision_chain.rs:962-978` 对 sell request 保留 `ExistingPlan` 分类，买请求 `:1232-1242` 亦为 `ExistingPlan`。本批未核验官方规则，沿用正式 ADR 与 `docs/trading-rules.md`；不把策略预算归类当成交易所撮合优先级。

## G/Q 关联、新候选与反证

- **G16 保留，非本批 OOP 项目遗漏。** 现行 `roots.rs:25` 仍 `session.state.plans.clone()`；`plan_chain_candidates.rs:304` 每个 root 批次 capture 一次，Arc 只让 workers 共享副本。最新总账 `implementation-audit-2026-10-02.md:75` 与裁定 `exhaustive-review/resolution.md:13` 已将 PlanBook 及其他历史全量复制列入同一所有权/复制目标；本批 root 抽取不能核销它，也不足以新开重复 G 项。没有性能测量，不声称幅度。
- **G38 保留，非本批边界缺失。** 两类生产 request 仍在 `decision_chain.rs:972,1236` 标 `AllocationClass::ExistingPlan`。当前 OOP 划分不提供新机会分类输入，也不应把它扩大为跨账户撮合优先级；总账 `implementation-audit-2026-10-02.md:117` 已登记。
- **Q 关联：** 本批材料没有引入新的未裁决交易语义。潜在关联仅限现行总账中与 root 观察/经历留痕相邻的 Q02（总账 `:140`）：它讨论 `PersonalPriceMemory::record_public_history_read` 的生产消费边界，不因 root 被对象化而关闭。本批不是历史读取接线审计；没有证据把它升级为新 G。
- **新候选：** 未发现独立于已登记 G16/G38/Q02 的新缺口。批次 03 的连续撤单测试建议仍是测试覆盖改善候选，具体测试矩阵因 `03.json` 未列入本批而未核；不擅自升格为产品缺陷。
- **反证边界：** 现行 callers 证明对象被实际消费，不证明行为测试本轮通过；来源 04/05 明确写未运行测试/构建。历史“OOP 无需新对象”与现行已抽取对象不冲突：后续实施采用了清晰的短生命周期/只读对象，未引入持久重复 authority。

## EOF 与未核实点

| 来源 | EOF 证明 |
|---|---|
| `.../session/parts/03.md` | 逐行读完第 1–11 行，末行为“未运行测试/构建”；`wc -l`=11，计划/实测 SHA 一致。 |
| `.../session/parts/04.md` | 逐行读完第 1–37 行，末段为迁移步骤第 5 项；`wc -l`=37，计划/实测 SHA 一致。 |
| `.../session/parts/05.md` | 逐行读完第 1–37 行，末段为不得改交易语义与“不运行测试/构建”说明；`wc -l`=37，计划/实测 SHA 一致。 |

未核验：三个来源各自引用但不属于清单的 JSON 明细；官方现行交易所法源；运行测试/构建；性能幅度；03 记录所指撤单测试细项。历史旧 source 对当前 API 的 SHA 不能代替现行源码 SHA，本次现行判断以指定产品工作树 caller 行号为准。
