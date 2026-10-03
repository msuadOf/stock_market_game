# Continuous 文件簇 receiver 迁移记录

日期：2026-10-03。范围：`engine-pipeline-05-A01`、`pipeline-N01`、`pipeline-R2-N04`、`pipeline-R2-N06`；采用 A01 的可选 `pipeline-R2-E03` 单 Place builder 组合，不重复计数旧 receipt 方法迁移。

## 方法与实际生产调用

| 动作 | owner 与方法 | 已迁生产调用 |
| --- | --- | --- |
| A01 | `ContinuousStockRoundProcessor::new/run/reject_place/record_place_fact/apply_receipts/fill_receipts` | `process_continuous_stock_step_inner` 初始化并消费 Processor；完整操作循环与最终 MarketDelta 位于 `run`，原 worker 入口保留。 |
| A01/E03 | `ContinuousFillReceiptProjection::capture/run/record_fill` | Processor 从自己的 ledger/config 构造单 Place builder；builder 持有 states/ordinals/receipts，按原每成交腿 Buy→Sell 顺序推进。原 `fill_receipts` 仅保留 `cfg(test)` 入口。 |
| N01 | `ContinuousLifecycleEventBatch::from_events/emit_event/emit_fill/finish` | `project_continuous_retail_lifecycle` → `order_lifecycle_events` → batch；原 `push_fill_and_terminal` 已移除。Session 仍仅在全部依赖校验成功后 append。 |
| R2-N06 | `IncrementalContinuousStockShadow::apply_round/finish` | coordinator 并行 round 调用各股 consuming `apply_round`；coordinator consuming `finish_for_tick` 按原股票迭代调用每股 `finish`，总 fact 计数与首错仍在 coordinator。原 `apply_stock_round` 已移除，测试 private caller 同步迁移。 |
| R2-N04 | `ContinuousDayEndLifecycleProjection::new/apply/apply_release/finish` | `finalize_continuous_tick` 的 ends_day 分支；日终生命周期投影完成后，原位置调用 `TradingDayEndTransition::new(session).apply()`。 |

## 语义与失败边界

依据 ADR-0017 顶部 2026-09-24/25 修订和 ADR-0018 §7 已接受的局部受理决定。本批是 receiver 与内部状态 owner 迁移，没有改变交易制度，无须新增官方规则依据：同股操作 vector、价格时间优先、P0/P1 截点、P4 释放不回补预算、统一 Settlement、日终之后的 T+1 解锁与 P9 提交边界均保持。

单轮错误丢弃 Processor；跨轮 shadow 的 consuming round 错误丢弃该 shadow 并使 coordinator 失效，不承诺局部重试或 Err 后保留 shadow。首次/独立 worker 的完整证据检查仍在轮次完成处，所有 continuation 排空后的跨轮簿/ledger 检查仍在 coordinator consuming finish 路径，只将每股行为移入 consuming shadow finish。

N01 保持 source quantity 检查→余留 quantity 映射检查→数量链检查→重复 Submitted 检查→事件依赖推进→未解决依赖检查的首错顺序。只整理同 OrderId 的 Submitted/Fill/terminal 依赖，无关订单仍保持原位置关系。

日终仍先 causal termination、parent cancellation、NPC removal、retail cancellation，再计算 checked event index；错误前已有的 candidate 写入保留，由外层丢弃 candidate。closing price 在清簿前冻结，日终释放与最终交易日转换仍仅一次。Money 单位为分，qty 为股，费用计算继续委托原 FillTransition。

跨组接口同步：GameSession 私有状态迁 `state`；Account/Position 读调用迁 getters；Candle 读调用迁 `state.candle_book.active()/histories()`，fixture 使用 `set_active`；parent 字段迁 getter；PlanChainFactConsumption 查询迁 `contains_operation/contains_receipt`；belief 参数接 `state.belief_participants`，patch 仅替换 participant 内 `belief_mut()` 并保留其余三成员。

## 精准短测试过滤器

新增或补充行为测试，建议由 root 使用普通 10000ms 进程树 deadline 与明确测试线程并发运行；测试二进制构建单独归编译验收。默认 features 覆盖主体，另用 `simulation-diagnostics` 复验诊断分支。

- `symbolic_resolution_and_two_makers_keep_source_local_fill_ordinals`：PriceResolved→incoming 两腿 ordinal 0/1/2、两个 maker 各 ordinal 0、数量链及完整 receipt replay。
- `lifecycle_batch_preserves_quantity_and_dependency_first_errors`：source quantity 缺失/多余、断链、重复 Submitted、双 terminal，以及混合错误首错。
- `lifecycle_batch_rejects_unresolved_dependencies_without_publishing_output`：局部数量链缺失、重复 fill、finish 未解决依赖拒绝。
- `empty_day_end_release_projection_has_no_order_outbox`：空日终 release 不产生订单诊断或事件。
- `day_end_release_projection_orders_envelopes_and_keeps_partial_candidate_on_index_error`：EnvelopeKey 排列、身份唯一与 index 溢出前的 candidate 局部写入。
- `consuming_stock_finish_freezes_depth_before_day_end_and_emits_one_release`：清簿前冻结 depth 与唯一 DayEnd release。

既有关键过滤器：`fill_receipts_loads_only_the_incoming_order_and_traded_maker`、`post_quote_expiry_stock_shadow_survives_routes_and_same_tick_cancel_sees_the_created_order`、`stock_local_trade_identity_continues_across_adaptive_routes`、`one_stock_worker_failure_invalidates_the_discardable_tick_coordinator`、`two_stock_worker_errors_select_first_stock_under_reversed_delivery`、`successful_cancel_cannot_hide_a_remaining_book_ledger_mismatch_at_finish`、`partial_fill_then_day_end_keeps_cross_source_receipts_and_cancellation_quantity`。

诊断特性既有关键过滤器：`day_end_release_terminates_the_causal_lifecycle`、`day_end_causal_facts_keep_the_completed_continuous_phase`、`day_end_quotes_each_cleared_stock_and_skips_untouched_stocks`。

## 验证状态

精确 `rustfmt --config skip_children=true` 与本簇 `git diff --check` 已通过。按照父任务协调限制，本 worker 未运行 cargo、产品测试、Git 写入或全仓格式化；新增测试没有执行过 red/green，不把静态检查冒充运行结果。root 统一编译、执行精准短测试并安排未参与实施者独立审查完整 diff，门禁完成前不能宣称本批完成。
