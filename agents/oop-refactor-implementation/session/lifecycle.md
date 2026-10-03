# Session 生命周期实施记录

日期：2026-10-03。实施范围为 `session-N01`、`session-N02`、`engine-session-02-A03`、`session-R2-N01`；最后一项的可选优先级不影响本轮实施，已纳入。

权威范围：[action-index.md](../../oop-refactor-audit/challenge-2026-10-03/action-index.md)。领域依据沿用 [ADR-0015](../../../docs/decisions/0015-parent-order-execution.md)、[ADR-0011](../../../docs/decisions/0011-market-time-observations-and-position-risk.md)、[ADR-0018](../../../docs/decisions/0018-long-running-immutable-timeline.md) 与 [trading-rules.md](../../../docs/trading-rules.md) 的已有规则登记。本轮只迁移 owner 和调用，不新增交易制度，未重新访问官方网页；不能把既有规则日期当作本轮重新核验日期。

## 状态与范围

| 动作 | receiver 与真实接线 | 状态 |
| --- | --- | --- |
| session-N01 | `ParentOrderPlan` 接收实际 child submission/fill/cancel，拥有同向 limit 修订与 child intent 续发计算；连续路径接 `execution/records.rs`，两种构造接 `orders.rs` 与 `plan_execution/routing.rs`。竞价调用交 `pipeline/auction_day_end.rs` owner 接入。 | 所属源码已落盘；跨组接线与独立复核待统一验收。 |
| session-N02 | 私有 `ProtocolState` 成为 `ProtocolSession`/`ProtocolCheckpoint` 唯一回滚集合；`PublicationFactCursor` 拥有同 tick 的事实续接。 | 所属源码已落盘，新增缓存恢复和 cursor 测试。 |
| engine-session-02-A03 | `SessionCandleBook` 拥有完整 histories 与 active map，接真实记录/日终归档；Session 保留原薄适配器签名。 | 所属源码已落盘；snapshot/save/hash/restore 双字段投影交 Session 与 persistence owner。 |
| session-R2-N01 | `NpcAttentionScheduler` 拥有 heap 与过期候选过滤；当前 candidate tick 从 Session 权威状态只读查询。 | 所属源码已落盘；其余 enqueue/hash/restore caller 交对应 owner。 |

指定文件以外没有实施修改。`session.rs` 由上层持有；竞价、存档、哈希与 snapshot 调用迁移不在本 agent 文件所有权内。`plan_execution` 与 `protocol` 中使用的 `GameSession` 字段已迁到 `state`；`TradingPlan`/`ParentOrderPlan` 读取使用 getter，非法测试 fixture 继续构造等价事实，没有删除或弱化原断言。

## 跨组 API

- `SessionCandleBook::new(histories, active)`；`histories()`/`active()` 只读 map；恢复使用 `replace_histories`/`replace_active`；test 使用 `replace_history`/`set_active`。`record_trade_or_mark(day, code, price, volume)` 与 `commit_active()` 保留既有 checked 算术、零成交占位及首笔真实成交替换。
- `NpcAttentionScheduler::enqueue(tick, account)`、`next_scheduled_tick()`、`iter()`，支持 `FromIterator<(u64, AccountId)>`。`clear()` 为 test 接缝；`pop_due(tick, current_candidate_tick)` 保留完整 popped entries、匹配 tick 过滤、排序去重。
- `ParentOrderPlan` 字段仅 execution module 内可写，同名 getter 返回原事实（`code()` 为引用，其余为 Copy）。`from_facts` 接配对的 active tuple；`from_saved_facts` 保留恢复阶段独立的双 Option；`restore_active_child_remaining_qty` 用真实 live order 重建数量，不增加预校验。测试专用 `replace_execution_facts_for_test` 保留不一致 fixture。
- 连续 `record_submission`/`record_fill_after_settlement`/`clear_matching_child` 保留原 assert/expect 顺序。竞价 `checked_record_submission`/`checked_record_fill`/`checked_clear_matching_child` 返回原错误字符串，由 caller 映射原 `lifecycle_invariant`；先校验再改候选，不提前安装全批结果。
- `ParentFillTransition` 返回 `child_complete`、`completed_unlinked` 与 `linked_plan_id`；Session 继续附加 trading day、入队 pending facts、按 account/stock 清理 map。linked 母单终态仍归 PlanBook 同步后清理。
- 诊断连续 fill 已接 `CausalCollector::record_continuous_fill`，沿用此前事实顺序及累计 gross 溢出保留旧值的契约。

## 语义保持

金额仍为分，数量仍为股；T+1、合法撤单窗口、真实费用与订单受理/撮合路径沿用原实现。母单买入不足 lot size 的余量继续不提交，收盘集合竞价不复制连续簿 child；child 接受不计成交。完整 candle history 不截断，clone 继续共享旧 AppendOnlyHistory chunk。存档继续分别投影 `daily_candles` 与 `active_daily_candles`，不增加 wire owner 或迁移格式。Protocol 测试注入 flags 留在 facade 外层，不进入 checkpoint；无 DerefMut；新 restore 从空 publication cursor 开始。

## 测试与诚实边界

新增 15 个短行为 tests：ParentOrderPlan 6 个、SessionCandleBook 4 个、NpcAttentionScheduler 2 个、Protocol 3 个。初始保护 tests 先于 receiver 实现写入；额外边界在迁移中补入。按上层禁止各 agent 运行 Cargo/产品测试的约束，尚未执行实际红灯或绿灯，不宣称 TDD 运行已验证。

交 root 的 filters：

- `session::execution::parent_transition_tests`
- `session::candles::candle_book_tests`
- `session::candles::shared_daily_history_tests`
- `session::attention::scheduler_tests`
- `session::attention::attention_tests`
- `session::protocol::civil::session::rollback_tests`
- `session::plan_execution::continuation_tests`
- `session::protocol::civil::publication_tests`

另需统一跑 `auction_day_end_tests`、`continuous_tick_transaction_tests`、`decision_snapshot_capture_tests` 与 `persistence::v2_tests` 的既有短分片，以核验外部迁移。

已执行：只对所属修改文件运行 `rustfmt --edition 2021 --config skip_children=true`，及指定路径 `git diff --check`，均无错误。未运行 Cargo、build、产品 tests；未 Git 写操作；未派 subagent。独立完整 diff 复核由上层统一安排，尚未通过前本记录只表示代码交付，不表示任务验收完成。

## 独立初审修正

`review_lifecycle` 提出的 L1/L2 已修复：补齐 `replace_child_facts` 配对写入口，恢复 helper 同样使用该入口；补齐 `owned_plan_sync_late_failure` 的 `first.*` getter。`engine-build-02-session.log` 的全部所属文件诊断已逐条核对，范围内诊断正是这两组；已修复但本 agent 未运行新的 Cargo 验证。对所有所属 plan execution 源码完成已知 TradingPlan/ParentOrderPlan receiver 的 raw field 读取自扫，无剩余命中。

L3 已补 `unlinked_parent_reverse_rebuilds_execution_from_the_new_target`：先受理旧 Buy child，用结算成功的 typed fill fixture 经生产 adapter 推进部分成交，再在已有 Buy 工作单的输入上给新 Sell 固定目标；验证新 target/fill/child/limit/expiry，且不继承旧 child。该 fixture 只覆盖母单 adapter 输入，不冒称运行完整撮合/账户结算；真实撮合路径由既有 integration tests 另行固定。等待 reviewer 再次复核与 root 集中运行。
