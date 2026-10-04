# Q21 / Q22 当前实现核实

## 范围与依据

只核对当前工作区生产路径和其短测试，不运行测试或编译，不把旧审计扫描结果当作当前事实。领域契约按优先级对照用户在 `agents/implementation-audit/implementation-audit-2026-10-02.md` 中 Q21/Q22 的完整回答、[ADR-0017](../../docs/decisions/0017-escrow-parallel-tick.md)、[ADR-0018](../../docs/decisions/0018-long-running-immutable-timeline.md) §6.1、§6.2、§7 和 [ADR-0020](../../docs/decisions/0020-native-allocator-for-concurrent-ticks.md)。`docs/testing.md`“并发受理与重放的覆盖边界”是目前测试解释契约。

Q21 用户回答：多个非法条件没有特定报错优先级，一个明确原因即可；NPC 可以预检查，但不能替代受理时的最新资金、股份和价格校验，也不得误拒合法零费用或非正净成本。

Q22 用户回答：多线程并行、减少锁是首要目标；来源排列不决定受理顺序；NPC 在 T-1 完成决策后异步进入委托队列，T 统一受理撮合；同账户资源约束和同股价格时间优先继续有效。完整核实还应覆盖实际请求接收顺序、决策完成、队列截止和测试；减少锁不是全系统绝对无锁证明。

## Q21：交易校验

**核实结论：** 当前有权威 AccountValidation 与 StockProcessing 复核，未发现 NPC 预检绕过受理校验；业务拒绝由 tick shadow 收据/事实路径处理。现有代码与短测试支持“拒绝可显式反馈且不产生被拒单的部分成交/挂单”。不应把任意两个非法条件的先后原因固化成需求。此项可按当前证据核销实现缺口；它不是对每种错误组合都已逐项覆盖的测试声明。

- NPC 策略确实会按观察到的个人现金做买入数量预估，例如 `strategy/zi_noise.rs`、`strategy/institution.rs`、`strategy/hot.rs`；但生产 `continuous_tick_transaction.rs::apply_session_continuous_transaction` 仍为本 tick 建立 `AccountValidatorDriver`，所有候选都进入 `ready_ingress.rs::validate_available_ready` / AccountValidation。分配预算来自 `plan_tick` 的 ExpiryShadow 后 `DecisionResourceSnapshot::seal`，不会以 NPC 预检作为权威接受。
- `account_validation.rs::validate_place` 使用当前 tick 密封的账户预算检查数量、手数、可卖股数和买入现金；账户预算在持续 validator 中按账户资源 lane 消耗。价格笼子、涨跌停及阶段校验继续在股票处理路径中对实际受理操作执行；例如 `continuous_matching_tests.rs::price_cage_rechecks_an_observed_buy_after_the_earlier_admitted_sell` 验证先发生的同股操作可改变后单结果，失败单释放预留、无 Trade，原挂单保留。
- 多个非法条件时，`validate_place` 当前有数量、可卖数量、现金等检查顺序，但这只是实现可能返回的其中一个理由；比如 cash 不足与价格越界可能先返回 `InsufficientCash`，价格合法性再由股票阶段判定。用户没有指定错误优先序，测试不应要求固定跨阶段错误优先级。
- 单拒绝无部分挂单/成交证据：`continuous_matching_tests.rs::price_cage_reject_consumes_the_preallocated_id_and_releases_the_draft` 断言空簿、一个拒绝收据、全部预留释放；`price_cage_rechecks_an_observed_buy_after_the_earlier_admitted_sell` 断言零成交且原卖单完整保留；`continuous_tick_transaction_tests.rs::public_step_reports_zero_quantity_in_event_and_retail_diagnostic` 断言数量拒绝事件及簿订单数为零。价格/股票阶段拒绝仍按 ADR-0017 消耗已预分配 `OrderId`；这不是部分生效。
- 新增的 `npc_tick_preparation_tests.rs::next_tick_account_validation_rechecks_npc_orders_against_current_cash` 在真实 NPC 决策入队后把现金改为零，再走 `GameSession::step`，断言 T 时 `InsufficientCash`、无该账户 `OrderAccepted`/`Trade`、余额仍为零及无 NPC 挂单。第一次精确执行失败于 fixture 生命周期：`GameSession::new` 留下了未消费的空 `pending_npc`，导致重复 queue；修正为先断言该批为空并清除，再显式构造 T-1 的 NPC 决策。修正后的源码尚未重跑，不能先报通过。
- `account_validation_tests.rs` 的多拒绝表断言各独立非法输入获得合理明确理由；该表的各条件各自单一无效，不承诺复合非法条件之间的错误顺序。`continuous_tick_transaction_tests.rs::player_rejections_and_acceptance_keep_cash_order_and_order_identity` 验证同账户现金序列中拒绝单不占预算、后单可接受，属于相关资金语义证据。

## Q22：实际接收、队列与截止

**核实结论：** 保留待修缺口，不可核销。价格时间与账户资源冲突的局部排序机制、跨股票并行和计划按需唤醒已经存在；但当前候选入口和账户收据把来源/生产阶段映射成固定顺序，且 NPC 决策结果在全部账户结束后才整体汇入一批，没有满足用户回答的“按实际接收先后”以及独立决策异步入队。当前没有证据把固定类别顺序解释成真实接收事实。

### 实际生产流水

1. `npc_tick_preparation.rs::queue_npc_for_next_tick` 在 tick shadow CommitTick 前调用；它通过 `prepare_npc_projection` 捕获决策快照、运行决策、投影后，才一次性把 `projected.intents` 放入 `pending_npc: Option<PendingNpcBatch>`。`npc_decisions.rs::run_npc_decisions` 对 due accounts 使用 `par_iter`，但 `.collect::<Vec<Result<_, _>>>()` 等待整批结束，再按 canonical account 顺序消费结果。账户内部有并行计算，却没有每个账户/意图决策完成就异步登记到可受理队列的路径。
2. 下一个市场 tick 的 `ready_ingress.rs::capture_sources` 先 `take_ready_npc_batch(session)`，再 `capture_player_candidate_batch()`；`candidate_composition.rs` 以 NPC vec 后接玩家 queue vec 组成 initial candidates。这里仅标识来源的 key 不该成为优先级，但调用顺序为候选准备提供了固定来源布局。
3. `local_admission.rs::AccountReceipt` 直接定义 `PreviousCommit(u64) < BetweenTicks(u64) < ReadyThisTick(u64)`。NPC 映射为 `PreviousCommit(npc_local_index)`，玩家为 `BetweenTicks(player_queue_index)`，PlanChain 为 `ReadyThisTick` ordinal。`build_resource_edges` 在同账户 Cash/Share lane 中按该 receipt 建偏序，因此 NPC 对玩家的固定优先不由请求真实接收时间决定。跨来源同账户冲突的 outcome 是可观察业务结果，故不是纯事件布局问题。
4. 同股 gate 对无因果边候选通过并发登记局部受理顺序，worker 完成/订单身份不直接充当股票交易优先；同账户现金/股份 lane 限于真实资源冲突。`stock_stream.rs` 继续逐股驱动，`ready_ingress.rs::validate_available_ready` 在计划续行就绪时可继续接纳独立候选，当前这部分符合减少无关等待方向。
5. 玩家 queue 的实际 T cutoff 是 `capture_player_candidate_batch` 在 tick candidate 上取走 pending 队列的边界；`enqueue_player_intent(&mut self, ...)` 校验玩家身份后追加 `pending_player`。本仓库同步 engine API 没有接收时间戳/全局接收 ordinal，也没有显示异步 server 队列到 engine cutoff 的桥接契约。因此无法证明跨 NPC 与玩家来源的实际接收顺序。API 的独占 `&mut self` 令同一 `GameSession` 的直接调用不能并发修改；它本身也不能替代宿主异步请求队列的接收顺序记录。

### 现有测试证据及边界

- `ready_ingress_tests.rs::account_receipts_follow_request_time_and_plan_readiness_not_key_order` 实际构造 PlanChain 与 NPC，检查 NPC 在同账户 lane 中的 account receipt 顺序；没有 Player 候选，不能证成 NPC/Player 跨来源按实际接收顺序。
- `account_validation_tests.rs::account_validation_cash_budget_contends_across_stocks_in_account_receipt_order` 验证两张同来源 player 买单在异股竞争现金时按 queue index；`local_admission` 的 `crossed_account_stock_requests_keep_each_account_receipt_order` 同样只使用 Player keys。这些用例覆盖来源内序，不覆盖跨来源到达轨迹。
- `initial_candidate_round_tests.rs::continuous_initial_round_batches_independent_accounts_and_stocks` 证明独立账户/股票同批校验和撮合；它甚至展示 `npc` key 后接 `player` key 的 sealed index/OrderId布局，但账户不同，不验证来源顺序正确性。
- `npc_tick_preparation_tests.rs` 有 pending NPC 单批存档/恢复、身份和依赖测试；`npc_decisions.rs` 只验证账户决策并行、canonical 错误反馈和 RNG 与调度独立。未见“每账户决策完成立即异步入 queue”和玩家入队 cutoff 竞争的生产 contract test。
- `docs/testing.md` 当前并发测试已正确放弃自由调度整局字节等价要求；`executor_perturbation_tests.rs` 验收守恒和成交事实并不自动覆盖实际接收轨迹或队列截止。这项不能由已做并发扰动测试核销。

### 建议最小修正范围

- 用显式的“已接收事实”给冲突请求排序，而不是由 `CandidateSource` / `AccountReceipt` enum variant 表示来源阶段优先。其粒度只需足以维护同账户资源冲突和股票入口局部事实；独立账户、独立股票不应为获得全局序号而串行。
- 对 NPC 决策完成与入队边界作窄修正，使不同账户互不依赖的决策结果能在 T-1 完成时进入 queue；同时保持 T 的统一业务校验/撮合 cutoff。账户或同股冲突仍按实际登记的先后建立偏序，计划动作仍只依赖自身 typed result。
- 补一个短 contract test 固定至少一组跨 NPC/Player 来源的同账户现金竞争输入，改变来源标签与实际 receive order 的对应关系，断言 winner 跟 receive order 走而非来源/key/OrderId；再补 queue cutoff 的边界断言：cutoff 前已接收进入当前 T，cutoff 后进入 T+1。决策并行入队如改动，则另加可控完成顺序测试证明首个完成的独立账户不等待其他账户。
- 失败仍由 tick shadow 原子提交边界丢弃；测试检查业务状态、订单簿和现金/股份不留下失败轮次部分效果。避免把“减少锁”扩写为绝对无锁断言。

本报告包含 Q22 只读流水核实和 Q21 实施记录；只改了 Engine 测试源码与 `docs/testing.md`，未改生产逻辑、ADR、公开 open-questions 或审计总账。root 刷新编译后在外部 10000ms deadline 下精确执行 Q21 新 case，通过，实际执行 0.22 秒。非作者 `review_q21_current` 完整复核三文件通过；Q22 仍是待实现项，未跑完整回归。
