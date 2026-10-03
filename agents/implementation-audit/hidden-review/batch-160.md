# 隐藏扫描批次 160（owner 5）

## 来源完整性

- 按 scan-plan 指定三篇来源，逐段连续读取至 EOF；计划路径、SHA-256、行数均与实测相符，共 309 行。逐篇指纹见配套 `batch-160.json`。
- 来源基线与当前 `HEAD` 均为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。按要求读取 `AGENTS.md`、`docs/principles.md`、相关 ADR、`docs/trading-rules.md`、`docs/open-questions.md` 及 implementation audit G/Q 总账。
- 本轮只静态核对指定历史材料与当前 owner/caller/consumer；未运行测试、构建或外网规则检索，未改产品代码、G/Q 总账或 Git 状态。

## 历史材料与现行代码

- `engine-session-02.md` 的 `SessionCandleBook` 提案已成为实际 owner：`SessionState.candle_book` 持有历史与活动日 K；`GameSession::update_active_daily_candle`、`commit_active_daily_candles` 接到撮合/日界路径。SaveSlot/snapshot 仍把 `daily_candles` 和 `active_daily_candles` 分别投影及恢复，未把两字段合并成存档迁移。AccountPagedMap、NPC attention、causal facade、公司装配、撤单等保留边界与当前模块职责大体相符。
- 同篇指出的 `CivilClock::register_due` 序号耗尽边界仍存在：`next_due_seq` 为 `u32`，当前先以其构造 ID，再用 `+= 1`；游标到 `u32::MAX` 时 debug 构建会溢出 panic，release 构建会回绕。当前没有耗尽专用错误或递增前检查。需先裁决 MAX 是保留哨兵还是可分配值，再以显式错误和状态不变测试固定契约；历史建议的保留哨兵方案是候选决策，不能当作已批准规则。
- `CompanyOperationsClockWiring::sync` 的整批失败原子性缺口也仍在：循环先将 scheduler id 插入权威 `mirrored`，再调用可能失败的 `clock.register_due`。单项失败会留下镜像；多项时，前项 clock 注册和镜像可在后项失败时部分保留，后续 sync 可能因已镜像而跳过缺失 due。生产 init caller 是 `GameSession::new` → `install`；日终 caller 是 `GameSession::end_civil_day` → `run_day_end`，外层 checkpoint 会回滚完整日终失败，但这不提供 `sync` / `install` 自身的局部原子性。需要的修复边界是候选 clock 与 mirrored 整批准备、全成功后同时提交，且保留原错误透传。
- 另两项 `CivilClock` 局部边界仍如来源所述：`end_day` 在 `extract_if` 移除到期 due 后，才执行可失败的 `calendar.day_status(next)?`；因此直接调用方法时该错误可留下 pending 被移除而日期未推进。外层 `GameSession::end_civil_day` checkpoint 会回滚此生产入口，不应泛化成 `end_day` 本地零变更。`from_parts` 检查每个 due ID 小于 cursor 与 due 日期，却未拒绝 cursor 为 0 或重复 ID；恢复完整路径可能另有交叉验证，本轮仅记录 `CivilClock::from_parts` 的局部校验事实。
- `engine-session-03.md` 所建议的 `DecisionChainObservation`、窄输入 `RootReadContext`、账户短生命周期 `InstitutionDecisionRoot` 已在 `decision_chain.rs` / `decision_chain/roots.rs` 实现；生产 caller 在 `plan_chain_candidates.rs`。`adaptive.rs` 按 `(account, code)` 保留 pending/unfinished 路线，处理精确 typed `PlanRouteOutcome` 后才对 ready assessments 重收集并应用 `PlanLifecycleAction`。候选仍只在现有 GameSession private candidate 中更新。来源的生命周期阶段主张在当前代码中有实证，不等于整条交易管线的全局验收。
- `engine-session-04.md` 中多数对象/边界是现存设计的保留说明，不构成待实施 OOP 工作。当前 `PlanPersonalState`、报价与 urgency 会话方法、`DisclosureDispatch`、envelope projection、`ParentOrderPlan`、`ReconciliationPlan`、母单 records、hash/snapshot DTO、`observation_civil_instant` 均有对应 owner。历史中指出 disclosures 顶部旧注释与活跃日终封账调用不一致，仍可作为文档准确性线索；本轮未据此修改。

## 语义、G/Q 与 ADR

- 本批未实现或修改交易规则。涉及撮合、零股、T+1、费用、阶段和单位的历史约束按当前 `docs/trading-rules.md`、ADR-0011、ADR-0014、ADR-0017 及 accepted ADR-0019 的范围复核；ADR-0018 标为 proposed，不能把未接受条款当作新授权。存档边界另遵 ADR-0025；个人策略参数依 ADR-0026，属于游戏假设而非交易所规则。本轮没有重新核验官方法源。
- 在 `implementation-audit-2026-10-02.md` 的 G01–G63、Q01–Q23 中，未找到对 `CivilClock` due 序号耗尽、`sync` 整批失败原子性、`end_day` 方法本地错误回滚或 `from_parts` 重复 ID 检查的直接条目。G41（经营付款失败状态）和 Q13（混合交易所日历政策）与这些局部问题不同，不据领域关键词强行对应。序号耗尽与 sync 部分提交是值得总账负责人评估的新行为缺口；本批不擅自分配 G/Q 编号。局部 end_day/from_parts 观察暂列限制性证据，尚未确定为公共产品承诺或独立总账项。
- 旧材料明确标出的新 OOP owner 候选已实现或应保留现有边界；无须再登记重复 OOP action。`SessionCandleBook` 实现保留活动 K/历史 owner 与存档双字段；不声称本批运行行为验证。

## 结论与限制

- **有效发现：** due 序号递增可 panic/回绕，`CompanyOperationsClockWiring::sync` 有写入在错误前、且跨条目部分提交的缺口。两项与其 A 股语义无直接规则冲突，但都影响调度事实一致性，应由总账负责人决定编号、修复和定向测试范围。
- **现状反证：** 历史的 candle owner 与 decision root/observation 设计现在已有生产实现；不能把旧“候选、未实施”标题转述为现状。
- 未运行测试或构建，未核验长流程/动态恢复，也未做完整 session / company-operations 审计；以上不是全量验收结论。
