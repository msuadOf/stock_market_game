# Session root 与续作协调迁移

## 范围与依据

本 worker 仅实施 `engine-session-03-A01`、`session-R2-N03/N08/N09/N11`，以及所负责 caller 的 `CommittableSessionState`、Account/Position、TradingPlan、ParentOrderPlan、BeliefParticipantState、SessionCandleBook、NpcAttentionScheduler 与 CausalCollector 接口对接。`personal_state.rs` 与计划/执行 owner 的实现归其他 worker。

依据为 completeness/challenge 两份 `action-index.md` 的完整动作正文、既有源码以及 ADR-0016、0017、0018、0021、0025、0026。A 股语义沿用 `docs/trading-rules.md`/ADR-0021 已登记依据，不新增交易制度假设。

## 已落盘的责任边界

- `DecisionChainObservation` 一次封存 MarketView、价格路径、TechnicalObservation、CivilInstant 与曝光集合；校验 tick/minute/phase 与账户集合仍在 Session 捕获适配器，按 StockCode 保留技术结果与错误次序。
- `RootReadContext` 仅持有 COW AccountBook、只读公司注册/经营/public library/PlanBook 与时刻标量；没有 GameSession、市场簿、历史、个人 map、OrderId/seq 游标或可写 Session API。
- `InstitutionDecisionRoot` 仅持单账户 PlanPersonalState，在共享 context 上观察本人资源、读取公开信息、更新个人认识并输出 Lifecycle assessment/AccountExecution typed 增项及诊断。root 不生成 PlanLifecycleAction，不路由。
- `PlanRootCoordinator` 拥有 AccountSource 与完成通知，保留封存 context 后再 take personal、成功发布结果后通知、单 Rayon worker 的 yield 路径；个人状态安装后由外层 batch 汇总操作。
- `PlanLifecycleReview` 仅在 adaptive coordinator 的固定观察边界内构造，拥有 equity/held 派生事实并只读借用当前 candidate 的 plan/parent/belief。`assess_one` 保留股票/动作次序，零权益仍先返回，动作应用留在既有适配器。
- `StockRouteCoordination` 单独持有 pending、reconsideration、retry_market 三 map，保留独立生命周期、generation 检查、reconsideration 优先且不清掉 retry。错误 location 仍为 `plan_chain_candidates::adaptive`。
- `CandidateTargetProposal` 复用 weight → 股本/u32 表达性收缩 → 100 股整手换算顺序，保存收缩后 raw_qty/rounding；它只是意向目标，不表示可负担申报或成交。

没有改动 ±2000bp 方向门槛、个人风险暂停/恢复、Money 分/数量股、T+1、零股全部卖出、涨跌幅、价格笼子或真实费用。root 的固定账户观察与反馈后计划/母单事实仍分阶段读取。P9 权威提交、日终存档和失败时丢弃私有 candidate 的边界保留。

## 测试与静态核对

本 worker 按任务约束没有运行 cargo/产品测试或 Git 写操作；仅对所属 Rust 文件执行精准 rustfmt（`skip_children=true`）。测试执行与完整 diff 独立审查由 root 协调，结果须追加后才能声明全批完成。

新增定向过滤器：

- `proposal_`：非整手股本上限先收缩再取整，收缩后 raw_qty，全现金/恰整手，错误顺序。
- `root_coordinator_`：空 root 不取个人状态，worker 断开显式 fatal，worker 启动后不能变更通知 sender。
- `stock_route_`：reconsideration/retry 独立消费，错误 generation 不消费 pending。
- `root_observation_rejects_tick_minute_and_phase_clock_mismatches`：三个 P1 时钟不匹配均显式 fatal。
- `shared_root_context_keeps_own_facts_when_account_execution_order_changes`：两机构正/反 root 顺序得到相同个人结果，live 账户资源修改不进入封存的 COW 观察。
- `lifecycle_uses_fixed_account_resources_and_current_plan_after_outcome`：current 账户资源归零时仍复核 fixed 资源；当前 PlanBook 已终止时不复核旧计划；观察交换后 live 零资源保留。

保留已有 `plan_root_owns_its_personal_state_until_the_private_candidate_installs_it`、`live_plan_without_position_or_belief_still_enters_root_observation`、`generation_preserves_pending_plan_facts_when_a_child_fill_is_waiting`、`filled_child_cancel_failure_reconsiders_the_live_plan`、整手/零股/费用及机构风险暂停场景。单 worker 和成功完成通知已有 `pipeline/ready_ingress_tests.rs::real_root_notifies_before_stock_stream_starts_with_one_or_several_workers` 覆盖。

## 复核修正

`review_roots` 指出 Coordinator 提取时遗留了一条不存在的 `self.operations` append，现已删除，仅 batch 负责汇总。另自查修正 route outcome fatal location，避免 owner 提取改变错误 payload。编译 02 的 ParentOrderPlan 私有字段 caller、CandleBook/Scheduler 接口、借用 StockCode 断言等已按实际诊断逐项修复；quote/lifecycle/urgency 的最终 getter 对接由 root 指派的 `roots_callers` 独占处理，避免共享写入。

独立复核 R02 进一步指出 `RootReadContext::capture` 不应新增 `AccountBook::clone_for_shadow` 全账户策略校验；现恢复原 `clone_for_plan_roots` 的 `accounts.clone()` COW 读取，保留调用签名、不改变错误接受集合，未改既有 `FrozenPlanChainObservation::capture` 校验。`chain_observation_instant` 与 batch `accounts` 仍有测试 caller，仅以 `cfg(test)` 收窄到测试编译，不用 allow 掩盖生产 dead code。

当前状态：源码落盘并再次冻结，等待 root 最终编译/定向短测及独立复核结论。
