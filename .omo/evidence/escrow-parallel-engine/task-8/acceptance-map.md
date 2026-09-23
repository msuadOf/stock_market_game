# Task 8 存档 v2 / 恢复验收映射

基线：`94f337eaafd0c3ed4e83a0a85182239c9170885b`。结论：**PASS**。

本表把计划中的 Task 8 验收项映射到真实测试符号。当前 HEAD 的核心 v2 单元组已用
4 个 build jobs、4 个 Rust test threads 完成 29/29 smoke；`save_contract` 14/14 与
`verification_evidence` 33/33 为此前独立整包审查的定向执行结果，本轮按要求复用，
没有冒充重新运行。

| 验收契约 | 测试符号 / 证据 | 覆盖内容 |
| --- | --- | --- |
| schema v2，旧档/缺失/未来版本显式拒绝 | `session::persistence::v2_tests::schema_header_accepts_only_explicit_v2`; `new_session_rejects_legacy_and_unknown_simulation_policies`; `verification_evidence::tests::save_restore_surface_rejects_legacy_schema_before_restore` | 无迁移器、无静默 fallback；错误区分 legacy/newer/current malformed |
| 初始、成功 market tick、完整 CivilUpdate、恢复后未 step 均为合法静默点 | `save_contract::every_approved_quiet_point_restores_and_resaves_byte_identically` | 四类批准边界均可 decode/restore/resave，字节连续 |
| 完整 v2 权威状态 roundtrip | `save_contract::new_format_roundtrip_restores_authoritative_state_byte_identically`; `session::persistence::v2_tests::healthy_initial_quiet_point_roundtrips_complete_strategy_state` | SaveSlot v2、全量 StrategyState、恢复后立即重存档逐字节相等 |
| 同 seed 恢复后续跑等价 | `save_contract::restore_is_byte_continuous_with_uninterrupted_run` | 连续三天逐 tick 事件字节相等，日界后权威存档字节相等；覆盖 RNG、seq、策略后续行为 |
| 真实非空活订单与 envelope 恢复 | `verification_evidence::tests::real_save_v2_live_sell_projects_restore_and_continuation_evidence`; `session::persistence::v2_tests::real_step_settles_and_persists_gross_capped_seller_without_cash_reservation` | 公共玩家路径创建真实 live Sell；保存、恢复、重存、business hash 与后续 Cancel/成交路径一致 |
| envelope 逐键 live/audit 与 receipt cursor 连续 | `session::persistence::v2_tests::live_envelope_and_receipt_prefix_roundtrip_losslessly`; `pipeline::retail_projection_persistence_tests::save_decode_restore_preserves_seen_prefix_and_consumes_only_the_next_receipt` | 非空 ledger、`next_receipt_base`、seen 身份集合无损；旧收据不重复消费，合法新收据继续消费且不复用序号 |
| 卖单跨 tick 累计成交额、名义/实收分项费用持续 | `seller_cumulative_nominal_and_charged_audit_survives_restore`; `per_fill_priority_history_survives_restore_and_next_tick`; `same_tick_routes_advance_one_seller_fee_debt_without_reusing_tick_start_audit` | `filled_value`、commission/stamp-tax/transfer-fee 的 nominal/charged 累计状态保留；欠费追收不复用旧 audit |
| 零现金卖单和费用封顶的 #9 简化 | `complete_restore_accepts_zero_cash_seller_with_v2_live_envelope`; `real_step_settles_and_persists_gross_capped_seller_without_cash_reservation` | 卖单 cash escrow 恒 0、股份占用保留、低额成交收费封顶，恢复后继续一致 |
| 损坏或跨层不一致状态显式拒绝 | `complete_restore_rejects_tampered_snapshot_reservations`; `live_envelope_tampering_is_rejected_without_partial_restore`; `receipt_gap_is_rejected_as_corrupt_save`; `restore_rejects_strategy_attention_probability_drift`; `poisoned_marker_and_unknown_runtime_fields_fail_closed` | 快照/订单/envelope/游标/策略/poison 交叉校验；失败不部分恢复、不默认清空 |
| 竞价余单与到达序恢复 | `auction::restoring_mid_auction_preserves_deterministic_completion`; `auction::auction_reserves_sellable_shares_across_orders_and_restore` | 集合竞价中途恢复后完成结果与不中断一致，卖单可卖股份占用与 order arrival 序保持 |
| 非空恢复证据负对照 | `verification_evidence::tests::save_restore_surface_rejects_valid_v2_save_without_a_live_order`; `save_restore_surface_rejects_restore_resave_byte_drift`; `save_restore_surface_rejects_authority_drift`; `save_restore_surface_rejects_continuation_drift` | 证据不能由空档、伪造重存字节、伪造权威 hash 或伪造续跑产物空转通过 |

## 大 A 语义与范围

- 存档只序列化并恢复既有 A 股业务权威状态，没有改变 T+1、价格笼子、涨跌停、集合竞价决胜或 FIFO。
- 卖单零现金预留、逐腿实收封顶及“佣金 → 印花税 → 过户费”顺序属于 ADR-0017 分歧 #9，测试与存档字段保存的是该已批准游戏简化；未将其描述为交易所真实清算规则。
- 改动范围为 Task 8 存档/恢复与其 Task 9 证据接缝。本轮仅新增验收证据，不修改业务代码。

## 本轮未执行

未启动 engine 全量回归、release suite、K7 矩阵或性能 harness；这些不属于本次
Task 8 证据收束，且不得由上述定向 PASS 推导为已经通过。
