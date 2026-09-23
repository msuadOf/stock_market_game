# 保留测试台账（Todo 3）

测试基线是 sealed attempt-12 的 commit `7041d35dc362ca74f4f3313e6804db9499f0679a` 加 `overlay.json`。verifier 读取 overlay 而不硬编码；当前 overlay 不含 `packages/engine/tests/**`，所以现有测试字节等于该 commit。

## (a) 未改动保护

未出现在 diff 的保护符号必须保持 baseline 字节。它们不是 hunk，不得伪造成变更。

## (b) 冻结 #9，严格四项

| 文件 | 符号 | effect id |
| --- | --- | --- |
| `packages/engine/src/session.rs` | `planned_sell_fee_is_reserved_before_a_later_buy` | `E9-A_SELL_CASH_RESERVATION` |
| `packages/engine/tests/session.rs` | `sell_order_is_rejected_when_cash_cannot_cover_fee_shortfall` | `E9-B_SELL_ACCEPTANCE_FLIP` |
| `packages/engine/tests/session.rs` | `sell_order_reserves_fees_for_a_possible_small_partial_fill` | `E9-B_SELL_ACCEPTANCE_FLIP` |
| `packages/engine/tests/session.rs` | `buy_and_sell_orders_share_one_cash_reservation_budget` | `E9-A_SELL_CASH_RESERVATION` |

`company_scenarios/constraints.rs:90-94` 是 plan soft-budget `available_cash`/`allocated_cash=500`，不是订单卖方 `reserved_cash`，明确排除。直接 accepted resting seller cash assertion anchor 为无。机器源为同目录 `preserved-test-inventory.json`：A=2、B=4、exact C=4、additive C=12。

## (c) 精确 Todo 2 合同改写

`step/save -> Result` 的 `expect("healthy step/save")` unwrap 及其 rustfmt 相邻片段允许；禁止借此改变 reservation、available cash、acceptance/rejection、fee、matching、plan input 或 decision output。

| 文件 | 符号 | normalized SHA-256 | 理由 | 禁止扩张 |
| --- | --- | --- | --- | --- |
| `packages/engine/tests/account.rs` | `reexport_from_crate_root` | `e988cf9ae659f38ba8074c60ef13eb27ba9a7117ad38ee80ca5f948b93a8c850` | Todo 2B-2 non-authoritative strategy production export capability boundary。 | reservation, available-cash, acceptance/rejection, fee, matching, decision-output |
| `packages/engine/tests/company_event_contract.rs` | `announcement_event_follows_successful_immutable_library_insertion` | `82fbe6405eec88e3ecd5fc3003780a00438f46e088ae0e09bc2a78f53b378ffe` | Todo 2B-4 immutable announcement public index。 | trading, reservation, acceptance, decision |
| `packages/engine/tests/company_event_contract.rs` | `civil_report_refresh_validates_and_reconnect_resolves_publication` | `44703e83b60214b0c16e257a56ca7f78c459f6098f70bb62de19f13aee657eb1` | Todo 2B-4 CivilUpdate reconnect/API。 | trading, reservation, acceptance, decision |
| `packages/engine/tests/company_scenarios/restore.rs` | `live_partial_fill_restores_and_continues_identically` | `dfd70220f09e77227a08fc98e4eb57c843db7b7752d3cb649b6509e03553c10d` | Todo 2B-2 `save() -> Result` rustfmt-split adaptation。 | reservation, available-cash, acceptance/rejection, plan input, matching, fee, decision-output |

最后一项额外强制保留 `uninterrupted` 与 `restored` 两个 `filled_qty == 200` 断言。

12 个 untracked Todo 2 contract 测试按 `scripts/simulation/verify-preserved-tests.mjs` 中的路径及 SHA-256 allowlist 接受；同路径 replacement 或任何未列入文件均拒绝。
