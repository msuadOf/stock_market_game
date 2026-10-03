# 批次 161 隐藏来源复核

## 范围与来源

- 审计基线：`43b1aa5`；目标为三个旧版 `engine-session` OOP 调查记录。按 scan-plan 核对精确路径、SHA-256、行数；三篇从主工作区连续读取至 EOF。因工具输出上限，分段读取第 06 篇并逐段覆盖全篇，不以摘要或搜索片段代读。
- 已读取当前 worktree 的 `AGENTS.md` 与 `docs/principles.md`。复核相关 accepted ADR-0017、ADR-0025；ADR-0018 仍为 proposed，但其中已明确接受的并发到达语义及此前裁决替换须按现行条文理解，不把整个提案当新授权。
- 不修改产品代码、Git 状态或测试，不运行测试/构建；本任务只记录历史结论的现行核对与边界。

## 来源全文结构

| 来源（行数 / SHA-256） | 章节族与复核范围 | EOF |
|---|---|---|
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/modules/engine-session-05.md`（61 / `21c57fd6ab743315c5034f7a99f5138bf8e013fd1e0bca929411e8a0df4a1e24`） | L1–3：存档恢复/计划候选范围与不实施结论；L5–16：基础存档校验、v2 DTO、测试覆盖；L18–26：候选计划链 coordinator、调度及测试；L28–31：文件关系、ADR 与边界。 | 已读至 EOF；实测 61 行。 |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/modules/engine-session-06.md`（195 / `a21eefe4d946eea4e4db5071556aeb64cf2bbd08ceaa4846914069bd95e83cca`） | L1–13：PlanChain/冻结观察与 typed continuation；L15–40：ProtocolSession checkpoint、CivilUpdate、迁移与验证方案；L42–74：逐文件 retained/support 清单；L76–79：单位及交易语义边界。 | 已读至 EOF；实测 195 行。 |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/modules/engine-session-07.md`（105 / `a22e2747aa1377afe70b71bdf4b40aa92823ed72235827037b36838430919d81`） | L1–10：协议提交、delta、facts、wire DTO；L12–32：订单查询、replay、计划测试、snapshot/views；L34–36：调用与验证边界。 | 已读至 EOF；实测 105 行。 |

## 当前调用链与结论

- **存档所有权仍成立。** `GameSession::save` 只要求会话健康、调用 `capture_runtime_v2` 并制作 SaveSlot（`packages/engine/src/session/failure.rs:85-89`）；`restore` 在函数局部创建并恢复 session、验证订单及其余状态（`packages/engine/src/session.rs:2716-2721,2765-2793`）。v2 恢复先做验证、ledger/receipt/strategy 转换与影子账户构建，最后才写回四组权威状态（`packages/engine/src/session/persistence/v2.rs:370-443`）。未发现需要新增 validator/staging owner 的依据。
- **引擎捕获与公共持久化政策必须分开。** `ProtocolSession::save` 返回最近完整日终档，没有候选时显式错误（`protocol/civil/session.rs:175-184`）；`end_civil_day_update` 以 checkpoint 包住日结、事实附加、校验和候选准备，成功后才发布候选（`:283-329`）。ADR-0025:12-33 限定持久化仅来自成功完整自然日日结，并禁止把普通快照、检查点或活动委托查询当作用户存档。旧材料对此边界的区分再证。
- **计划候选是 tick 内临时协调，不是全局账本。** 当前 `PlanChainOperationBatch` 组合 roots、FIFO operations、candidate source、局部 routes 与 reports（`plan_chain_candidates.rs:90-96`）；生产消费者从 decision chain 进入 `prepare_ready_accounts`，已完成根的 operations 被 append 到 coordinator（`decision_chain.rs:687-729`、`plan_chain_candidates.rs:342-383,452-463`）。generation index 通过 checked-add 为实际产生命令编号，身份不成为交易优先级（`:31-76`）。adaptive continuation 以账户、股票、generation index 关联 typed outcome（`adaptive.rs:105-112,339-355`）。现有 owner 足够，不提出 `InFlightPlanRoots` 或独立账本对象。
- **两处旧文字需要按现状收窄。** 第 05 篇将 generation 分配描述为“不由 worker 完成回调直接分配”且候选身份不是优先级，此点与现行实现/ADR-0017:39 相符；但实际 worker 收集/operations append 顺序受结果回收时机影响，可能改变哪条 route 到达受理入口在先，不能扩写为“spawn/poll 不影响同股接受顺序”。ADR-0018:755-780 明确独立并发请求不保证跨运行同序。第 06 篇记载裸 `GameSession::end_civil_day_update` 后续可失败项没有本地 checkpoint；当前仍是 `ProtocolSession` 对外入口 checkpoint/rollback 保证发布事务（`:283-329`），不应把此保证泛化为裸 GameSession API 自身原子。
- **G/Q 对照。** 对照审计总账 G01–G68/Q01–Q23：本批 OOP 归属没有直接对应的新 G；相邻 G16 是历史复制性能缺口，不能由本次候选/存档结构结论核销；G39 是验收工具跨 worker 等值契约，和生产候选对象归属不同。Q17、Q22、Q23 分别涉及公司更正原子边界、受理轨迹证据、年度税务调用语义，均不由本批 OOP 提取产生或解决。其余 G/Q 仍以当前总账具体定义为准，不从“无新候选”推断核销。
- **大 A 语义。** 本批无交易制度实现或规则变更。金额分、数量股、T+1、资源冻结及真实订单簿受理保持原边界；generation index、event seq 和 DTO canonical 顺序仅作身份/协议游标，不能转成跨实体优先级。ADR-0017:32-45、130 与 ADR-0025:30-33 及现行代码相互吻合；没有新增交易规则，也未重新联网认证法源。

## 复核结果

三个来源均支持“保留现状，不因 OOP 提取创建新 owner”的结论；无新 OOP 候选、无新 G。关键旧结论经当前 caller 与后续 ADR 再证；候选调度顺序及 CivilUpdate 原子性按上述范围明确，避免旧报告被过度概括。没有运行测试、构建或动态验证，因此不对行为测试通过作声明。
