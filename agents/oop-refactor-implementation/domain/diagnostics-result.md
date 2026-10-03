# diagnostics 簇实施记录

复核基线：`b89afb3346743a4b4fccf26c9ac9ff108595f696`。动作权威正文为
[challenge action-index](../../oop-refactor-audit/challenge-2026-10-03/action-index.md)。

本记录对应 domain-R2-N30/N31/N32/N39/N41。实现者仅修改下列六个已有源码文件和本记录；
未修改 Session caller；N41 的 caller 已由 Session 实施者接入并完成静态复查。未运行 Cargo、
产品测试、Git 写操作，未进行全仓格式化。

## 阅读与语义边界

已读 AGENTS.md、docs/principles.md、docs/testing.md、docs/architecture.md、docs/open-questions.md、
ADR-0002、ADR-0011、ADR-0017、ADR-0019、ADR-0026，以及 docs/trading-rules.md；已全文核对
本簇 diagnostics 源码、phase_timing 与测试及 N41 对应 caller 片段。

这批改动只收束非权威诊断采集、派生报告和 opt-in 阶段证据，没有新增交易制度或修改现行
沪深规则。现行官方来源与已有复核日期继续以 docs/trading-rules.md 记载为准；本轮未重新
访问官方规则，不能将本次源码核验说成新的官方制度核验。Money/gross/value 为分，qty 为股；
目标仓位、可执行目标、真实委托、成交和账户资产保持分开。maker/taker 双边参与量维持市场
单边量的两倍。事实序号仅表示诊断事实流顺序，未赋予跨实体交易优先级或因果解释。

## domain-R2-N30

`RetailOrderLedger` 独占 seed、RetailExecutionRunReport 和订单 outcome map；真实 runner
通过 SeedDiagnostics 委托 `record`，结束时 consuming `finish` 输出原报告。
移除原 record_retail_order_event/finalize_retail_execution 散落写口。

保留 Submitted 的计数后插入/重复错误、Filled 的逐单先写后检查、Canceled/Aborted 数量检查
与报告累计次序。Aborted 仍累入 outcome.canceled_qty 参与释放守恒，但报告只计 aborted_shares。
未改变报告 DTO/序列化或目标与真实执行的含义。

先在旧函数入口写行为保护，再随迁移改为 receiver；覆盖多次部分成交后撤销、中止与未结
分别统计，以及未知 Filled/Canceled/Aborted 与 overfill 的部分写入。没有运行并确认红/绿，
这些是原行为保护测试，不能声称新增业务 TDD 红绿循环已通过。

测试 filter：`diagnostics::tests::retail_ledger_`（2 个）；default features 可用。

## domain-R2-N31

`SeedDiagnostics` 拥有单 seed 计数、retail 行为与 RetailOrderLedger、参与者 profile 及
ParticipantExecutionAccumulator。`StockRunDiagnostics` 将原四份同键 map 合并为每股一份
candles/Trade 股数/Trade 分金额/MarketDiagnosticsAccumulator。

真实 run_one_seed 仅驱动 GameSession::step，并按原先决策、retail 生命周期、Event 顺序
委托 receiver；不会重排种子执行、改变自由调度或建立交易状态副本。观察和 consuming finish
继续调用既有 summarize_stock/ensemble 纯统计；ParticipantExecutionAccumulator 的
record/reconcile/finish 改为 receiver。所有 checked arithmetic、错误上下文、浮点语句顺序与
PreOpen 错误之前已累计的部分投影保持。

短 fixture 覆盖空盘口及双边深度/spread/imbalance 样本、日末 no-trade streak 重置、PreOpen
错误之前已写 Trade/双边参与量，以及未知 taker 前已写 maker 的诊断行为。

测试 filter：`diagnostics::tests::seed_projection_`（3 个）；default features 可用。
原参与量对账 filter：`diagnostics::tests::participant_execution_reconciliation_`。

## domain-R2-N32

私有 `CausalReportBuilder` 借用 facts，拥有 orders/fills/unmatched_fills/market_volume/
information_delays，consume_fact 保留 Sequence、预算、订单、fill、终止和 Execution 的校验
次序；accept_submission/accept_fill/accept_termination/reconcile_execution 分别推进状态，
build_report consuming 装配原 CausalReport。OrderLifecycle::finish_lifecycle 负责终结时的两种
时钟和删失信息。CausalReport::from_facts 保留公开 facade 与 DTO 形状。

microstructure 由两个 owner 组合：DirectionPersistenceAccumulator 借用完整 facts，
拥有 directions/pairs/same，按原股序列更新并 consuming finish 派生比例；QuoteResponseAccumulator
同样借用完整 facts，拥有 prior_quotes/impacts/recoveries，consume 时保留原 Execution 后续
index+1 切片和 loss 当前 index 切片扫描，consuming finish 输出观测样本。analyze 是真实组合 caller，
逐 fact 先更新方向，再更新 Quote 响应，与原失败顺序一致；没有建立在线递推替代路径。
先追加 direction_persistence 短行为保护，再迁移累加器；Quote 响应沿用既有短保护。测试固定
每股独立方向链与 side=None 不计主动方向，以及首个有效 Quote 后扫和当下恢复，不把观测冲击
改为因果估计。原浮点运算、缺失原因与删失字段保持。

测试 filters（需要 simulation-diagnostics）：
- `diagnostics::causal::aggregate::tests::report_builder_`（5 个）：序号/预算错误优先、空/restore、
  双边 gross、重复 fill/overfill 部分写，以及 market/civil 时钟分别倒退。
- `diagnostics::causal::microstructure::tests::`（2 个）：有效 Quote 后扫和当下恢复；每股方向链与竞价无主动方向。

## domain-R2-N39

可选动作已实施。`PhaseTimingLedger` 独占固定 Accumulator slots、前置阶段完整性及 records
投影；PhaseTimingPhase::metadata 统一 rank/name，index 仍由 rank 派生，From<TickPhase> 映射不变。
Collector 继续持有 active span、tick_before/tick_after 和 overflow，并保留前置校验的缺阶段优先、
计数溢出与成功提交后才能产生证据的原先次序。

runnable_threads/runnable_minimum/runnable_maximum 的名称、序列化和数值未修改；
current_runnable_threads 仍调用 rayon::current_num_threads，代表 registry 配置 worker 容量，
不是 OS runnable、active worker 或 CPU 利用率。已有英文注释保持原样，未据本动作另改其含义。

先写 ALL/rank/index/name/From 映射与 sample overflow 保护；再补 slots 的 wall/span/sample
错误覆盖次序和部分采样、precommit span 完整但缺 sample 的投影拒绝。

测试 filter：`verification_evidence::phase_timing::tests::phase_timing_`（4 个）；default features 可用。
已有真实 Session 计时保护可用整个 `verification_evidence::phase_timing::tests::`（共 12 个）。

## domain-R2-N41

`CausalCollector` 的 facts/decision/filled_values 已设私有，termination 保留原字段，不造生命周期。
Session 的 causal、plan-root、continuous fill 和 auction receipt caller 已全部委托下列 receiver，
原调用位置与后续 decision trace 顺序已静态复查闭合：

- facts() -> &[CausalFact]；decision_for(account) -> Option<u64>。
- record_plan_root(time, account, facts: impl IntoIterator<Item=CausalFactKind>)：先记录当前 facts.len()，
  再逐项 record；空 batch 或首项不是 Decision 均保持原接受集合。
- record_continuous_fill(time, order, account, code: &StockCode, qty, gross: i64) -> ()：entry0，
  append Filled，再 checked_add；溢出仍保留已追加事实与旧累计值，不新增 panic/Err。
- record_auction_fill(time, order, account, code: &StockCode, qty, value_before: i64,
  value_after: i64) -> Result<(), &'static str>：先 gross.checked_sub，再 entry0/chaincheck，
  然后更新累计值并 append；原两条错误文本保持，caller 用 map_err(lifecycle_invariant)。

Auction chaincheck 失败可能已经插入 0 键；gross 算术失败发生在 entry0 前。采集仍仅存在于
simulation-diagnostics feature，不写存档，也不从最终余额重建真实 receipt。

先写 receiver 行为测试，再实施方法；由于禁止本 agent 运行产品测试，未确认任何运行红/绿。
测试 filter：`diagnostics::causal::tests::collector_`（4 个），需要 simulation-diagnostics。

## 检查与交接

限定六个源码文件执行 rustfmt（edition 2021，skip_children=true）成功；限定文件的
`git diff --check` 成功。该检查仅证明语法可格式化和没有空白错误，不证明 Rust 类型检查或测试通过。

完整改动文件清单：
- packages/engine/src/diagnostics.rs
- packages/engine/src/diagnostics/causal.rs
- packages/engine/src/diagnostics/causal/aggregate.rs
- packages/engine/src/diagnostics/causal/microstructure.rs
- packages/engine/src/verification_evidence/phase_timing.rs
- packages/engine/src/verification_evidence/phase_timing_tests.rs
- agents/oop-refactor-implementation/domain/diagnostics-result.md（新工作记录）

未修改 diagnostics/decision_trace.rs、causal/report.rs。六个最终冻结源码文件的完整 baseline diff
已由未参与实施的 review_diagnostics subagent 独立静态复核；N32 两个 microstructure owner、
A 股语义、范围必要性、边界及跨层接线均已核对，无待修有效发现。N41 的 private-field caller
迁移已由 Session owner 接入并完成只读复查。依据与限制见
[diagnostics-review.md](diagnostics-review.md)。

root 最终冻结后编译及 21 个指定短 case 已通过，包括 N32 两个 microstructure owner 的方向/报价保护和 N41 receiver 接线。每个普通测试 child/case 仍受 10000ms deadline，显式多进程与 Rayon 并行；实际证据见 [final-summary.md](final-summary.md)。

当前状态：5 动作及其可选组合、N41 跨组 caller、独立静态复核与指定短测均已闭环。本组无未完成事项；完整回归及任意长期诊断验收未运行。
