# Rust 精准短测最终验证索引

**278 个唯一 Rust case 全部通过**：273 个常规 case 与 5 个 Writer case，0 failed，全部 `schedule=false`。权威结果为 [final-validation.json](final-validation.json)，本索引保留原 R001–R278 和 128 action 映射，逐项绑定完整语义 key、最终 proof record、当前 source SHA 与函数声明行。

末次删除 decision_chain.rs 中无 caller 的过渡 wrapper 后，两个同源 case 已精确重验并替换最终 proof。当前 source SHA 为 `b6696ad873d44ab39443ea99d21f31e1dd26e658ad447e494f3971da644399d5`；build10、features check11 与 default check12 全部退出 0，最终 Rust typecheck warnings 为 0。

本文件最初作为待执行短测计划建立，现已原位更新为最终验证索引。索引维护者本人没有运行 Cargo、test binary 或 `--list`；运行结果来自已有 root 证据。

基线：`b89afb3346743a4b4fccf26c9ac9ff108595f696`。完整 key、record/source SHA、当前声明行和 features 见 [rust-short-plan.json](rust-short-plan.json)。

## 已记录的执行参数

- root 逐 case 使用预编译 binary 与 `--exact`，最多 8 个进程并行；每进程 `RAYON_NUM_THREADS=4`、`--test-threads=1`。
- 单 case 与整命令进程树 deadline 为 10000ms；最终最长 case 为 1.778 秒。
- 编译、执行、sandbox 环境失败与成功重跑分别留证；最终 proof 选取成功的精确执行，不重复计数。

## 编译与检查证据

| 记录 | exit code | 秒 |
|---|---:|---:|
| [hosts-engine-build-05-result.json](hosts-engine-build-05-result.json) | 0 | 66.574 |
| [targeted-tests-build-07-result.json](targeted-tests-build-07-result.json) | 0 | 57.556 |
| [rust-all-targets-check-08-result.json](rust-all-targets-check-08-result.json) | 0 | 4.258 |
| [rust-default-targets-check-09-result.json](rust-default-targets-check-09-result.json) | 0 | 39.535 |
| [engine-final-build-10-result.json](engine-final-build-10-result.json) | 0 | 35.284 |
| [rust-final-features-check-11-result.json](rust-final-features-check-11-result.json) | 0 | 8.259 |
| [rust-final-default-check-12-result.json](rust-final-default-check-12-result.json) | 0 | 7.759 |

## package / target / features

| package / target | 已记录的 compiled features | PASS case 数 |
|---|---|---:|
| `engine/lib/engine` | simulation-diagnostics, verification-harness | 219 |
| `server/lib/server` | host-parity, web-ui | 10 |
| `web-wasm/lib/web_wasm` | simulation-diagnostics | 4 |
| `stock-market-game/lib/stock_market_game_lib` | custom-protocol, host-parity | 4 |
| `engine/test/information_acquisition` | simulation-diagnostics, verification-harness | 2 |
| `engine/test/attention_discovery` | simulation-diagnostics, verification-harness | 2 |
| `engine/test/auction` | simulation-diagnostics, verification-harness | 2 |
| `engine/test/behavior` | simulation-diagnostics, verification-harness | 2 |
| `engine/test/fundamental_beliefs` | simulation-diagnostics, verification-harness | 2 |
| `engine/test/industry_reports` | simulation-diagnostics, verification-harness | 2 |
| `engine/test/plans` | simulation-diagnostics, verification-harness | 2 |
| `engine/test/publications` | simulation-diagnostics, verification-harness | 2 |
| `engine/test/session` | simulation-diagnostics, verification-harness | 2 |
| `engine/test/account` | simulation-diagnostics, verification-harness | 4 |
| `engine/test/strategy_state` | simulation-diagnostics, verification-harness | 1 |
| `server/test/deployment_cli` | host-parity, web-ui | 4 |
| `server/test/deployment_routes` | host-parity, web-ui | 3 |
| `server/test/api_contract` | host-parity, web-ui | 3 |
| `server/test/ws` | host-parity, web-ui | 2 |
| `engine/test/save_contract` | simulation-diagnostics, verification-harness | 1 |
| `engine/example/escrow_verification_harness` | verification-harness | 5 |

`required_features` 与集中构建的 `compiled_features` 分开登记。Writer 使用记录内已注册的 ESCROW_WORKSPACE_ROOT、CARGO_TARGET_DIR、TMPDIR、TMP 与 TEMP，目录绑定 workspace/.tmp，具体值保留在 JSON。

## 实际范围限制

- 不运行完整回归、浏览器 E2E、长期模拟或性能矩阵。
- hosts-N04 与 hosts-R2-N05 的旧长周期 fixture 只承接编译和完整 diff 复核；不声称做了运行验证。
- 未运行 compile_fail doctest；字段 private 的检查由 Rust 编译和独立源码复核承担。
- 三个 Market case 完整迁入 lib，不重复计为新增覆盖。
- server_only_build_rejects_static_modes 在 web-ui 构建下被 cfg 排除，本索引不计该 case 通过。
- 索引维护者本人没有运行 Cargo、test binary 或 --list；本索引汇总已有 root 运行证据。

短测只保护选定 owner、调用与边界，使用少量条目/合同/facts、固定转换或局部 tick；integration 只执行最小代表 case。这些证据不扩称完整产品验收，也不证明每个 action 有独立新增测试。

## 精确 case 与最终 proof 索引

### `engine/lib/engine`

| ID | exact filter（完整语义 key 的第四项） | 最终 proof record | 当前源文件 / 声明行 |
|---|---|---|---|
| R001 | `indicators::tests::recurrence_protection_spans_ema_period_and_kdj_window_rollover` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/indicators.rs:301](../../../packages/engine/src/indicators.rs#L301) |
| R002 | `orderbook::book_state::tests::partial_maker_qty_write_precedes_progress_overflow_on_both_sides` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/orderbook/book_state.rs:22](../../../packages/engine/src/orderbook/book_state.rs#L22) |
| R003 | `orderbook::filled_orders::tests::clone_and_duplicate_preserve_filled_identity_snapshot` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/orderbook/filled_orders.rs:24](../../../packages/engine/src/orderbook/filled_orders.rs#L24) |
| R004 | `orderbook::persistent_index::tests::boundary_ids_remove_missing_and_duplicate_keep_original_root` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/orderbook/persistent_index.rs:180](../../../packages/engine/src/orderbook/persistent_index.rs#L180) |
| R005 | `orderbook::state_contract_tests::partial_full_cancel_and_clone_keep_identity_and_fifo` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/orderbook/state_contract_tests.rs:18](../../../packages/engine/src/orderbook/state_contract_tests.rs#L18) |
| R006 | `orderbook::state_contract_tests::maker_money_overflow_precedes_taker_money_overflow_without_book_write` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/orderbook/state_contract_tests.rs:50](../../../packages/engine/src/orderbook/state_contract_tests.rs#L50) |
| R007 | `orderbook::state_contract_tests::taker_money_overflow_leaves_book_status_and_cursor_unchanged` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/orderbook/state_contract_tests.rs:78](../../../packages/engine/src/orderbook/state_contract_tests.rs#L78) |
| R008 | `orderbook::state_contract_tests::projection_later_insert_error_preserves_earlier_write_and_old_cursor` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/orderbook/state_contract_tests.rs:106](../../../packages/engine/src/orderbook/state_contract_tests.rs#L106) |
| R009 | `orderbook::state_contract_tests::duplicate_qty_value_progress_price_keep_first_error_order` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/orderbook/state_contract_tests.rs:137](../../../packages/engine/src/orderbook/state_contract_tests.rs#L137) |
| R010 | `orderbook::state_contract_tests::valid_fill_progress_reaches_u32_boundary_on_both_sides` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/orderbook/state_contract_tests.rs:171](../../../packages/engine/src/orderbook/state_contract_tests.rs#L171) |
| R011 | `strategy::beliefs::risk_owner_tests::risk_latch_without_peak_preserves_memory_but_real_failures_can_pause` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/strategy/beliefs.rs:332](../../../packages/engine/src/strategy/beliefs.rs#L332) |
| R012 | `strategy::beliefs::risk_owner_tests::risk_latch_trigger_and_recovery_keep_distinct_equality_boundaries` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/strategy/beliefs.rs:359](../../../packages/engine/src/strategy/beliefs.rs#L359) |
| R013 | `strategy::beliefs::risk_owner_tests::risk_latch_invalid_facts_preserve_memory_and_error_order` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/strategy/beliefs.rs:383](../../../packages/engine/src/strategy/beliefs.rs#L383) |
| R014 | `strategy::retail::decision_context_tests::chase_profit_takes_only_sellable_shares_and_invalid_cost_keeps_buy_signal` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/strategy/retail.rs:324](../../../packages/engine/src/strategy/retail.rs#L324) |
| R015 | `strategy::retail::decision_context_tests::falling_chase_preserves_stop_dip_and_zero_stop_boundaries` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/strategy/retail.rs:345](../../../packages/engine/src/strategy/retail.rs#L345) |
| R016 | `strategy::retail::decision_context_tests::incomplete_minute_history_keeps_noise_branch_and_draw_order` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/strategy/retail.rs:378](../../../packages/engine/src/strategy/retail.rs#L378) |
| R017 | `strategy::retail::decision_context_tests::noise_sell_reselects_its_own_stock_view_and_position` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/strategy/retail.rs:392](../../../packages/engine/src/strategy/retail.rs#L392) |
| R018 | `strategy::zi_noise::projection_tests::strategy_data_projects_all_individual_retail_parameters` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/strategy/zi_noise.rs:296](../../../packages/engine/src/strategy/zi_noise.rs#L296) |
| R019 | `account::shadow_tests::restored_balances_and_t1_unlock_leave_shared_authority_unchanged` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/account.rs:720](../../../packages/engine/src/account.rs#L720) |
| R020 | `account::shadow_tests::failed_settlement_does_not_copy_or_mutate_shared_account` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/account.rs:746](../../../packages/engine/src/account.rs#L746) |
| R021 | `behavior::heuristics::position_context_tests::position_targets_preserve_odd_lots_and_t1_intent_execution_split` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/behavior/heuristics.rs:281](../../../packages/engine/src/behavior/heuristics.rs#L281) |
| R022 | `behavior::heuristics::position_context_tests::confidence_only_consumes_one_draw_for_eligible_buy_and_preserves_hold_watch` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/behavior/heuristics.rs:330](../../../packages/engine/src/behavior/heuristics.rs#L330) |
| R023 | `behavior::heuristics::position_context_tests::unusable_price_or_equity_preserves_existing_target_fallback_and_risk_guard` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/behavior/heuristics.rs:375](../../../packages/engine/src/behavior/heuristics.rs#L375) |
| R024 | `experience::feedback::lifecycle::institutional_transition_tests::institutional_observation_preserves_guards_threshold_and_personal_facts` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/experience/feedback/lifecycle/institutional_transition_tests.rs:31](../../../packages/engine/src/experience/feedback/lifecycle/institutional_transition_tests.rs#L31) |
| R025 | `experience::feedback::lifecycle::institutional_transition_tests::stale_institutional_holding_clear_preserves_history_and_unrelated_fields` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/experience/feedback/lifecycle/institutional_transition_tests.rs:102](../../../packages/engine/src/experience/feedback/lifecycle/institutional_transition_tests.rs#L102) |
| R026 | `experience::feedback::lifecycle::institutional_transition_tests::retail_dated_failures_preserve_legacy_partial_writes_and_feedback_success_boundary` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [packages/engine/src/experience/feedback/lifecycle/institutional_transition_tests.rs:126](../../../packages/engine/src/experience/feedback/lifecycle/institutional_transition_tests.rs#L126) |
| R027 | `experience::feedback::lifecycle::institutional_transition_tests::institutional_initialization_and_fee_failures_preserve_original_guard_order` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [packages/engine/src/experience/feedback/lifecycle/institutional_transition_tests.rs:187](../../../packages/engine/src/experience/feedback/lifecycle/institutional_transition_tests.rs#L187) |
| R028 | `experience::feedback::lifecycle::institutional_transition_tests::dated_writers_keep_distinct_map_key_sets_and_original_acceptance` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [packages/engine/src/experience/feedback/lifecycle/institutional_transition_tests.rs:281](../../../packages/engine/src/experience/feedback/lifecycle/institutional_transition_tests.rs#L281) |
| R029 | `experience::feedback::lifecycle::institutional_transition_tests::institutional_fill_accepts_epoch_without_legacy_and_preserves_closed_legacy_rows` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [packages/engine/src/experience/feedback/lifecycle/institutional_transition_tests.rs:348](../../../packages/engine/src/experience/feedback/lifecycle/institutional_transition_tests.rs#L348) |
| R030 | `experience::price_memory::protection_tests::price_memory_rejection_and_read_overflow_preserve_existing_failure_surface` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/experience/price_memory.rs:206](../../../packages/engine/src/experience/price_memory.rs#L206) |
| R031 | `experience::retention::tests::selection_caps_only_unprotected_contacts_and_breaks_ties_by_code` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/experience/retention.rs:46](../../../packages/engine/src/experience/retention.rs#L46) |
| R032 | `experience::watchlist::protection_tests::watchlist_protection_exceeds_cap_and_ties_keep_larger_codes` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/experience/watchlist.rs:180](../../../packages/engine/src/experience/watchlist.rs#L180) |
| R033 | `experience::watchlist::protection_tests::public_history_touch_does_not_change_watchlist_attention_eviction` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/experience/watchlist.rs:204](../../../packages/engine/src/experience/watchlist.rs#L204) |
| R034 | `market::price_limit_state_tests::symbolic_limit_prices_resolve_at_the_current_authoritative_boundary` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/market.rs:503](../../../packages/engine/src/market.rs#L503) |
| R035 | `market::price_limit_state_tests::price_limit_overflow_is_an_explicit_error_not_a_panic` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/market.rs:546](../../../packages/engine/src/market.rs#L546) |
| R036 | `market::price_limit_state_tests::legal_limit_order_prices_keep_the_wider_ten_tick_cage_and_propagate_errors` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/market.rs:553](../../../packages/engine/src/market.rs#L553) |
| R037 | `verification_evidence::phase_timing::tests::phase_timing_identity_keeps_rank_index_names_and_tick_mapping` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/verification_evidence/phase_timing_tests.rs:252](../../../packages/engine/src/verification_evidence/phase_timing_tests.rs#L252) |
| R038 | `verification_evidence::phase_timing::tests::phase_timing_sample_overflow_keeps_previous_bounds` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/verification_evidence/phase_timing_tests.rs:286](../../../packages/engine/src/verification_evidence/phase_timing_tests.rs#L286) |
| R039 | `verification_evidence::phase_timing::tests::phase_timing_ledger_preserves_overflow_order_and_partial_samples` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/verification_evidence/phase_timing_tests.rs:305](../../../packages/engine/src/verification_evidence/phase_timing_tests.rs#L305) |
| R040 | `verification_evidence::phase_timing::tests::phase_timing_ledger_records_reject_missing_sample_after_precommit_spans` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/verification_evidence/phase_timing_tests.rs:331](../../../packages/engine/src/verification_evidence/phase_timing_tests.rs#L331) |
| R041 | `session::committable_state_tests::shadow_keeps_the_complete_save_projection_and_resets_facade_hooks` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session.rs:4269](../../../packages/engine/src/session.rs#L4269) |
| R042 | `session::committable_state_tests::state_commit_installs_candidate_facts_and_keeps_authority_hooks` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session.rs:4300](../../../packages/engine/src/session.rs#L4300) |
| R043 | `session::attention::scheduler_tests::insertion_order_does_not_change_due_ids_or_popped_entries` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/attention.rs:463](../../../packages/engine/src/session/attention.rs#L463) |
| R044 | `session::attention::scheduler_tests::stale_due_entries_are_returned_but_only_current_ids_wake_once` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/attention.rs:478](../../../packages/engine/src/session/attention.rs#L478) |
| R045 | `session::candles::candle_book_tests::no_trade_day_is_archived_without_becoming_a_traded_sample` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/candles.rs:363](../../../packages/engine/src/session/candles.rs#L363) |
| R046 | `session::candles::candle_book_tests::active_candle_volume_overflow_stays_explicit` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/candles.rs:377](../../../packages/engine/src/session/candles.rs#L377) |
| R047 | `session::candles::candle_book_tests::active_candle_trade_statistics_overflow_stays_explicit` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/candles.rs:397](../../../packages/engine/src/session/candles.rs#L397) |
| R048 | `session::candles::candle_book_tests::first_trade_replaces_mark_and_commit_archives_once` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/candles.rs:419](../../../packages/engine/src/session/candles.rs#L419) |
| R049 | `session::company_assembly::tests::opening_figures_preserve_both_account_orders_and_yuan_units` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/company_assembly.rs:444](../../../packages/engine/src/session/company_assembly.rs#L444) |
| R050 | `session::company_assembly::tests::opening_figures_default_selection_requires_stock_code_and_share_count` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/company_assembly.rs:491](../../../packages/engine/src/session/company_assembly.rs#L491) |
| R051 | `session::company_assembly::tests::opening_figures_default_row_and_tiny_share_failure_remain_explicit` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/company_assembly.rs:523](../../../packages/engine/src/session/company_assembly.rs#L523) |
| R052 | `session::decision_chain::chain_restructure_tests::root_observation_rejects_tick_minute_and_phase_clock_mismatches` | [decision-chain-short-final-result.json](decision-chain-short-final-result.json) | [packages/engine/src/session/decision_chain.rs:3624](../../../packages/engine/src/session/decision_chain.rs#L3624) |
| R053 | `session::decision_chain::chain_restructure_tests::shared_root_context_keeps_own_facts_when_account_execution_order_changes` | [decision-chain-short-final-result.json](decision-chain-short-final-result.json) | [packages/engine/src/session/decision_chain.rs:3669](../../../packages/engine/src/session/decision_chain.rs#L3669) |
| R054 | `session::decision_chain::personal_state::participant_owner_tests::moving_one_participant_keeps_its_four_fields_and_other_accounts_isolated` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/decision_chain/personal_state.rs:139](../../../packages/engine/src/session/decision_chain/personal_state.rs#L139) |
| R055 | `session::decision_chain::personal_state::participant_owner_tests::aggregate_preserves_the_four_independent_save_maps_after_restore` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/decision_chain/personal_state.rs:202](../../../packages/engine/src/session/decision_chain/personal_state.rs#L202) |
| R056 | `session::decision_chain::personal_state::participant_owner_tests::duplicate_personal_install_still_checks_attention_first` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/decision_chain/personal_state.rs:231](../../../packages/engine/src/session/decision_chain/personal_state.rs#L231) |
| R057 | `session::decision_chain::personal_state::participant_owner_tests::duplicate_participant_install_still_reports_watchlist_after_attention` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/decision_chain/personal_state.rs:244](../../../packages/engine/src/session/decision_chain/personal_state.rs#L244) |
| R058 | `session::decision_chain::personal_state::duplicate_install_state_tests::duplicate_participant_install_keeps_the_original_partial_write_boundary` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/decision_chain/personal_state.rs:259](../../../packages/engine/src/session/decision_chain/personal_state.rs#L259) |
| R059 | `session::execution::parent_transition_tests::only_matching_settled_child_advances_and_completion_retains_linked_parent` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/execution.rs:311](../../../packages/engine/src/session/execution.rs#L311) |
| R060 | `session::execution::parent_transition_tests::auction_invalid_transitions_preserve_candidate_parent` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/execution.rs:340](../../../packages/engine/src/session/execution.rs#L340) |
| R061 | `session::execution::parent_transition_tests::continuous_assert_and_auction_checked_fill_keep_distinct_failure_boundaries` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/execution.rs:365](../../../packages/engine/src/session/execution.rs#L365) |
| R062 | `session::execution::parent_transition_tests::unlinked_parent_reverse_rebuilds_execution_from_the_new_target` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [packages/engine/src/session/execution.rs:399](../../../packages/engine/src/session/execution.rs#L399) |
| R063 | `session::execution::parent_transition_tests::closing_auction_never_duplicates_a_live_continuous_child_and_expiry_stops_intents` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/execution.rs:484](../../../packages/engine/src/session/execution.rs#L484) |
| R064 | `session::execution::parent_transition_tests::same_side_revision_keeps_target_and_sub_lot_buy_remaining_unsubmitted` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/execution.rs:519](../../../packages/engine/src/session/execution.rs#L519) |
| R065 | `session::persistence::save_validation_context_preserves_phase_boundaries_and_shared_domain_facts` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/persistence.rs:116](../../../packages/engine/src/session/persistence.rs#L116) |
| R066 | `session::persistence::save_validation_context_preserves_first_error_before_domain_checks` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/persistence.rs:167](../../../packages/engine/src/session/persistence.rs#L167) |
| R067 | `session::persistence::v2_tests::cumulative_fee_audit_v2_buyer_requires_every_nominal_component` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/persistence/v2_tests.rs:1313](../../../packages/engine/src/session/persistence/v2_tests.rs#L1313) |
| R068 | `session::persistence::v2_tests::cumulative_fee_audit_v2_zero_and_negative_gross_keep_existing_boundaries` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/persistence/v2_tests.rs:1355](../../../packages/engine/src/session/persistence/v2_tests.rs#L1355) |
| R069 | `session::pipeline::adaptive_plan_chain::consumption_tests::consumption_preparation_is_discardable_and_duplicate_precedes_payload_error` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/adaptive_plan_chain.rs:1682](../../../packages/engine/src/session/pipeline/adaptive_plan_chain.rs#L1682) |
| R070 | `session::pipeline::adaptive_plan_chain::consumption_tests::consumption_payload_failure_precedes_a_later_duplicate_in_the_same_round` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/adaptive_plan_chain.rs:1702](../../../packages/engine/src/session/pipeline/adaptive_plan_chain.rs#L1702) |
| R071 | `session::pipeline::continuous_lifecycle_projection::tests::lifecycle_batch_preserves_quantity_and_dependency_first_errors` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/continuous_lifecycle_projection.rs:692](../../../packages/engine/src/session/pipeline/continuous_lifecycle_projection.rs#L692) |
| R072 | `session::pipeline::continuous_lifecycle_projection::tests::lifecycle_batch_rejects_unresolved_dependencies_without_publishing_output` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/continuous_lifecycle_projection.rs:755](../../../packages/engine/src/session/pipeline/continuous_lifecycle_projection.rs#L755) |
| R073 | `session::pipeline::continuous_matching::incremental_continuous_stock_shadow::tests::successful_cancel_cannot_hide_a_remaining_book_ledger_mismatch_at_finish` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/incremental_continuous_stock_shadow_tests.rs:660](../../../packages/engine/src/session/pipeline/incremental_continuous_stock_shadow_tests.rs#L660) |
| R074 | `session::pipeline::continuous_matching::incremental_continuous_stock_shadow::tests::one_stock_worker_failure_invalidates_the_discardable_tick_coordinator` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/incremental_continuous_stock_shadow_tests.rs:790](../../../packages/engine/src/session/pipeline/incremental_continuous_stock_shadow_tests.rs#L790) |
| R075 | `session::pipeline::continuous_matching::incremental_continuous_stock_shadow::tests::two_stock_worker_errors_select_first_stock_under_reversed_delivery` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/incremental_continuous_stock_shadow_tests.rs:858](../../../packages/engine/src/session/pipeline/incremental_continuous_stock_shadow_tests.rs#L858) |
| R076 | `session::pipeline::continuous_matching::incremental_continuous_stock_shadow::tests::consuming_stock_finish_freezes_depth_before_day_end_and_emits_one_release` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/incremental_continuous_stock_shadow_tests.rs:1360](../../../packages/engine/src/session/pipeline/incremental_continuous_stock_shadow_tests.rs#L1360) |
| R077 | `session::pipeline::continuous_tick_finalizer::lifecycle_tests::empty_day_end_release_projection_has_no_order_outbox` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/continuous_tick_finalizer.rs:469](../../../packages/engine/src/session/pipeline/continuous_tick_finalizer.rs#L469) |
| R078 | `session::pipeline::continuous_tick_finalizer::lifecycle_tests::day_end_release_projection_orders_envelopes_and_keeps_partial_candidate_on_index_error` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/continuous_tick_finalizer.rs:480](../../../packages/engine/src/session/pipeline/continuous_tick_finalizer.rs#L480) |
| R079 | `session::pipeline::local_admission::tests::cash_and_different_stock_share_lanes_have_independent_roots` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/local_admission.rs:608](../../../packages/engine/src/session/pipeline/local_admission.rs#L608) |
| R080 | `session::pipeline::local_admission::tests::damaged_dependency_cycle_is_reported_by_gates_and_layout` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/local_admission.rs:635](../../../packages/engine/src/session/pipeline/local_admission.rs#L635) |
| R081 | `session::pipeline::local_admission::tests::damaged_atomic_dependency_count_is_reported` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/local_admission.rs:658](../../../packages/engine/src/session/pipeline/local_admission.rs#L658) |
| R082 | `session::pipeline::local_admission::tests::mixed_request_phases_keep_cash_receipts_when_layout_is_reversed` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/local_admission.rs:680](../../../packages/engine/src/session/pipeline/local_admission.rs#L680) |
| R083 | `session::pipeline::local_admission::tests::plan_receipt_scan_keeps_progress_before_later_validation_error` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/local_admission.rs:714](../../../packages/engine/src/session/pipeline/local_admission.rs#L714) |
| R084 | `session::pipeline::quote_expiry::ownership_tests::lifecycle_book_removal_uses_all_three_identity_fields_and_due_keeps_order` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/quote_expiry.rs:296](../../../packages/engine/src/session/pipeline/quote_expiry.rs#L296) |
| R085 | `session::pipeline::quote_expiry::ownership_tests::lifecycle_book_duplicate_order_is_global_across_account_and_stock` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/quote_expiry.rs:331](../../../packages/engine/src/session/pipeline/quote_expiry.rs#L331) |
| R086 | `session::pipeline::quote_expiry::ownership_tests::expiry_release_aggregation_keeps_resource_units_and_failed_append_boundary` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/quote_expiry.rs:348](../../../packages/engine/src/session/pipeline/quote_expiry.rs#L348) |
| R087 | `session::pipeline::stock_auction::auction_day_end::auction_refactor_tests::checked_auction_boundary_distinguishes_opening_rollover_and_closing_day_end` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/auction_refactor_tests.rs:16](../../../packages/engine/src/session/pipeline/auction_refactor_tests.rs#L16) |
| R088 | `session::pipeline::stock_auction::auction_day_end::auction_refactor_tests::checked_auction_boundary_preserves_phase_tick_and_day_first_errors` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/auction_refactor_tests.rs:31](../../../packages/engine/src/session/pipeline/auction_refactor_tests.rs#L31) |
| R089 | `session::pipeline::stock_auction::auction_day_end::auction_refactor_tests::day_end_transition_rejects_each_live_order_family_before_mutating_candidate` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/auction_refactor_tests.rs:52](../../../packages/engine/src/session/pipeline/auction_refactor_tests.rs#L52) |
| R090 | `session::pipeline::stock_auction::auction_day_end::auction_refactor_tests::day_end_transition_overflow_keeps_original_candidate_partial_progress` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/auction_refactor_tests.rs:100](../../../packages/engine/src/session/pipeline/auction_refactor_tests.rs#L100) |
| R091 | `session::pipeline::stock_auction::auction_day_end::auction_refactor_tests::auction_lifecycle_duplicate_identity_does_not_commit_projection_buffers` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/auction_refactor_tests.rs:132](../../../packages/engine/src/session/pipeline/auction_refactor_tests.rs#L132) |
| R092 | `session::pipeline::stock_auction::auction_day_end::auction_refactor_tests::auction_lifecycle_rejects_zero_and_regressing_fill_before_committing_buffers` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/auction_refactor_tests.rs:161](../../../packages/engine/src/session/pipeline/auction_refactor_tests.rs#L161) |
| R093 | `session::pipeline::stock_auction::auction_day_end::auction_refactor_tests::auction_lifecycle_child_quantity_mismatch_discards_pending_parent_changes` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/auction_refactor_tests.rs:209](../../../packages/engine/src/session/pipeline/auction_refactor_tests.rs#L209) |
| R094 | `session::pipeline::stock_auction::auction_day_end::auction_refactor_tests::day_end_transition_is_phase_independent_and_publishes_one_closed_candle_boundary` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/auction_refactor_tests.rs:269](../../../packages/engine/src/session/pipeline/auction_refactor_tests.rs#L269) |
| R095 | `session::pipeline::stock_auction::auction_day_end::auction_refactor_tests::day_end_transition_plan_failure_precedes_day_overflow_and_leaves_authority_untouched` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/auction_refactor_tests.rs:297](../../../packages/engine/src/session/pipeline/auction_refactor_tests.rs#L297) |
| R096 | `session::pipeline::stock_stream::tests::stock_notification_without_payload_is_an_explicit_error` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/stock_stream.rs:549](../../../packages/engine/src/session/pipeline/stock_stream.rs#L549) |
| R097 | `session::pipeline::stock_stream::tests::disconnected_notification_receiver_is_an_explicit_error` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/stock_stream.rs:571](../../../packages/engine/src/session/pipeline/stock_stream.rs#L571) |
| R098 | `session::pipeline::stock_stream::tests::stale_root_notification_does_not_take_stock_payload` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/stock_stream.rs:587](../../../packages/engine/src/session/pipeline/stock_stream.rs#L587) |
| R099 | `session::pipeline::stock_stream::tests::duplicate_stock_completion_is_an_explicit_error` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/stock_stream.rs:613](../../../packages/engine/src/session/pipeline/stock_stream.rs#L613) |
| R100 | `session::pipeline::stock_stream::tests::disconnected_stock_payload_receiver_is_an_explicit_error` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/stock_stream.rs:635](../../../packages/engine/src/session/pipeline/stock_stream.rs#L635) |
| R101 | `session::pipeline::stock_stream::tests::dropping_coordinator_closes_worker_payload_receiver` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/stock_stream.rs:651](../../../packages/engine/src/session/pipeline/stock_stream.rs#L651) |
| R102 | `session::pipeline::transition::seller_charge_tests::seller_charge_allocation_caps_at_each_component_boundary` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/transition.rs:298](../../../packages/engine/src/session/pipeline/transition.rs#L298) |
| R103 | `session::pipeline::transition::seller_charge_tests::seller_charge_allocation_rejects_component_excess_and_total_overflow` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/transition.rs:333](../../../packages/engine/src/session/pipeline/transition.rs#L333) |
| R104 | `session::pipeline::account_settlement_tests::institutional_fill_preserves_same_order_failure_and_rejects_unknown_fee_history_atomically` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/account_settlement_tests.rs:259](../../../packages/engine/src/session/pipeline/account_settlement_tests.rs#L259) |
| R105 | `session::pipeline::account_settlement_tests::institution_fill_updates_only_belief_book_trade_facts_and_is_idempotent` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/account_settlement_tests.rs:349](../../../packages/engine/src/session/pipeline/account_settlement_tests.rs#L349) |
| R106 | `session::pipeline::account_settlement_tests::institutional_exit_profit_requires_net_cash_after_actual_fees` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/account_settlement_tests.rs:442](../../../packages/engine/src/session/pipeline/account_settlement_tests.rs#L442) |
| R107 | `session::pipeline::account_settlement_tests::settlement_plan_account_failure_preserves_all_four_transaction_containers` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/account_settlement_tests.rs:1067](../../../packages/engine/src/session/pipeline/account_settlement_tests.rs#L1067) |
| R108 | `session::pipeline::account_validation_driver_tests::account_validation_independent_share_reservations_of_one_account_use_two_workers` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/account_validation_driver_tests.rs:4](../../../packages/engine/src/session/pipeline/account_validation_driver_tests.rs#L4) |
| R109 | `session::pipeline::account_validation_driver_tests::account_validation_driver_fatal_is_atomic_and_the_same_sealed_slot_can_be_retried` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/account_validation_driver_tests.rs:270](../../../packages/engine/src/session/pipeline/account_validation_driver_tests.rs#L270) |
| R110 | `session::pipeline::continuous_matching_tests::fill_receipts_loads_only_the_incoming_order_and_traded_maker` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/continuous_matching_tests.rs:1010](../../../packages/engine/src/session/pipeline/continuous_matching_tests.rs#L1010) |
| R111 | `session::pipeline::continuous_matching_tests::symbolic_resolution_and_two_makers_keep_source_local_fill_ordinals` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/continuous_matching_tests.rs:1737](../../../packages/engine/src/session/pipeline/continuous_matching_tests.rs#L1737) |
| R112 | `session::pipeline::continuous_tick_finalizer_tests::day_end_release_terminates_the_causal_lifecycle` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/continuous_tick_finalizer_tests.rs:114](../../../packages/engine/src/session/pipeline/continuous_tick_finalizer_tests.rs#L114) |
| R113 | `session::pipeline::continuous_tick_finalizer_tests::day_end_causal_facts_keep_the_completed_continuous_phase` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/continuous_tick_finalizer_tests.rs:193](../../../packages/engine/src/session/pipeline/continuous_tick_finalizer_tests.rs#L193) |
| R114 | `session::pipeline::continuous_tick_finalizer_tests::day_end_quotes_each_cleared_stock_and_skips_untouched_stocks` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/continuous_tick_finalizer_tests.rs:251](../../../packages/engine/src/session/pipeline/continuous_tick_finalizer_tests.rs#L251) |
| R115 | `session::pipeline::decision_snapshot_capture_tests::capture_held_retail_positions_share_equity_peaks_and_t1_with_experience` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/decision_snapshot_capture_tests.rs:362](../../../packages/engine/src/session/pipeline/decision_snapshot_capture_tests.rs#L362) |
| R116 | `session::pipeline::decision_snapshot_capture_tests::capture_non_retail_experience_keeps_self_positions_without_retail_risk` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/decision_snapshot_capture_tests.rs:442](../../../packages/engine/src/session/pipeline/decision_snapshot_capture_tests.rs#L442) |
| R117 | `session::pipeline::decision_snapshot_capture_tests::capture_missing_position_market_does_not_update_the_source_session` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/decision_snapshot_capture_tests.rs:475](../../../packages/engine/src/session/pipeline/decision_snapshot_capture_tests.rs#L475) |
| R118 | `session::pipeline::decision_snapshot_capture_tests::capture_equity_overflow_does_not_install_observed_experience` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/decision_snapshot_capture_tests.rs:500](../../../packages/engine/src/session/pipeline/decision_snapshot_capture_tests.rs#L500) |
| R119 | `session::pipeline::decision_snapshot_capture_tests::capture_self_view_money_error_precedes_risk_and_missing_strategy` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/decision_snapshot_capture_tests.rs:523](../../../packages/engine/src/session/pipeline/decision_snapshot_capture_tests.rs#L523) |
| R120 | `session::pipeline::decision_snapshot_capture_tests::capture_risk_error_precedes_missing_strategy` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/decision_snapshot_capture_tests.rs:554](../../../packages/engine/src/session/pipeline/decision_snapshot_capture_tests.rs#L554) |
| R121 | `session::pipeline::decision_snapshot_capture_tests::capture_mixed_working_orders_only_restore_cash_for_the_cancelable_subset` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/decision_snapshot_capture_tests.rs:568](../../../packages/engine/src/session/pipeline/decision_snapshot_capture_tests.rs#L568) |
| R122 | `session::pipeline::decision_snapshot_tests::decision_snapshot_accepts_non_retail_experience_without_retail_risk` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/decision_snapshot_tests.rs:297](../../../packages/engine/src/session/pipeline/decision_snapshot_tests.rs#L297) |
| R123 | `session::pipeline::retail_projection_tests::retail_order_average_truncates_fractional_cents_without_changing_gross` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/retail_projection_tests.rs:659](../../../packages/engine/src/session/pipeline/retail_projection_tests.rs#L659) |
| R124 | `session::pipeline::retail_projection_tests::retail_multiple_stock_fills_prune_only_unheld_watchlist_entries` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/retail_projection_tests.rs:682](../../../packages/engine/src/session/pipeline/retail_projection_tests.rs#L682) |
| R125 | `session::pipeline::retail_projection_tests::institutional_exit_missing_actual_fee_history_returns_no_patch` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/retail_projection_tests.rs:744](../../../packages/engine/src/session/pipeline/retail_projection_tests.rs#L744) |
| R126 | `session::pipeline::retail_projection_tests::retail_sell_beyond_the_running_position_returns_no_patch` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/retail_projection_tests.rs:786](../../../packages/engine/src/session/pipeline/retail_projection_tests.rs#L786) |
| R127 | `session::pipeline::retail_projection_tests::retail_zero_quantity_fill_reports_non_positive_gross` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/retail_projection_tests.rs:812](../../../packages/engine/src/session/pipeline/retail_projection_tests.rs#L812) |
| R128 | `session::pipeline::session_execution_transaction_tests::institutional_fill_updates_belief_and_preserves_participant_members_and_other_account` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/session_execution_transaction_tests.rs:89](../../../packages/engine/src/session/pipeline/session_execution_transaction_tests.rs#L89) |
| R129 | `session::pipeline::settlement_tests::settlement_plan_ignores_zero_quantity_fill_before_gross_and_fee_checks` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/settlement_tests.rs:599](../../../packages/engine/src/session/pipeline/settlement_tests.rs#L599) |
| R130 | `session::pipeline::settlement_tests::settlement_plan_later_malformed_receipt_does_not_apply_earlier_fill` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/settlement_tests.rs:612](../../../packages/engine/src/session/pipeline/settlement_tests.rs#L612) |
| R131 | `session::pipeline::settlement_tests::settlement_plan_accumulates_each_actual_seller_fee_component` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/pipeline/settlement_tests.rs:633](../../../packages/engine/src/session/pipeline/settlement_tests.rs#L633) |
| R132 | `session::plan_chain_candidates::adaptive::tests::root_coordinator_empty_poll_does_not_take_personal_state` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/plan_chain_candidates/adaptive_tests.rs:307](../../../packages/engine/src/session/plan_chain_candidates/adaptive_tests.rs#L307) |
| R133 | `session::plan_chain_candidates::adaptive::tests::root_coordinator_disconnected_worker_is_explicit_failure` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/plan_chain_candidates/adaptive_tests.rs:320](../../../packages/engine/src/session/plan_chain_candidates/adaptive_tests.rs#L320) |
| R134 | `session::plan_chain_candidates::adaptive::tests::stock_route_followups_consume_reconsideration_before_retry_without_erasing_it` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/plan_chain_candidates/adaptive_tests.rs:342](../../../packages/engine/src/session/plan_chain_candidates/adaptive_tests.rs#L342) |
| R135 | `session::plan_chain_candidates::adaptive::tests::stock_route_wrong_generation_does_not_consume_pending_route` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/plan_chain_candidates/adaptive_tests.rs:365](../../../packages/engine/src/session/plan_chain_candidates/adaptive_tests.rs#L365) |
| R136 | `session::plan_chain_candidates::adaptive::tests::lifecycle_uses_fixed_account_resources_and_current_plan_after_outcome` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/plan_chain_candidates/adaptive_tests.rs:380](../../../packages/engine/src/session/plan_chain_candidates/adaptive_tests.rs#L380) |
| R137 | `session::protocol::civil::session::rollback_tests::publication_cursor_appends_at_same_tick_and_resets_on_next_tick` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/protocol/civil/session.rs:376](../../../packages/engine/src/session/protocol/civil/session.rs#L376) |
| R138 | `session::protocol::civil::session::rollback_tests::checkpoint_restores_published_runtime_after_a_successful_batch` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/protocol/civil/session.rs:414](../../../packages/engine/src/session/protocol/civil/session.rs#L414) |
| R139 | `session::protocol::civil::session::rollback_tests::checkpoint_restores_prior_candidates_and_frozen_save_after_two_civil_updates` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/protocol/civil/session.rs:460](../../../packages/engine/src/session/protocol/civil/session.rs#L460) |
| R140 | `session::snapshot::tests::snapshot_reservations_combine_continuous_auction_and_partial_orders` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/snapshot.rs:237](../../../packages/engine/src/session/snapshot.rs#L237) |
| R141 | `session::snapshot::tests::live_order_reservations_take_is_isolated_and_empty_accounts_have_zero` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/snapshot.rs:304](../../../packages/engine/src/session/snapshot.rs#L304) |
| R142 | `session::snapshot::tests::live_order_reservations_sell_overflow_is_explicit` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/snapshot.rs:346](../../../packages/engine/src/session/snapshot.rs#L346) |
| R143 | `session::snapshot::tests::live_order_reservations_cash_overflow_is_explicit` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/session/snapshot.rs:365](../../../packages/engine/src/session/snapshot.rs#L365) |
| R144 | `diagnostics::tests::retail_ledger_partial_fills_keep_canceled_aborted_and_open_shares_distinct` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/diagnostics.rs:1722](../../../packages/engine/src/diagnostics.rs#L1722) |
| R145 | `diagnostics::tests::retail_ledger_rejects_unknown_orders_and_preserves_overfill_failure_write` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/diagnostics.rs:1750](../../../packages/engine/src/diagnostics.rs#L1750) |
| R146 | `diagnostics::tests::seed_projection_keeps_empty_book_and_day_boundary_no_trade_statistics` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/diagnostics.rs:1820](../../../packages/engine/src/diagnostics.rs#L1820) |
| R147 | `diagnostics::tests::seed_projection_preserves_partial_trade_counters_before_preopen_error` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/diagnostics.rs:1892](../../../packages/engine/src/diagnostics.rs#L1892) |
| R148 | `diagnostics::tests::seed_projection_unknown_taker_preserves_only_the_known_maker_participation` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/diagnostics.rs:1928](../../../packages/engine/src/diagnostics.rs#L1928) |
| R149 | `diagnostics::tests::participant_execution_reconciliation_rejects_missing_side_or_profile_volume` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/diagnostics.rs:1962](../../../packages/engine/src/diagnostics.rs#L1962) |
| R150 | `diagnostics::causal::tests::collector_decision_batch_keeps_empty_and_non_decision_acceptance` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/diagnostics/causal.rs:222](../../../packages/engine/src/diagnostics/causal.rs#L222) |
| R151 | `diagnostics::causal::tests::collector_continuous_overflow_appends_without_advancing_value` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/diagnostics/causal.rs:245](../../../packages/engine/src/diagnostics/causal.rs#L245) |
| R152 | `diagnostics::causal::tests::collector_auction_arithmetic_failure_precedes_zero_entry_and_chain_check` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/diagnostics/causal.rs:264](../../../packages/engine/src/diagnostics/causal.rs#L264) |
| R153 | `diagnostics::causal::tests::collector_auction_chain_failure_keeps_entry_zero_without_appending` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/diagnostics/causal.rs:285](../../../packages/engine/src/diagnostics/causal.rs#L285) |
| R154 | `diagnostics::causal::aggregate::tests::report_builder_reconciles_bilateral_gross_and_missing_quote_reason` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/diagnostics/causal/aggregate.rs:417](../../../packages/engine/src/diagnostics/causal/aggregate.rs#L417) |
| R155 | `diagnostics::causal::aggregate::tests::report_builder_rejects_duplicate_fill_and_preserves_partial_overfill_projection` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/diagnostics/causal/aggregate.rs:438](../../../packages/engine/src/diagnostics/causal/aggregate.rs#L438) |
| R156 | `diagnostics::causal::aggregate::tests::report_builder_rejects_terminal_market_or_civil_time_regression` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/diagnostics/causal/aggregate.rs:465](../../../packages/engine/src/diagnostics/causal/aggregate.rs#L465) |
| R157 | `diagnostics::causal::aggregate::tests::report_builder_keeps_sequence_and_budget_error_precedence` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/diagnostics/causal/aggregate.rs:491](../../../packages/engine/src/diagnostics/causal/aggregate.rs#L491) |
| R158 | `diagnostics::causal::aggregate::tests::report_builder_absence_and_restore_are_explicit` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/diagnostics/causal/aggregate.rs:513](../../../packages/engine/src/diagnostics/causal/aggregate.rs#L513) |
| R159 | `diagnostics::causal::microstructure::tests::direction_persistence_keeps_each_stock_chain_and_ignores_auction_direction` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/diagnostics/causal/microstructure.rs:189](../../../packages/engine/src/diagnostics/causal/microstructure.rs#L189) |
| R160 | `diagnostics::causal::microstructure::tests::quote_response_uses_first_later_valid_quote_and_includes_current_recovery` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/diagnostics/causal/microstructure.rs:230](../../../packages/engine/src/diagnostics/causal/microstructure.rs#L230) |
| R161 | `plans::candidates::targets::proposal_tests::proposal_caps_non_board_lot_share_limit_before_rounding` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/plans/candidates/targets.rs:155](../../../packages/engine/src/plans/candidates/targets.rs#L155) |
| R162 | `plans::candidates::targets::proposal_tests::proposal_preserves_cash_and_exact_board_lot_targets` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/plans/candidates/targets.rs:175](../../../packages/engine/src/plans/candidates/targets.rs#L175) |
| R163 | `plans::candidates::targets::proposal_tests::proposal_rejects_invalid_weight_before_invalid_quantity_inputs` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/plans/candidates/targets.rs:193](../../../packages/engine/src/plans/candidates/targets.rs#L193) |
| R164 | `plans::state::owner_tests::review_preview_preserves_authoritative_plan_and_wire_facts` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/plans/state.rs:374](../../../packages/engine/src/plans/state.rs#L374) |
| R165 | `accounting::closing::tests::restatement_rows_preserve_overwrite_empty_and_scope_semantics` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/accounting/closing/mod.rs:426](../../../packages/engine/src/accounting/closing/mod.rs#L426) |
| R166 | `accounting::consolidation::sale::tests::validated_sale_keeps_account_error_first_and_half_even_zero_lines` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/accounting/consolidation/sale.rs:171](../../../packages/engine/src/accounting/consolidation/sale.rs#L171) |
| R167 | `accounting::fixed_assets::tests::impairment_keeps_life_and_salvage_floor` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/accounting/fixed_assets.rs:287](../../../packages/engine/src/accounting/fixed_assets.rs#L287) |
| R168 | `accounting::inventory::tests::receipt_cost_overflow_preserves_quantity_write_order` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/accounting/inventory.rs:290](../../../packages/engine/src/accounting/inventory.rs#L290) |
| R169 | `accounting::reports::balance_sheet::tests::equity_presentation_keeps_standalone_negative_retained` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/accounting/reports/balance_sheet.rs:383](../../../packages/engine/src/accounting/reports/balance_sheet.rs#L383) |
| R170 | `accounting::reports::balance_sheet::tests::equity_presentation_keeps_consolidated_split_and_missing_prior_equity` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/accounting/reports/balance_sheet.rs:409](../../../packages/engine/src/accounting/reports/balance_sheet.rs#L409) |
| R171 | `accounting::reports::notes::tests::classification_keeps_extra_mappings_and_same_target_idempotence` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/accounting/reports/notes.rs:276](../../../packages/engine/src/accounting/reports/notes.rs#L276) |
| R172 | `accounting::reports::notes::tests::classification_rejects_missing_code_and_conflicting_targets` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/accounting/reports/notes.rs:299](../../../packages/engine/src/accounting/reports/notes.rs#L299) |
| R173 | `accounting::reports::consolidated_window::tests::member_scan_keeps_multiple_minority_splits_and_no_history` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/accounting/reports/consolidated_window.rs:348](../../../packages/engine/src/accounting/reports/consolidated_window.rs#L348) |
| R174 | `accounting::reports::window::tests::current_worksheet_keeps_cash_and_prior_buckets_untouched` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/accounting/reports/window.rs:344](../../../packages/engine/src/accounting/reports/window.rs#L344) |
| R175 | `accounting::reports::window::tests::worksheet_overflow_keeps_negation_and_bucket_error_order` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/accounting/reports/window.rs:365](../../../packages/engine/src/accounting/reports/window.rs#L365) |
| R176 | `accounting::tax::tests::policy_methods_preserve_two_roundings_and_unvalidated_rates` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/accounting/tax.rs:230](../../../packages/engine/src/accounting/tax.rs#L230) |
| R177 | `accounting::tax::tests::policy_compute_keeps_pool_unchanged_and_inclusive_expiry` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/accounting/tax.rs:249](../../../packages/engine/src/accounting/tax.rs#L249) |
| R178 | `company::bank::behavior_tests::opening_policy_precedes_first_forbidden_account` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/bank/behavior_tests.rs:73](../../../packages/engine/src/company/bank/behavior_tests.rs#L73) |
| R179 | `company::bank::behavior_tests::deposit_partial_withdrawal_keeps_residual_and_maturity_cutoff` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/bank/behavior_tests.rs:96](../../../packages/engine/src/company/bank/behavior_tests.rs#L96) |
| R180 | `company::bank::behavior_tests::deposit_payment_failure_preserves_residual_flow_and_event` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/bank/behavior_tests.rs:128](../../../packages/engine/src/company/bank/behavior_tests.rs#L128) |
| R181 | `company::bank::behavior_tests::ecl_sums_scenarios_before_single_half_even_rounding` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/bank/behavior_tests.rs:166](../../../packages/engine/src/company/bank/behavior_tests.rs#L166) |
| R182 | `company::bank::behavior_tests::ecl_validation_precedes_unknown_loan_and_overflow_does_not_commit` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/bank/behavior_tests.rs:194](../../../packages/engine/src/company/bank/behavior_tests.rs#L194) |
| R183 | `company::bank::behavior_tests::restored_initial_policy_is_not_revalidated_during_issue` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/bank/behavior_tests.rs:229](../../../packages/engine/src/company/bank/behavior_tests.rs#L229) |
| R184 | `company::bank::behavior_tests::loan_collection_and_recovery_keep_first_errors_and_exact_limits` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/bank/behavior_tests.rs:253](../../../packages/engine/src/company/bank/behavior_tests.rs#L253) |
| R185 | `company::bank::behavior_tests::loan_same_day_and_written_off_accrual_keep_zero_amount_progress` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/bank/behavior_tests.rs:300](../../../packages/engine/src/company/bank/behavior_tests.rs#L300) |
| R186 | `company::bank::behavior_tests::recovery_overflow_retains_existing_post_then_partial_apply_order` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/bank/behavior_tests.rs:335](../../../packages/engine/src/company/bank/behavior_tests.rs#L335) |
| R187 | `company::industrial::ownership_tests::opening_first_error_keeps_seed_counterparty_debt_order` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/industrial/ownership_tests.rs:58](../../../packages/engine/src/company/industrial/ownership_tests.rs#L58) |
| R188 | `company::industrial::ownership_tests::positive_days_zero_interest_keeps_carry_date_and_event_gap` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/industrial/ownership_tests.rs:114](../../../packages/engine/src/company/industrial/ownership_tests.rs#L114) |
| R189 | `company::industrial::ownership_tests::rejected_repayment_keeps_preview_uncommitted_and_serialized_shape` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/industrial/ownership_tests.rs:149](../../../packages/engine/src/company/industrial/ownership_tests.rs#L149) |
| R190 | `company::industrial::ownership_tests::repeated_annual_tax_keeps_existing_non_idempotent_loss_behavior` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/industrial/ownership_tests.rs:181](../../../packages/engine/src/company/industrial/ownership_tests.rs#L181) |
| R191 | `company::industrial::ownership_tests::zero_line_tax_still_commits_expired_loss_pool` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/industrial/ownership_tests.rs:199](../../../packages/engine/src/company/industrial/ownership_tests.rs#L199) |
| R192 | `company::industrial::ownership_tests::failed_tax_post_keeps_loss_pool_and_event_id_unchanged` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/industrial/ownership_tests.rs:211](../../../packages/engine/src/company/industrial/ownership_tests.rs#L211) |
| R193 | `company::industrial::ownership_tests::opening_implicit_debt_roundtrip_keeps_short_account_and_credit_usage` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/industrial/ownership_tests.rs:229](../../../packages/engine/src/company/industrial/ownership_tests.rs#L229) |
| R194 | `company::insurance::behavior_tests::opening_guard_keeps_discount_precedence_and_first_seeded_account` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/insurance/behavior_tests.rs:56](../../../packages/engine/src/company/insurance/behavior_tests.rs#L56) |
| R195 | `company::insurance::behavior_tests::multiple_partial_payments_exhaust_claim_then_reject_one_cent` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/insurance/behavior_tests.rs:77](../../../packages/engine/src/company/insurance/behavior_tests.rs#L77) |
| R196 | `company::insurance::behavior_tests::flat_group_snapshot_keeps_required_duplicate_and_unknown_field_behavior` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/insurance/behavior_tests.rs:104](../../../packages/engine/src/company/insurance/behavior_tests.rs#L104) |
| R197 | `company::insurance::behavior_tests::restored_remeasurement_and_release_keep_all_tail_components_and_carries` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/insurance/behavior_tests.rs:157](../../../packages/engine/src/company/insurance/behavior_tests.rs#L157) |
| R198 | `company::insurance::behavior_tests::zero_remeasurement_consumes_one_event_slot_without_group_writes` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/insurance/behavior_tests.rs:215](../../../packages/engine/src/company/insurance/behavior_tests.rs#L215) |
| R199 | `company::insurance::behavior_tests::release_apply_overflow_preserves_existing_post_and_partial_write_order` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/insurance/behavior_tests.rs:225](../../../packages/engine/src/company/insurance/behavior_tests.rs#L225) |
| R200 | `company::insurance::behavior_tests::measurement_previews_are_pure_and_keep_exact_release_and_remeasurement_gold` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/insurance/behavior_tests.rs:252](../../../packages/engine/src/company/insurance/behavior_tests.rs#L252) |
| R201 | `company::insurance::behavior_tests::failed_post_leaves_measurement_unchanged_for_release_and_remeasurement` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/insurance/behavior_tests.rs:296](../../../packages/engine/src/company/insurance/behavior_tests.rs#L296) |
| R202 | `company::insurance::behavior_tests::exhausted_csm_keeps_existing_inactive_carry_after_final_release` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/insurance/behavior_tests.rs:313](../../../packages/engine/src/company/insurance/behavior_tests.rs#L313) |
| R203 | `company::insurance::behavior_tests::remeasurement_apply_overflow_preserves_existing_partial_write_order` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/insurance/behavior_tests.rs:329](../../../packages/engine/src/company/insurance/behavior_tests.rs#L329) |
| R204 | `company::operations::day::tests::duration_error_precedes_pair_guard_and_mismatched_dto_deserializes` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/operations/day.rs:448](../../../packages/engine/src/company/operations/day.rs#L448) |
| R205 | `company::operations::day::tests::restored_pair_mismatch_keeps_prior_expiry_sampling_and_due_consumption` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/operations/day.rs:477](../../../packages/engine/src/company/operations/day.rs#L477) |
| R206 | `company::operations::day::tests::restored_spec_kind_is_not_an_extra_daily_pair_guard` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/operations/day.rs:521](../../../packages/engine/src/company/operations/day.rs#L521) |
| R207 | `company::operations::day::tests::shock_records_follow_market_then_sorted_industries_then_sorted_companies` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/operations/day.rs:542](../../../packages/engine/src/company/operations/day.rs#L542) |
| R208 | `company::operations::day::tests::wrong_date_invalidates_projection_before_rejecting_without_business_changes` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/operations/day.rs:613](../../../packages/engine/src/company/operations/day.rs#L613) |
| R209 | `company::operations::day::tests::same_day_maturity_funds_flow_then_only_next_day_interest_is_queued` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/operations/day.rs:671](../../../packages/engine/src/company/operations/day.rs#L671) |
| R210 | `company::operations::day::tests::flow_error_retains_prior_maturity_and_does_not_queue_next_interest` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/operations/day.rs:688](../../../packages/engine/src/company/operations/day.rs#L688) |
| R211 | `company::real_estate::ownership_tests::opening_policy_precedes_first_seeded_account` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/real_estate/ownership_tests.rs:69](../../../packages/engine/src/company/real_estate/ownership_tests.rs#L69) |
| R212 | `company::real_estate::ownership_tests::project_guards_preserve_first_error_and_rejected_state` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/real_estate/ownership_tests.rs:90](../../../packages/engine/src/company/real_estate/ownership_tests.rs#L90) |
| R213 | `company::real_estate::ownership_tests::accrual_tracks_mixed_loans_project_total_and_entry_slots` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/real_estate/ownership_tests.rs:133](../../../packages/engine/src/company/real_estate/ownership_tests.rs#L133) |
| R214 | `company::real_estate::ownership_tests::planning_and_post_failures_do_not_advance_accrual_state` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/real_estate/ownership_tests.rs:170](../../../packages/engine/src/company/real_estate/ownership_tests.rs#L170) |
| R215 | `company::real_estate::ownership_tests::zero_amount_accrual_keeps_both_remainder_chains_and_date` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/real_estate/ownership_tests.rs:209](../../../packages/engine/src/company/real_estate/ownership_tests.rs#L209) |
| R216 | `company::real_estate::ownership_tests::delivery_last_unit_conserves_cost_and_collection_preconditions` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/real_estate/ownership_tests.rs:245](../../../packages/engine/src/company/real_estate/ownership_tests.rs#L245) |
| R217 | `company::real_estate::ownership_tests::owner_guards_are_read_only_and_return_existing_context` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/real_estate/ownership_tests.rs:302](../../../packages/engine/src/company/real_estate/ownership_tests.rs#L302) |
| R218 | `company::real_estate::ownership_tests::segmented_accrual_conserves_capitalized_and_expensed_chains` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/real_estate/ownership_tests.rs:381](../../../packages/engine/src/company/real_estate/ownership_tests.rs#L381) |
| R219 | `company::real_estate::ownership_tests::loan_apply_overflow_preserves_existing_posted_partial_failure` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [packages/engine/src/company/real_estate/ownership_tests.rs:425](../../../packages/engine/src/company/real_estate/ownership_tests.rs#L425) |

### `server/lib/server`

| ID | exact filter（完整语义 key 的第四项） | 最终 proof record | 当前源文件 / 声明行 |
|---|---|---|---|
| R220 | `actor::interval_tests::pacing_keeps_speed_mode_interval_and_pause_metrics_consistent` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [apps/server/src/actor.rs:510](../../../apps/server/src/actor.rs#L510) |
| R221 | `actor::interval_tests::pacing_restore_resets_sampling_but_fatal_stop_preserves_it` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [apps/server/src/actor.rs:553](../../../apps/server/src/actor.rs#L553) |
| R222 | `actor::fatal_tests::fixed_first_step_failure_restores_the_entire_cycle` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [apps/server/src/actor/fatal_tests.rs:217](../../../apps/server/src/actor/fatal_tests.rs#L217) |
| R223 | `actor::fatal_tests::command_burst_keeps_submission_order_after_callers_stop_waiting` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [apps/server/src/actor/fatal_tests.rs:356](../../../apps/server/src/actor/fatal_tests.rs#L356) |
| R224 | `routes::baseline_failure_tests::latched_failure_is_sent_after_an_unchanged_baseline` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [apps/server/src/routes.rs:1547](../../../apps/server/src/routes.rs#L1547) |
| R225 | `routes::publisher_connection_tests::failure_precedes_generation_and_resync_and_latches_after_delivery` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [apps/server/src/routes.rs:1723](../../../apps/server/src/routes.rs#L1723) |
| R226 | `routes::publisher_connection_tests::covered_updates_timeline_changes_and_lag_keep_frame_barrier` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [apps/server/src/routes.rs:1745](../../../apps/server/src/routes.rs#L1745) |
| R227 | `routes::publisher_connection_tests::resync_preparation_clears_buffer_without_mutating_failed_baseline_state` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [apps/server/src/routes.rs:1779](../../../apps/server/src/routes.rs#L1779) |
| R228 | `routes::publisher_connection_tests::flush_actions_leave_incoming_update_unaccepted_until_send_succeeds` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [apps/server/src/routes.rs:1815](../../../apps/server/src/routes.rs#L1815) |
| R229 | `routes::publisher_connection_tests::pull_capacity_clears_backlog_and_get_frame_rejects_push_mode` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [apps/server/src/routes.rs:1845](../../../apps/server/src/routes.rs#L1845) |

### `web-wasm/lib/web_wasm`

| ID | exact filter（完整语义 key 的第四项） | 最终 proof record | 当前源文件 / 声明行 |
|---|---|---|---|
| R230 | `error_layout_tests::step_update_error_keeps_fatal_payload_indirect` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [apps/web-wasm/src/lib.rs:536](../../../apps/web-wasm/src/lib.rs#L536) |
| R231 | `protocol_tests::registry_step_returns_one_valid_frame_and_invalid_handle_is_explicit` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [apps/web-wasm/src/protocol_tests.rs:23](../../../apps/web-wasm/src/protocol_tests.rs#L23) |
| R232 | `protocol_tests::registry_owns_independent_handles_and_failed_construction_does_not_register` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [apps/web-wasm/src/protocol_tests.rs:200](../../../apps/web-wasm/src/protocol_tests.rs#L200) |
| R233 | `protocol_tests::registry_restore_failure_preserves_existing_session_and_civil_barrier` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [apps/web-wasm/src/protocol_tests.rs:238](../../../apps/web-wasm/src/protocol_tests.rs#L238) |

### `stock-market-game/lib/stock_market_game_lib`

| ID | exact filter（完整语义 key 的第四项） | 最终 proof record | 当前源文件 / 声明行 |
|---|---|---|---|
| R234 | `actor::tests::pacing_keeps_speed_mode_interval_and_pause_metrics_consistent` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [apps/desktop/src-tauri/src/actor.rs:1422](../../../apps/desktop/src-tauri/src/actor.rs#L1422) |
| R235 | `actor::tests::pacing_restore_resets_sampling_but_fatal_stop_preserves_it` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [apps/desktop/src-tauri/src/actor.rs:1461](../../../apps/desktop/src-tauri/src/actor.rs#L1461) |
| R236 | `actor::tests::desktop_pacing_rejects_invalid_internal_speed_without_changing_state` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [apps/desktop/src-tauri/src/actor.rs:1482](../../../apps/desktop/src-tauri/src/actor.rs#L1482) |
| R237 | `actor::fatal_tests::fixed_auto_step_emits_one_fatal_and_stops` | [rust-lib-short-01-result.json](rust-lib-short-01-result.json) | [apps/desktop/src-tauri/src/actor/fatal_tests.rs:119](../../../apps/desktop/src-tauri/src/actor/fatal_tests.rs#L119) |

### `engine/test/information_acquisition`

| ID | exact filter（完整语义 key 的第四项） | 最终 proof record | 当前源文件 / 声明行 |
|---|---|---|---|
| R238 | `acquisition_gold::unread_publication_never_enters_unread_npc_inputs` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [packages/engine/tests/information_acquisition/acquisition_gold.rs:65](../../../packages/engine/tests/information_acquisition/acquisition_gold.rs#L65) |
| R239 | `view_gold::acquired_version_pinned_across_later_correction` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [packages/engine/tests/information_acquisition/view_gold.rs:16](../../../packages/engine/tests/information_acquisition/view_gold.rs#L16) |

### `engine/test/attention_discovery`

| ID | exact filter（完整语义 key 的第四项） | 最终 proof record | 当前源文件 / 声明行 |
|---|---|---|---|
| R240 | `exposure::announcement_exposure_enters_weights_only_through_the_public_surface` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [packages/engine/tests/attention_discovery/exposure.rs:152](../../../packages/engine/tests/attention_discovery/exposure.rs#L152) |
| R241 | `failures::discovery::discovery_sampling_never_writes_into_information_state` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [packages/engine/tests/attention_discovery/failures/discovery.rs:150](../../../packages/engine/tests/attention_discovery/failures/discovery.rs#L150) |

### `engine/test/auction`

| ID | exact filter（完整语义 key 的第四项） | 最终 proof record | 当前源文件 / 声明行 |
|---|---|---|---|
| R242 | `auction_fixture_preserves_exchange_identity_and_order_envelope_keys` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [packages/engine/tests/auction.rs:188](../../../packages/engine/tests/auction.rs#L188) |
| R243 | `shanghai_and_shenzhen_apply_their_own_final_auction_tie_breaks` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [packages/engine/tests/auction.rs:520](../../../packages/engine/tests/auction.rs#L520) |

### `engine/test/behavior`

| ID | exact filter（完整语义 key 的第四项） | 最终 proof record | 当前源文件 / 声明行 |
|---|---|---|---|
| R244 | `behavior_scenario_keeps_stock_paths_and_breadth_inputs_together` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [packages/engine/tests/behavior.rs:154](../../../packages/engine/tests/behavior.rs#L154) |
| R245 | `unrelated_market_stocks_do_not_change_a_retailers_target` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [packages/engine/tests/behavior.rs:283](../../../packages/engine/tests/behavior.rs#L283) |

### `engine/test/fundamental_beliefs`

| ID | exact filter（完整语义 key 的第四项） | 最终 proof record | 当前源文件 / 声明行 |
|---|---|---|---|
| R246 | `failures::unacquired_material_is_rejected` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [packages/engine/tests/fundamental_beliefs/failures/mod.rs:71](../../../packages/engine/tests/fundamental_beliefs/failures/mod.rs#L71) |
| R247 | `gold::earnings_multiple_end_to_end_gold` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [packages/engine/tests/fundamental_beliefs/gold.rs:72](../../../packages/engine/tests/fundamental_beliefs/gold.rs#L72) |

### `engine/test/industry_reports`

| ID | exact filter（完整语义 key 的第四项） | 最终 proof record | 当前源文件 / 声明行 |
|---|---|---|---|
| R248 | `correction_restatement::correction_does_not_leak_into_later_periods` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [packages/engine/tests/industry_reports/correction_restatement.rs:135](../../../packages/engine/tests/industry_reports/correction_restatement.rs#L135) |
| R249 | `correction_restatement::restatement_worksheet_survives_serde_round_trip` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [packages/engine/tests/industry_reports/correction_restatement.rs:237](../../../packages/engine/tests/industry_reports/correction_restatement.rs#L237) |

### `engine/test/plans`

| ID | exact filter（完整语义 key 的第四项） | 最终 proof record | 当前源文件 / 声明行 |
|---|---|---|---|
| R250 | `real_fills_reaching_share_target_complete_the_plan` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [packages/engine/tests/plans.rs:352](../../../packages/engine/tests/plans.rs#L352) |
| R251 | `fill_beyond_target_is_rejected` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [packages/engine/tests/plans.rs:568](../../../packages/engine/tests/plans.rs#L568) |

### `engine/test/publications`

| ID | exact filter（完整语义 key 的第四项） | 最终 proof record | 当前源文件 / 声明行 |
|---|---|---|---|
| R252 | `failures::announcement_failures::announcement_timing_rejected` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [packages/engine/tests/publications/failures/announcement_failures.rs:14](../../../packages/engine/tests/publications/failures/announcement_failures.rs#L14) |
| R253 | `failures::publication_failures::correction_link_rejected_on_mismatch` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [packages/engine/tests/publications/failures/publication_failures.rs:147](../../../packages/engine/tests/publications/failures/publication_failures.rs#L147) |

### `engine/test/session`

| ID | exact filter（完整语义 key 的第四项） | 最终 proof record | 当前源文件 / 声明行 |
|---|---|---|---|
| R254 | `order_save_fixture_restores_envelopes_cursors_and_reservations` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [packages/engine/tests/session.rs:336](../../../packages/engine/tests/session.rs#L336) |
| R255 | `continuous_multi_fill_charges_one_minimum_commission_per_account_batch` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [packages/engine/tests/session.rs:2834](../../../packages/engine/tests/session.rs#L2834) |

### `engine/test/account`

| ID | exact filter（完整语义 key 的第四项） | 最终 proof record | 当前源文件 / 声明行 |
|---|---|---|---|
| R256 | `account_new_player_has_no_strategy` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [packages/engine/tests/account.rs:126](../../../packages/engine/tests/account.rs#L126) |
| R257 | `buy_position_overflow_is_rejected_without_debiting_cash` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [packages/engine/tests/account.rs:245](../../../packages/engine/tests/account.rs#L245) |
| R258 | `grant_position_sets_cost_basis` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [packages/engine/tests/account.rs:470](../../../packages/engine/tests/account.rs#L470) |
| R259 | `derived_values_and_grants_report_overflow` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [packages/engine/tests/account.rs:493](../../../packages/engine/tests/account.rs#L493) |

### `engine/test/strategy_state`

| ID | exact filter（完整语义 key 的第四项） | 最终 proof record | 当前源文件 / 声明行 |
|---|---|---|---|
| R260 | `strategy_state_round_trip_preserves_concrete_parameters` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [packages/engine/tests/strategy_state.rs:8](../../../packages/engine/tests/strategy_state.rs#L8) |

### `server/test/deployment_cli`

| ID | exact filter（完整语义 key 的第四项） | 最终 proof record | 当前源文件 / 声明行 |
|---|---|---|---|
| R261 | `unknown_cli_argument_fails_explicitly` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [apps/server/tests/deployment_cli.rs:64](../../../apps/server/tests/deployment_cli.rs#L64) |
| R262 | `invalid_services_fail_explicitly` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [apps/server/tests/deployment_cli.rs:69](../../../apps/server/tests/deployment_cli.rs#L69) |
| R263 | `missing_option_value_fails_explicitly` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [apps/server/tests/deployment_cli.rs:74](../../../apps/server/tests/deployment_cli.rs#L74) |
| R264 | `web_root_cannot_hide_an_unknown_option` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [apps/server/tests/deployment_cli.rs:79](../../../apps/server/tests/deployment_cli.rs#L79) |

### `server/test/deployment_routes`

| ID | exact filter（完整语义 key 的第四项） | 最终 proof record | 当前源文件 / 声明行 |
|---|---|---|---|
| R265 | `head_has_headers_without_body_and_static_post_is_rejected` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [apps/server/tests/deployment_routes.rs:99](../../../apps/server/tests/deployment_routes.rs#L99) |
| R266 | `traversal_and_symlinks_cannot_expose_host_files` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [apps/server/tests/deployment_routes.rs:210](../../../apps/server/tests/deployment_routes.rs#L210) |
| R267 | `startup_requires_valid_root_index_and_nonempty_assets` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [apps/server/tests/deployment_routes.rs:277](../../../apps/server/tests/deployment_routes.rs#L277) |

### `server/test/api_contract`

| ID | exact filter（完整语义 key 的第四项） | 最终 proof record | 当前源文件 / 声明行 |
|---|---|---|---|
| R268 | `settled_api_fixture_reports_creation_failure_with_context` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [apps/server/tests/api_contract.rs:249](../../../apps/server/tests/api_contract.rs#L249) |
| R269 | `settled_api_fixture_reports_unsettled_day_with_context` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [apps/server/tests/api_contract.rs:265](../../../apps/server/tests/api_contract.rs#L265) |
| R270 | `load_rejects_corrupt_body_before_actor_replacement` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [apps/server/tests/api_contract.rs:825](../../../apps/server/tests/api_contract.rs#L825) |

### `server/test/ws`

| ID | exact filter（完整语义 key 的第四项） | 最终 proof record | 当前源文件 / 声明行 |
|---|---|---|---|
| R271 | `server_fixture_shutdown_stops_listener_and_removes_sessions` | [ws-fixture-short-final-result.json](ws-fixture-short-final-result.json) | [apps/server/tests/ws.rs:588](../../../apps/server/tests/ws.rs#L588) |
| R272 | `server_fixture_cleans_resources_after_assertion_panic` | [ws-fixture-short-final-result.json](ws-fixture-short-final-result.json) | [apps/server/tests/ws.rs:605](../../../apps/server/tests/ws.rs#L605) |

### `engine/test/save_contract`

| ID | exact filter（完整语义 key 的第四项） | 最终 proof record | 当前源文件 / 声明行 |
|---|---|---|---|
| R273 | `cached_baseline_clones_do_not_share_tampering` | [rust-short-increment-02-result.json](rust-short-increment-02-result.json) | [packages/engine/tests/save_contract/main.rs:156](../../../packages/engine/tests/save_contract/main.rs#L156) |

### `engine/example/escrow_verification_harness`

| ID | exact filter（完整语义 key 的第四项） | 最终 proof record | 当前源文件 / 声明行 |
|---|---|---|---|
| R274 | `runtime::tests::capture_artifact_receipts_bind_empty_and_large_bytes` | [writer-short-final-result.json](writer-short-final-result.json) | [packages/engine/examples/escrow_verification_harness/runtime.rs:824](../../../packages/engine/examples/escrow_verification_harness/runtime.rs#L824) |
| R275 | `runtime::tests::capture_writer_persists_exact_bytes_and_receipts_and_refuses_existing_output` | [writer-short-final-result.json](writer-short-final-result.json) | [packages/engine/examples/escrow_verification_harness/runtime.rs:927](../../../packages/engine/examples/escrow_verification_harness/runtime.rs#L927) |
| R276 | `runtime::tests::capture_writer_keeps_prior_files_and_stops_later_files_on_partial_write_failure` | [writer-short-final-result.json](writer-short-final-result.json) | [packages/engine/examples/escrow_verification_harness/runtime.rs:964](../../../packages/engine/examples/escrow_verification_harness/runtime.rs#L964) |
| R277 | `runtime::tests::workspace_path_validation_rejects_parent_escape_and_outside_symlink` | [writer-short-final-result.json](writer-short-final-result.json) | [packages/engine/examples/escrow_verification_harness/runtime.rs:1099](../../../packages/engine/examples/escrow_verification_harness/runtime.rs#L1099) |
| R278 | `runtime::tests::workspace_directory_inputs_must_be_absolute` | [writer-short-final-result.json](writer-short-final-result.json) | [packages/engine/examples/escrow_verification_harness/runtime.rs:1151](../../../packages/engine/examples/escrow_verification_harness/runtime.rs#L1151) |

## 128 action 验证映射

映射到 Rust case 的 99 个 action，其列出的 case 已全部通过；27 个 action 由 Web/Node 证据承接；两个旧长周期 fixture 只承接编译与完整 diff 复核。相同 case 可保护多个耦合动作，最终签署仍以总审与 final-validation.json 为准。

| action | 通过的 case IDs / 已验证范围 |
|---|---|
| `engine-foundation-01-A03` | R019, R020, R256, R257, R258, R259 |
| `engine-pipeline-01-A01` | R108, R109 |
| `engine-pipeline-03-A02` | R087, R088 |
| `engine-pipeline-03-A03` | R091, R092, R093 |
| `engine-pipeline-05-A01` | R110, R111 |
| `engine-pipeline-10-A01` | R104, R105, R106 |
| `engine-pipeline-11-A01` | R096, R097, R098, R099, R100, R101 |
| `engine-session-01-A01` | R041, R042 |
| `engine-session-02-A03` | R045, R046, R047, R048 |
| `engine-session-03-A01` | R052, R053 |
| `engine-strategy-01-A01` | R164, R250, R251 |
| `engine-strategy-03-A01` | R018 |
| `hosts-01-A01` | R234, R235 |
| `hosts-01-A02` | R220, R221, R223 |
| `hosts-03-A01` | R231, R232, R233 |
| `tooling-01-A02` | 不属于 Rust 短测 |
| `tooling-01-A01` | R274, R275, R276, R277, R278 |
| `tooling-04-A01` | 不属于 Rust 短测 |
| `tooling-05-A01` | 不属于 Rust 短测 |
| `web-01-A06` | 不属于 Rust 短测 |
| `web-01-A27` | 不属于 Rust 短测 |
| `web-08-A01` | 不属于 Rust 短测 |
| `domain-N01` | R001 |
| `domain-N02` | R002, R005, R006, R007, R008, R009, R010 |
| `domain-N03` | R034, R035, R036 |
| `domain-N04` | R178, R194, R211 |
| `domain-N05` | R174, R175 |
| `domain-N06` | R176, R177, R191, R192 |
| `domain-N07` | R024, R025, R026, R027, R028, R029 |
| `pipeline-N01` | R071, R072 |
| `pipeline-N02` | R087, R088 |
| `session-N01` | R059, R060, R061, R062, R063, R064 |
| `session-N02` | R137, R138, R139 |
| `frontend-N01` | 不属于 Rust 短测 |
| `frontend-N02` | 不属于 Rust 短测 |
| `frontend-N03` | 不属于 Rust 短测 |
| `frontend-N04` | 不属于 Rust 短测 |
| `frontend-N05` | 不属于 Rust 短测 |
| `frontend-N06` | 不属于 Rust 短测 |
| `frontend-N07` | 不属于 Rust 短测 |
| `frontend-N08` | 不属于 Rust 短测 |
| `frontend-N09` | 不属于 Rust 短测 |
| `hosts-N01` | R220, R221, R234, R235, R236 |
| `hosts-N02` | R265, R266, R267 |
| `hosts-N03` | R238, R239 |
| `hosts-N04` | 旧长周期 fixture 未运行；编译与完整 diff 复核承接 |
| `hosts-N05` | R273 |
| `hosts-N06` | R261, R262, R263, R264 |
| `hosts-N07` | 不属于 Rust 短测 |
| `hosts-N08` | 不属于 Rust 短测 |
| `domain-R2-N01` | R165 |
| `domain-R2-N02` | R167 |
| `domain-R2-N03` | R168 |
| `domain-R2-N04` | R166 |
| `domain-R2-N06` | R169, R170 |
| `domain-R2-N07` | R171, R172 |
| `domain-R2-N08` | R173 |
| `domain-R2-N10` | R179, R180 |
| `domain-R2-N11` | R181, R182, R183 |
| `domain-R2-N12` | R184, R185, R186 |
| `domain-R2-N13` | R187, R193 |
| `domain-R2-N14` | R188, R189, R193 |
| `domain-R2-N16` | R190, R191, R192 |
| `domain-R2-N18` | R194, R195, R196, R197, R198, R199, R200, R201, R202, R203 |
| `domain-R2-N19` | R204, R205, R206, R207, R208, R209, R210 |
| `domain-R2-N20` | R204, R205, R206, R207, R208, R209, R210 |
| `domain-R2-N22` | R213, R214, R219 |
| `domain-R2-N25` | R212, R217 |
| `domain-R2-N26` | R215, R217, R218 |
| `domain-R2-N27` | R216, R217 |
| `domain-R2-N28` | R021, R022, R023 |
| `domain-R2-N30` | R144, R145 |
| `domain-R2-N31` | R146, R147, R148, R149 |
| `domain-R2-N32` | R154, R155, R156, R157, R158, R159, R160 |
| `domain-R2-N34` | R030 |
| `domain-R2-N35` | R031, R032, R033 |
| `domain-R2-N37` | R002, R005, R006, R007, R008, R009, R010 |
| `domain-R2-N38` | R003, R004 |
| `domain-R2-N39` | R037, R038, R039, R040 |
| `domain-R2-N40` | R001 |
| `domain-R2-N41` | R150, R151, R152, R153 |
| `pipeline-R2-N01` | R069, R070 |
| `pipeline-R2-N02` | R089, R090, R094, R095 |
| `pipeline-R2-N04` | R077, R078, R112, R113, R114 |
| `pipeline-R2-N05` | R115, R116, R117, R118, R119, R120, R121, R122 |
| `pipeline-R2-N06` | R073, R074, R075, R076 |
| `pipeline-R2-N07` | R102, R103 |
| `pipeline-R2-N08` | R079, R080, R081, R082, R083 |
| `pipeline-R2-N09` | R084, R085 |
| `pipeline-R2-N10` | R086 |
| `pipeline-R2-N11` | R123, R124, R125, R126, R127 |
| `pipeline-R2-N12` | R107, R129, R130, R131 |
| `pipeline-R2-N13` | R121 |
| `session-R2-N01` | R043, R044 |
| `session-R2-N02` | R049, R050, R051 |
| `session-R2-N03` | R136 |
| `session-R2-N04` | R054, R055, R056, R057, R058, R128 |
| `session-R2-N05` | R011, R012, R013 |
| `session-R2-N06` | R065, R066 |
| `session-R2-N07` | R067, R068 |
| `session-R2-N08` | R132, R133 |
| `session-R2-N09` | R134, R135 |
| `session-R2-N10` | R140, R141, R142, R143 |
| `session-R2-N11` | R161, R162, R163 |
| `session-R2-N12` | R014, R015, R016, R017 |
| `frontend-R2-N01` | 不属于 Rust 短测 |
| `frontend-R2-N02` | 不属于 Rust 短测 |
| `frontend-R2-N03` | 不属于 Rust 短测 |
| `frontend-R2-N04` | 不属于 Rust 短测 |
| `hosts-R2-N01` | R240, R241 |
| `hosts-R2-N02` | R242, R243 |
| `hosts-R2-N04` | R244, R245 |
| `hosts-R2-N05` | 旧长周期 fixture 未运行；编译与完整 diff 复核承接 |
| `hosts-R2-N06` | R246, R247 |
| `hosts-R2-N08` | R248, R249 |
| `hosts-R2-N10` | R250, R251 |
| `hosts-R2-N11` | R252, R253 |
| `hosts-R2-N12` | R254, R255 |
| `hosts-R2-N13` | R237 |
| `hosts-R2-N14` | R224, R225, R226, R227, R228, R229 |
| `hosts-R2-N15` | R268, R269, R270 |
| `hosts-R2-N16` | R271, R272 |
| `hosts-R2-N17` | 不属于 Rust 短测 |
| `hosts-R2-N18` | 不属于 Rust 短测 |
| `hosts-R2-N19` | 不属于 Rust 短测 |
| `hosts-R2-N20` | 不属于 Rust 短测 |
| `hosts-R2-N21` | 不属于 Rust 短测 |
| `hosts-R2-N22` | 不属于 Rust 短测 |

## 源记录与版本绑定

本次仅做 JSON 整体解析、source SHA/声明行更新与两项迁移元数据校正，没有重跑编译或测试。完整 key 为 `[package, target_kind, target_name, exact_filter]`，每个 case 的 `final_proof` 同时引用 final-validation.json 和对应最终执行记录。当前记录 SHA 快照见 JSON，不将机械核对称为重新阅读全部源码或独立完整 diff 审查。

两个原 N07 case 的测试定义已经迁入 lifecycle/institutional_transition_tests.rs。final-validation.json 的 `source_metadata_correction` 保留旧路径/SHA与校正原因；据 root 核对，该文件冻结于 03:36:48.761 UTC，早于 build05 的 03:36:52.509 UTC，原 case 已实际执行，因此只校正 source 元数据，不改变 record、结果或耗时。

final-validation.json SHA-256：`0fa50db914b7b04aab8b33adcc65fe5d79ce31aeb7788e1bf4a5998a0df93825`。

- [rust-lib-short-01-result.json](rust-lib-short-01-result.json)：最终选取 230 个唯一 case。
- [decision-chain-short-final-result.json](decision-chain-short-final-result.json)：最终选取 2 个唯一 case。
- [rust-short-increment-02-result.json](rust-short-increment-02-result.json)：最终选取 39 个唯一 case。
- [ws-fixture-short-final-result.json](ws-fixture-short-final-result.json)：最终选取 2 个唯一 case。
- [writer-short-final-result.json](writer-short-final-result.json)：最终选取 5 个唯一 case。
