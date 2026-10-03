# pipeline state caller 迁移记录

## 范围与结果

按 `/tmp/pipeline_remaining_0.txt` 的 36 个文件逐个全文读取并检查跨组 OOP caller。实际修改 15 个文件；未涉及新接口的文件保持原样。

- `GameSession` 原字段访问改为显式 `state` 访问，facade 方法保持既有调用。
- `Account` 读取改用 getter，测试写入改用 `fixture_set_cash` / `fixture_set_strategy`；生产 NPC strategy 投影改用 `restore_strategy`。
- `ParentOrderPlan` 读取改用 getter，测试构造改用 `from_facts`；`TradingPlan` 读取改用 getter，`SaveSlot`、公开 Snapshot 与订单事实 DTO 的字段保持原契约。
- Candle 只读访问改用 `state.candle_book.histories()` / `active()`。
- attention 队列改用 `state.attention_scheduler`；生产队头判断使用 `next_scheduled_tick()`，保持原 O(1) 队头读取与到期条件。
- `institutional_experience_projection` 与 `stock_execution_transaction` 的参数改为 `AccountPagedMap<BeliefParticipantState>`；通过 participant 的 `belief()` 读取，返回的 `belief_patch` 仍为 `BeliefBook`。测试个人信息读取通过 `state.belief_participants[id].information()`。

`retail_projection_persistence_tests.rs` 虽列在原分配清单中，父任务随后明确转交 projections。停止对该文件继续编辑与格式化；此前仅做 Session state 和 Account/Position getter 迁移，最终内容由 projections 接管。

## 已修改文件

- `account_validation_context.rs`
- `candidate_commit.rs`
- `continuous_matching_adapter_tests.rs`
- `continuous_tick_transaction_tests.rs`
- `executor_perturbation_tests.rs`
- `institutional_experience_projection.rs`
- `npc_state_projection.rs`
- `npc_tick_preparation.rs`
- `pre_open_transaction_tests.rs`
- `quote_expiry_checkpoint_tests.rs`
- `receipt_aggregation.rs`
- `session_execution_transaction_tests.rs`
- `stock_auction_adapter_tests.rs`
- `stock_execution_transaction.rs`
- `tests.rs`

## 语义与验证

本批仅适配状态封装接口，未改变 A 股申报数量、T+1、费用、交易阶段、母单实际成交回写、Receipt 身份或提交原子性。依据既有 ADR-0015 与 pipeline 原实现保留语义；未新增交易制度，无需新增官方规则假设。

已对以上 15 个文件精确运行 `rustfmt --edition 2021 --config skip_children=true`。范围静态扫描未发现旧 Session 容器访问、重复 `state` 或重复 getter 括号；范围内 `git diff --check` 通过。

按父任务约束未运行 Cargo、测试、全仓格式化或 Git 写操作。编译、测试和未参与实现的独立 subagent 复核由父任务统一安排；本记录不宣称完整改动已验收。

## 独立复核补修

session 独立复核发现 `continuous_tick_transaction_tests.rs` 遗漏 `TradingPlan` 私有字段读取。已补齐该文件与 `pre_open_transaction_tests.rs` 的 `active_child_order_id()`、`filled_qty()` 和 `status()`；`SaveSlot.plans.plan()` 返回的同一 `TradingPlan` 也同步适配 getter，Snapshot DTO 字段保持原样。重新精确格式化两个文件，范围 `git diff --check` 与全部 owned 文件的 `TradingPlan` 命名变量字段静态扫描通过。完整编译与复核结果仍由父任务记录。
