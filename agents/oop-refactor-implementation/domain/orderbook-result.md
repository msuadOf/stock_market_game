# Account、Market 与 OrderBook 动作实施记录

本记录对应 `challenge-2026-10-03/action-index.md` 的五个动作，审查基线为
`b89afb3346743a4b4fccf26c9ac9ff108595f696`。只修改指定 engine 文件；全仓 caller 由
session、pipeline、hosts 任务组按冻结接口迁移。独立静态审查已完成，见 [orderbook-review.md](orderbook-review.md)；root 最终编译与 14 个指定短 case 已通过，证据见 [final-summary.md](final-summary.md)。

## 逐动作结果

| 动作 | 真实 owner 与 caller | 保留边界 | 当前状态 |
| --- | --- | --- | --- |
| engine-foundation-01-A03 | Account 私有身份与 Arc<AccountState>，Position 私有四项事实；结算、历史股份分配、T+1 解锁直接由 Account 修改；restore_balances 消费 GameSession 完整校验后的事实，restore_strategy 用于持久化/投影 | 无 mutable map/Position 引用；AccountBook::get_mut 的校验缓存失效由原 caller 保持；Position serde 字段形状与原解码接受集保持 | owner 已实施；跨文件 caller 与验证由根任务汇总 |
| domain-N02 | Order::validate_progress、filled_value_after、filled_qty_after；OrderBook taker 路径与 BookState maker 路径直接消费 | duplicate → qty → filled_value → quantity progress → price；maker Money 先于 taker Money；taker value → filled_qty → qty，partial maker qty → filled_qty → value | 已实施，root 指定短测通过 |
| domain-N03 | Market::restore_prices 同时恢复两项价格事实；apply_auction_price 应用 clearing price；成交身份恢复/登记改为 crate 内有限入口 | 不新增正价/身份来源拒绝或改变输入接受集；原 save slot guard 和竞价 receipt 判定仍由 caller 保持；public 任意 setter 删除 | 已实施，caller 迁移由 session/pipeline/hosts 核销 |
| domain-R2-N37 | BookState 私有持有 bids/asks/live_by_id/filled_orders；insert/cancel/maker fill/restore/changes 都由该 owner 维护；OrderBook 继续拥有 tick/FIFO/next_seq | price-time key、maker 价、hash_projection tuple 不变；apply_changes 原分步变更与失败前已写入保留，next_seq 仅成功后推进 | 已实施，root 指定短测通过 |
| domain-R2-N38 | OrderIdPersistentIndex<V> 唯一持有 Arc<Node<V>> root 及 insert/get/remove/merge/entries；FilledOrders 与 RestingOrderIndex 保持不同领域操作 | deterministic priority 原常数与算序不变；重复 id 不替换 root；remove 缺失 id 保持共享旧 root；FilledOrders 不新增 delete | 已实施，root 指定短测通过 |

## 文件范围

- `packages/engine/src/account.rs`
- `packages/engine/src/market.rs`
- `packages/engine/src/orderbook.rs`
- `packages/engine/src/orderbook/book_state.rs`（新增）
- `packages/engine/src/orderbook/persistent_index.rs`（新增）
- `packages/engine/src/orderbook/state_contract_tests.rs`（新增）
- `packages/engine/src/orderbook/filled_orders.rs`
- `packages/engine/src/orderbook/resting_index.rs`

## 冻结接口

Account 的只读 API 为 `id()`、`kind()`、`cash()`、`strategy()`、`positions()`、
`position(&StockCode)`；Position 的只读 API 对应 `qty()`、`t1_locked()`、
`invested_cents()`、`recovered_cents()`。`Position::from_restored_parts(qty: u32,
t1_locked: u32, invested_cents: i64, recovered_cents: i64) -> Self` 重建不可变事实，
不增加超出原 serde 的局部输入拒绝。Account 的 crate 内恢复入口只应用在完整
`validate_save_slot` 后已验证的数据，不另加重复 guard 改变首错。

测试专用 helper 全部受 `cfg(test)` 约束：fixture_set_cash、fixture_set_kind、
fixture_set_strategy、fixture_set_positions、fixture_insert_position、
fixture_clear_positions、fixture_remove_position；integration tests 使用公开正常构造/
结算路径或既有 serde，不可使用这些 helper。没有 raw_mut。

Market 的 `restore_prices(last_price, last_close)` 与 `apply_auction_price(price)` 返回
`()`；恢复/登记 filled 身份原 Result 类型不变，只收窄为 crate 内。零/负价格的异常
fixture 可在 lib tests 使用 fixture_set_last_price/fixture_set_last_close。Order 字段与
serde 保持公开原形状。

这些更改有意收窄 Rust 源码 API 可见性；不声称兼容仓库外依赖旧 mutable 字段/setter 的
消费者。wire schema、价格单位（人民币分）、股数单位（股）未修改。

## 行为保护与验证

实施 treap 共基底前先加入 FilledOrders clone/重复 id/排序行为保护测试。其他重构沿用
既有 account/orderbook 行为保护，并补下列短边界测试。本 worker 按根任务的集中验证
约束未运行 cargo 或产品测试，因此没有实测 red/green，不把新增断言记作已通过。

全部新增 lib tests 无 feature 要求，可按这些 filter 运行：

- `state_contract_tests`：部分/完全成交、撤单/FIFO/clone；maker/taker Money 溢出先后；后续 projection 插入失败保留早期写入与旧游标；入口首错；两侧合法 maker/taker 数量进度到达 u32::MAX。
- `partial_maker_qty_write_precedes_progress_overflow_on_both_sides`：私有异常 fixture 验证两侧 maker 的 qty 部分写入，不扩大生产输入接受集。
- `clone_and_duplicate_preserve_filled_identity_snapshot`：FilledOrders clone、重复 id、排序与边界 id。
- `boundary_ids_remove_missing_and_duplicate_keep_original_root`：共基底缺失删除、重复插入、0/u64::MAX 与旧 root。
- `restored_balances_and_t1_unlock_leave_shared_authority_unchanged`：合法编辑事实恢复、负净成本与 T+1/COW。
- `failed_settlement_does_not_copy_or_mutate_shared_account`：资金不足保持账务与共享根。

原 `quiet_shadow_shares_account_until_its_own_state_changes` 与
`removed_orders_leave_shared_tick_snapshot_intact` 保留并迁移等价调用。产品回归、非法
save slot 恢复拒绝与真实 caller 验证仍由根任务负责；建议覆盖原 integration account、
orderbook、market price limits 及 session 的恢复/auction/shadow 套件。

本 worker 已执行局部 rustfmt（指定文件，skip_children=true），成功完成解析与格式化。
指定已跟踪文件的 `git diff --check` 通过。没有运行全仓 fmt，没有 Git 写操作，没有新增依赖。

独立复核提出有效测试缺口：maker 溢出对照不足以覆盖实际 taker 金额累计失败。
已新增 `taker_money_overflow_leaves_book_status_and_cursor_unchanged`，以双侧短 fixture
固定 maker 累计成功后 taker 返回 `MAX + 2` 的 Money add 错误，且盘口、完成身份与
next_seq 不变。数量临界另有合法 u32::MAX 双侧用例；不为正常入口已排除的 taker
quantity overflow 制造不可达生产输入。新增 lib 测试共 11 个；修复后已由原 reviewer 静态复审关闭；root 集中指定短测已通过。

## Market 异常状态 fixture 用例完整迁移

根任务明确批准将需要既有盘口与不同昨收状态的三个 integration 用例完整迁入
Market 内部 `cfg(test)` 模块。采用完整迁移，没有把原断言拆散，也没有扩大生产
Market 恢复 API。`mk_market`、`sell` 从原 `tests/market/main.rs` 逐字提取，保留股票
600101、价格 1000、10% 涨跌幅、tick=1 分及 sell owner=2/seq=0/原数量/累计值。
case 正文仅把原 set_last_close 改为 fixture_set_last_close，随后局部 rustfmt。

| 原位置 | 新位置 |
| --- | --- |
| `tests/market/price_limits.rs::symbolic_limit_prices_resolve_at_the_current_authoritative_boundary` | `src/market.rs::price_limit_state_tests::symbolic_limit_prices_resolve_at_the_current_authoritative_boundary` |
| `tests/market/price_limits.rs::price_limit_overflow_is_an_explicit_error_not_a_panic` | `src/market.rs::price_limit_state_tests::price_limit_overflow_is_an_explicit_error_not_a_panic` |
| `tests/market/price_limits.rs::legal_limit_order_prices_keep_the_wider_ten_tick_cage_and_propagate_errors` | `src/market.rs::price_limit_state_tests::legal_limit_order_prices_keep_the_wider_ten_tick_cage_and_propagate_errors` |

实际复制落地后才通知 hosts/account_callers 移除旧位置。该组已回传：完整正文归一化
空白与 setter 名称核对等价后才删除旧 case；原 10 个用例在新 source 与旧 integration
合计恰好各出现一次，integration 保留 7 个。该组的 old/new mapping 另见
`../hosts/account-callers.md`。整体搬移已提交独立复核。迁移用例不计作新增覆盖，lib
filter 为 `market::price_limit_state_tests`，无额外 feature。本 worker 未执行产品测试。

独立复核已闭环通过：F1 的 taker Money 溢出用例修复有效；三 Market case 与 baseline
正文及 helpers 等价，原 10 case 最终 lib 3 + integration 7 各一次，无断言/覆盖丢失
或生产 API 扩口。完整复核与最终文件 SHA 见 `orderbook-review.md`。集中 runner 必须
包含 lib `market::price_limit_state_tests`，仅运行 `--test market` 只覆盖留下的 7 项。
实现与静态复核冻结；编译和运行结果仍由根任务集中补记。

## 领域语义与复核

这批不修改交易制度。继续沿用现行 `docs/trading-rules.md` 的官方依据与适用日期、
ADR-0005/0017/0019/0025 的账户、提交、编辑存档与日终边界；不把 treap priority
当成交优先级，不把 filled 身份当完整历史证据，不把局部 BookState 操作当整 tick
回滚保证。OrderBook 仍不执行资金、股份可卖量或 T+1 结算。独立完整 diff 静态复核已完成；root 集中指定短测结果已记录于 [final-summary.md](final-summary.md)。
