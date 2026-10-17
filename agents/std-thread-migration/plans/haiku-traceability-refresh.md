# 语义追溯矩阵刷新（#103 与市价修复）

- 基线：`/data1/baiyifan/workplace/stock_market_game/.worktree/std-thread-model/agents/std-thread-migration/plans/semantic-traceability.md`（2026-10-17）。按旧矩阵核对零覆盖 4 面、仅对账 3 面、弱面 4 面。
- 范围：只读检查测试名、声明行及相邻断言；未运行 Cargo/测试。
- 路径别名：`desk`=`/data1/baiyifan/workplace/stock_market_game/.worktree/std-thread-model/packages/engine/src/runtime/desk_semantics_tests.rs`；`factory`=`/data1/baiyifan/workplace/stock_market_game/.worktree/std-thread-model/packages/engine/src/strategy/factory.rs`；`scope`=`/data1/baiyifan/workplace/stock_market_game/.worktree/std-thread-model/packages/engine/tests/scope_boundary.rs`；`skeleton`=`/data1/baiyifan/workplace/stock_market_game/.worktree/std-thread-model/packages/engine/src/runtime/skeleton_tests.rs`；`drive`=`/data1/baiyifan/workplace/stock_market_game/.worktree/std-thread-model/packages/engine/src/runtime/drive_tests.rs`；`anchor`=`/data1/baiyifan/workplace/stock_market_game/.worktree/std-thread-model/packages/engine/tests/runtime_semantics_anchor.rs`。
- “部分覆盖”表示测试已锚定规则的部分承诺，但表中列出的规则面仍有明确未锚部分。

| 规则面 | 旧状态 | 新状态（测试名:行号） | 备注 |
|---|---|---|---|
| R21 草稿容量约定（不按固定条数拒绝合法请求） | 零覆盖 | 已覆盖[`r21_capacity_convention_no_fixed_count_rejection`:762，desk] | 单 tick 300 张同账户买单均受理并挂簿，Drain 后资源回流；锚定“不按条数拒收”。 |
| R22b 计划分类观察节奏 | 零覆盖 | 已覆盖[`r22b_classification_cadence_matches_registered_policy`:237、`r22b_belief_horizon_days_by_classification`:289，factory] | 覆盖各风格 `daily_plan_review`、观察概率区间及 `belief_horizon_days` 60/20/5。 |
| R30 定时自动（到期执行、暂停无进度不执行、不写日终存档） | 零覆盖（engine 范围） | 部分覆盖[`intent_vocabulary_has_no_scheduled_margin_or_block_variants`:52，scope]；Web 行为仍未覆盖 | 仅锚定 `Intent` 无定时/条件触发变体。到期、暂停及存档行为属于 Web 会话面，当前引擎测试未覆盖；“存档不含该域”也未见单独断言。 |
| R33 尚未模拟负向范围（两融/大宗/临停/退整/新股无涨跌幅/科创板差异） | 零覆盖（登记性） | 部分覆盖[`intent_vocabulary_has_no_scheduled_margin_or_block_variants`:52、`security_category_vocabulary_excludes_star_and_bse`:111、`trading_phase_vocabulary_excludes_halt_and_suspension`:128，scope] | 已锚定委托、证券类别、交易相位词表的部分范围边界；退整、新股无涨跌幅等登记子面没有对应测试锚。 |
| R01b 连续竞价 taker 交叉档遇自单终止撮合 | 仅对账 | 已覆盖[`r07_continuous_self_cross_terminates_without_self_trade`:1234，desk] | 同账户交叉自单时零成交、买卖挂单原位保留，并核验成交量为零。 |
| R23 受理先后（NPC 上 tick 形成下 tick 受理、玩家下 tick 参与、来源标签非优先级、同账户按接收事实） | 仅对账／runtime 部分 | 部分覆盖[`entity_batches_arrive_before_next_start`:154、`sol3_continuation_round_acceptance_reaches_group_before_continue`:434、`accepted_batch_keeps_continuation_timing_and_owner_targeting_in_multi_group`:472，skeleton；`sol3r_continue_preserves_early_round_facts`:1725，desk] | 新锚覆盖实体批次与受理事实的跨轮时序、owner 定向和事实不丢；未直接验证来源标签非优先级、同账户按接收事实裁决。 |
| R24 计划根逐个完成交付、execution leg 生命周期、day-end drain/abort 回滚 | 仅对账（旧矩阵注明实为 runtime 单侧测试） | 部分覆盖[`d3a2c_plans_debug_projects_group_plan_book_at_delivery`:635、`d3a3_execution_leg_registers_and_prevents_duplicate_children`:900、`solmid_execution_leg_lifecycle_matches_independent_fills`:1013、`solmid_day_end_drain_releases_child_tickets`:1205、`sol4_day_end_abort_restores_execution_leg_with_tickets`:1280，drive；`solmid_plan_projection_carries_terminal_records`:1419，drive] | child 注册/成交推进、日终 Drain 和 Abort 有单侧 runtime 锚；`solmid_plan_projection_carries_terminal_records` 注释明确其 fixture 未产生 terminal 计划记录，计划根逐个完成/terminal 生命周期仍未完整锚定。 |
| R08 开盘竞价撤单窗 | 独立（runtime 锚弱：现金复原无法区分撤单成败） | 已覆盖[`r08_cancel_window_partition_by_receipt_and_clearing_participation`:998，desk] | 明确区分窗内 `Cancelled`、越窗 `CancelWindowClosed`、撤他人单 `OrderNotFound`，并核验被撤买单不参加清算。 |
| R09 部分成交只撤余量；已全成撤单显式失败 | 独立（旧管线；runtime 弱） | 已覆盖[`r09_partial_fill_cancel_releases_remainder_only`:1106、`r09_fully_filled_cancel_returns_already_filled_receipt`:1165，desk] | 断言只撤 200 股余量、全成撤单返回 `AlreadyFilled`，并核验既成结算不变。 |
| R16 市价意图按涨跌停价保护逐档立即成交、余量不进簿 | 独立（原 runtime 测试未走市价路径） | 已覆盖[`r16_market_intent_fills_walks_book_and_refunds_remainder`:807、`sol1_market_protection_uses_daily_band_not_cage_across_levels`:1360、`sol2_market_remainder_and_zero_fill_emit_visible_cancellation`:1405、`sol3r_market_sell_walks_bids_below_cage`:1923，desk；`player_market_and_symbolic_intents_route_into_runtime_world`:563，anchor] | 现有测试走真实市价路径，覆盖跨价格笼子逐档成交、余量退款/不挂簿及零成交显式撤销；保护按日涨跌幅而非价格笼子。 |
| R19 最高/最低价格选项（涨停价预留、不回补本 tick 预算、不自动追价） | 独立（runtime 仅验证 enum 变体互异） | 已覆盖[`r19_symbolic_prices_resolve_to_daily_band_edges`:886，desk；`player_market_and_symbolic_intents_route_into_runtime_world`:563，anchor] | runtime 现直接验证 `Highest`/`Lowest` 解析和按解析价预留，玩家路径另验证 `Lowest` 实际路由；预算不回补/不自动追价沿用旧矩阵所列旧管线锚点。 |

## 刷新结论

11 面中，R21、R22b、R01b、R08、R09、R16、R19 已有对应的直接测试锚；R30、R33、R23、R24 仍为部分覆盖，具体缺口见备注。测试行号均为测试函数声明行。