# Account / Position integration tests caller 迁移

## 范围与结果

- `packages/engine/tests/account.rs`：Position 字面量迁移到 `from_restored_parts`；Account / Position 读取改用 getter。持仓溢出 fixture 使用已有 `grant_position(code, u32::MAX, Money::ZERO)`，重建原有 qty、T+1 锁定、投入、回收及现金状态，不改变错误路径。
- `packages/engine/tests/strategy_state.rs`：通过 `Account::strategy()` 读取 `StoredStrategy`。
- `packages/engine/tests/company_opening/isolation.rs`：资金隔离快照读取 Account / Position getter，保持逐户与完整存档字节断言。
- `packages/engine/tests/company_scenarios/controlled_experience.rs`：相同账户收益比较使用 `Account::cash()`。

`strategy.rs` 中相关字段属于 `StrategyData`、`SelfView`、`PositionView`，无需迁移；`packages/engine/tests/account_book.rs` 不存在。`extraction_replay.rs` 的存档持仓是公开 DTO `PositionSnap`，无需迁移。其他 worker 拥有的 fixture 文件未修改。

本批仅改访问方式与等价构造，不改变现金分、股份股数、成本累加、费用、T+1 或失败原子性语义；未新增交易制度，因此未另行查证交易所规则。依据为已读工程规则、ADR-0006、ADR-0011、ADR-0024 与现有 Account 实现。

## 验证及交接

- 四个修改文件的测试名称与 `assert!` / `assert_eq!` / `assert_ne!` 序列均与 HEAD 一致，未删除或新增断言。
- 四个文件并行执行 `timeout 10s rustfmt --edition 2021 --check <file>`，全部退出 0。
- `git diff --check -- <四个修改文件>` 退出 0。
- 按 worker 分工，未运行 Cargo、编译、普通测试或完整回归；尚未独立复核，由上层统一验证及安排。

建议上层使用预构建 binaries，在每条命令 10 秒硬 deadline 和显式 harness / Rayon 并发预算下验证：

| test binary | 短 test filter |
| --- | --- |
| `account` | `position_`、`apply_`、`account_new_player_has_no_strategy`、`buy_position_overflow_is_rejected_without_debiting_cash`、`grant_position_sets_cost_basis`、`derived_values_and_grants_report_overflow`、`reexport_from_crate_root` |
| `strategy_state` | `strategy_state_round_trip_preserves_concrete_parameters` |
| `company_opening` | `initialization_does_not_pay_investors` |
| `company_scenarios` | `same_session_pnl_with_different_owned_experience_changes_retail_decision` |

如场景 filter 本身超出 10 秒，须按项目规则说明原因并拆分或缩小 fixture；本 worker 没有测量执行时长，不声称现有 filter 已通过时间门禁。

## Market 价格状态边界测试后续迁移

上层批准将三个需要独立修改 `last_close` 的完整既有用例搬入 Market 内部测试，避免为 fixture 扩大 production API。`Market::new` 从合法初始价格同时创建 `last_price` / `last_close`；当 `last_close = i64::MAX` 时，真实撮合先计算涨停价即溢出，无法通过公开撮合路径重建原有 `last_price = 1000 / 285` 的状态。

`implement_domain/orderbook` worker 已在 `packages/engine/src/market.rs` 的 `#[cfg(test)] mod price_limit_state_tests` 完整复制三个 case 和既有 `mk_market` / `sell` helpers。本 worker 在实际落地后核对完整 case 正文，仅归一化空白和 `set_last_close` → `fixture_set_last_close`；全部正文等价，然后从 `packages/engine/tests/market/price_limits.rs` 移除原位置及失效的 `LimitPrice` import。Market 源码未由本 worker 修改。

| 旧路径（test binary `market`） | 新路径（test binary `engine`） |
| --- | --- |
| `price_limits::symbolic_limit_prices_resolve_at_the_current_authoritative_boundary` | `market::price_limit_state_tests::symbolic_limit_prices_resolve_at_the_current_authoritative_boundary` |
| `price_limits::price_limit_overflow_is_an_explicit_error_not_a_panic` | `market::price_limit_state_tests::price_limit_overflow_is_an_explicit_error_not_a_panic` |
| `price_limits::legal_limit_order_prices_keep_the_wider_ten_tick_cage_and_propagate_errors` | `market::price_limit_state_tests::legal_limit_order_prices_keep_the_wider_ten_tick_cage_and_propagate_errors` |

新用例短 filter 为 `market::price_limit_state_tests`，不需要 feature。剩余 integration tests 使用 `price_limits::` filter。独立复核由 `review_orderbook` 接手；本 worker 仍未运行 Cargo / 测试，不把搬移等价检查当作运行通过。
