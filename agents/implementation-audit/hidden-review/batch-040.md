# 独立审读：批次 040（owner 5）

## 基线与覆盖

- 目标 worktree：`.worktree/implementation-reaudit`，HEAD `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`（2026-10-03）。仅在指定 `hidden-review` 目录写本记录；未改产品代码、未运行测试/构建，未执行 Git 写操作。
- 规范依据：根 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`；相关决定全文核对 ADR-0017、0018、0019、0023–0028。交易与并发约束沿用已接受决定；ADR-0018 整体仍为 proposed，只采用被后续接受的明确段落，不把提议部分升级为要求。
- 三篇材料均逐篇连续读取至 EOF，核对行数与 SHA-256：`engine-session-01.md` 82 行，`fa40343d0f8474db0390fc639ed4c7ba35d83d3ad587dc3a168d220b9472f9dc`；`engine-session-02.md` 94 行，`3be9a25efbc40b8c1d8dfdd592ceeee49b8d53e9e32621570162946ef933cbb2`；`engine-session-03.md` 70 行，`b562d61f0e7971494773b77d1614a178af49828b7f0e8a84d8c85d402100fe40`。三份均标注“候选设计，未实施”。
- 对照当前基线的代码 owner、调用方和消费者，不把材料内的历史“未实施”状态继承为 baseline 状态；G/Q 映射仅用于确认关联和反证，不声称重审实现审计全账。

## 主要结论

1. **三项主要对象候选在 baseline 已落地。** `CommittableSessionState` 位于 `session.rs:1125-1160`，由 `clone_for_tick_shadow` / `commit_tick_shadow` 调用 `clone_for_shadow` / `commit_from`（`session.rs:1328-1340`）；`SessionCandleBook` 聚合历史与活动 candle（`candles.rs:90-205`）；`DecisionChainObservation`、`RootReadContext`、`InstitutionDecisionRoot` 已分别在 `decision_chain.rs:63-87`、`decision_chain/roots.rs:6-29,188+` 实现。故这三篇调查稿作为实施期候选记录有价值，但不能再作为 baseline 的待实施建议或“尚无 owner”证据。
2. **调用边界与关键业务契约保持在现有 owner。** `PlanRootCoordinator` 捕获一个共享 `Arc<RootReadContext>`，把每个账户的 `PlanPersonalState` 交给 `InstitutionDecisionRoot`，并以 typed 结果回收（`plan_chain_candidates.rs:294-335`）；生命周期动作仍在 adaptive coordinator 消费路由结果后从当前 candidate 重新收集并应用（`plan_chain_candidates/adaptive.rs:82-180`）。`GameSession` 仍是 tick candidate 与日结协调 owner。未发现重构对象把资金、股票受理顺序、拍卖、费用、T+1 或存档契约改成新语义的证据。
3. **session-02 的两个局部失败线索仍有源码反证。** `CompanyOperationsClockWiring::sync` 先执行 `mirrored.insert` 再 `clock.register_due(...)?`（`company_operations.rs:60-73`），一批中后项失败会留下 partial clock/mirror 状态；这是明确成立的局部失败原子性风险。`CivilClock::register_due` 仍以 `self.next_due_seq += 1` 推进游标（`civil_clock.rs:330-351`），耗尽可能 panic/wrap；但“MAX 保留为哨兵、拒绝分配 MAX”是新边界政策，不能仅由现行 ADR 推导为已定规则。`end_day` 从 `pending` 移除当天 due 后再查询 `next_status`（`civil_clock.rs:431-449`），后一步错误不会自动恢复移除项，报告中限定此边界正确。
4. **G/Q 交叉核对没有被 OOP 候选核销。** 实现总账 G16 明确指出 `RootReadContext::capture` 仍克隆完整 `PlanBook`，普通 shadow 还复制历史/报告版本；当前 `roots.rs:18-29` 确认这项性能/所有权问题仍在。新建 `RootReadContext` 不等于解决 G16。实现总账中的 Q02（本人主动读取行情经历留痕）、Q11（更正公告/违约专门分发）均不是这些 OOP 抽取本身的结果；不可用对象存在与否代替消费链审计。这里的实现总账编号 Q11 不应与 `docs/open-questions.md` 中产品策略 Q11 混为一谈；后者的机构个人补充由 ADR-0026 定向。
5. **大 A 语义审查。** 本批只是历史模块设计与当前内部 owner 对照，没有引入交易规则结论。金额以分、数量以股、并发局部受理与失败候选丢弃继续服从 ADR-0017/0018 的已接受条款；公司个人风险仍是 ADR-0026 标明的游戏假设。无交易所规则新增或变更。

## 分文件审读

### engine-session-01

- 主候选 A01 已对应当前唯一的 `GameSession.state: CommittableSessionState`；poison/test failure hooks 仍在 `GameSession`，candle、账户、个人状态等权威字段集中在内部状态结构。不是尚待实施的独立候选。
- 文档建议的 `clone_for_plan_roots` 已不在该 baseline；当前 root 输入改由 `RootReadContext::capture` 显式复制所需事实（`decision_chain/roots.rs:18-29`），caller 在 `PlanRootCoordinator::start_ready_accounts` 使用它（`plan_chain_candidates.rs:294-335`）。审阅时应把这段视为已落地迁移，而非要求再建同义对象。
- `AccountBook` 继续是 `CommittableSessionState.accounts` 的组合 owner；未见把页/组次序当交易优先级的变更。
- 分类：候选对象保留为已实施结构记录；当前没有可执行的 OOP action。原文“候选设计，未实施”是编写时状态，若转为当前计划需更新状态说明。

### engine-session-02

- `SessionCandleBook` 已拥有 `histories` 与 `active`，`record_trade_or_mark` 保留零量昨收占位到第一笔真实成交替换的行为并维护统计（`candles.rs:90-189`）；`GameSession` 边界方法委托给该对象（`:191-205`）。snapshot/save 投影仍分别导出 `daily_candles` 与 `active_daily_candles`（`snapshot.rs:215-227`、`session.rs:2583-2590`）。候选 A03 已实施。
- `CompanyOperationsClockWiring::sync` 的 defect lead（A07）仍成立，且不能只按单条先注册再记录 mirror 修：一批前项成功、后项失败仍会部分改变 `clock`。候选记录提出全批候选和双边提交，修复前需要验证 observer/状态复制语义、错误原样返回、成功数量及 retry 无漏重。
- 序号耗尽线索真实存在，但具体错误类型和 MAX 哨兵政策未决；应报告为边界风险/政策候选，而非声称该分配政策已由 ADR 敲定。
- `CivilClock::end_day` 在取走到期 due 后才执行可能失败的 `calendar.day_status(next)`，因此文档对“不是任何 Err 均零修改”的限定准确。外层 `GameSession` checkpoint 不自动证明可独立调用的时钟 API 局部原子。
- 其他 OOP 处置（分页 map、注意力、causal facade、撤单 helper、公司装配）整体遵循 owner 边界；模块内另记的 `AccountPagedMap` callback 失败部分修改接收者，属于独立风险，不由新对象抽取解决。

### engine-session-03

- 只读共享 `DecisionChainObservation` 和只读 root 输入 `RootReadContext` 已实现；临时账户工作为 `InstitutionDecisionRoot`，输出仍返回既有 typed operation batch，不持久化为另一个业务状态副本。
- 调用端 `PlanRootCoordinator` 启动 root 后保留异步结果/个人状态归并；`adaptive.rs` 通过对应 `(account, code)` 的 pending/unfinished 路由状态分割 ready 项，消费真实 typed 结果后从当前 candidate 收集生命周期动作并应用（`adaptive.rs:82-180`）。候选设计对“不在 root 冻结 `PlanLifecycleAction`”的边界是重要契约，当前调用结构与之吻合。
- 当前观察仍做完整技术历史计算；candidate 类型抽取本身不证明 G16 中历史克隆/完整工作量问题已解决。`RootReadContext::capture` 复制 `PlanBook`，而 `DecisionChainObservation::build_technical` 遍历 candle history（`decision_chain.rs:88-105`）。不得把封装存在误报为性能收益。
- 分类：主候选已实施，边界说明可作复核依据；未发现需新增同名 owner 的有效 OOP 动作。

## G/Q 与限制

- 对照 `implementation-audit-2026-10-02.md` G01–G68、Q01–Q23：本批对象抽取不核销任何 G 或 Q。直接相关的 G16 仍是独立的历史复制/工作量缺口；Q02/Q11 也需要各自沿生产消费链评估。
- 实现总账文档记录的源码审计基线不是本次 `43b1aa5`；因此这里只核对其编号、声明与明确关联项，不把其余 G/Q 状态冒称为本次 baseline 全量源码重验。
- 未查询交易所/中国结算官方资料；本批没有交易制度主张，因此不需要以重构稿为由引入新的大 A 规则。

## 审读结果

- 发现：候选稿对 baseline 的实施状态已过时；应更新实施状态/引用，避免同名对象被误提议二次创建。
- 仍需独立跟进：`CompanyOperationsClockWiring::sync` 的失败原子性；`CivilClock` 序号溢出及 `end_day` 局部失败边界；G16 完整历史复制。前两者分别是行为修复与待定耗尽政策，不应归为 OOP 抽取。
- 审读覆盖：3 篇指定材料全文、当前 baseline 主要 owner/caller/consumer、相关 ADR 与 G/Q 总账关联。没有进行运行时验证、完整 G01–G68 再审或产品验收。
