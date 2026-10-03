# Account / Market / OrderBook 独立复核

- 审查日期：2026-10-03。
- reviewer：`/root/implement_domain/review_orderbook`，未参与产品实现。
- diff baseline：`b89afb3346743a4b4fccf26c9ac9ff108595f696`。
- 权威动作：[action-index](../../oop-refactor-audit/challenge-2026-10-03/action-index.md) 中 `engine-foundation-01-A03`、`domain-N02`、`domain-N03`、`domain-R2-N37`、`domain-R2-N38`。
- 审查对象：`account.rs`、`market.rs`、`orderbook.rs`、`orderbook/filled_orders.rs`、`orderbook/resting_index.rs` 的完整 diff，以及未跟踪的 `book_state.rs`、`persistent_index.rs`、`state_contract_tests.rs` 全文；已复核最后追加的 `fixture_remove_position`、Market 注释修正及两个边界测试。后续追加复核包括 Market 三个异常价格状态测试的完整搬移及 `tests/market/price_limits.rs` 对应完整 diff。
- 额外只读核验：GameSession 恢复路径、persistence/v2、npc_state_projection、auction_day_end 的相关调用；AccountBook 全文；account/orderbook 集成测试相关断言和跨组 caller 搜索。本记录不声称审查全仓其他动作的完整 diff。

## 结论

本批五项主体静态复核未发现尚未修复的行为缺陷；一项有效的 Money 边界测试缺口已修复并再次静态复核。本记录定位的跨组 caller 已追加核销，Market 三项异常状态测试完整搬移无覆盖丢失；全仓接线与集中编译、测试结果仍须 root 汇总，不能据本记录宣称整批改动或运行验收完成。

### 1. 大 A 语义与依据

已读 AGENTS、principles、architecture、open-questions、ADR-0017、ADR-0019、ADR-0025 及 trading-rules。依据沿用正式交易规则文档登记的沪深交易所官方规则与费用来源；文档记载规则最近核对日为 2026-09-22、同价同向受理先后补核对日为 2026-09-25，并明确中国结算费用表访问失败等局限。本次没有重新联网核查官方现行材料，因此不将静态重构审查描述为全部交易制度的最新外部验收。

本批未新增交易制度：Money 仍为分、数量为股；价格时间排序仍分别使用 `(Reverse(price), seq)` 和 `(price, seq)`；成交价仍取 maker price。OrderId treap 的 priority 只平衡身份查询树，没有成为交易优先级。账户 T+1、可卖股份、净投入成本、名义费用和实收费结算均保留原逻辑；未把 OrderBook 变成资金或费用 owner。恢复合法性仍由 save slot 校验决定，没有以局部新检查改变可编辑存档接受集或首错顺序。既有深市竞价参照、市价单、Escrow 卖费等游戏简化继续由正式文档登记，不能因本次审查升级为真实完整交易所模型。

### 2. 必要性与最小范围

- A03：Account.id、AccountState、Position 字段私有化并删除 Account 的 Deref/DerefMut，读取通过 getter，生产变更保留 Account 的初始化、结算、日界解锁及有限恢复入口；无第二账本、Wallet 或 Portfolio 服务。
- N02：Order 仅接管数量进度校验和 checked 下一值计算，保持公开字段与 serde 形状，不接管整手、交易时段、FIFO、价格或账户规则。
- N03：任意 public 价格 setter 收窄为恢复价格事实、应用竞价价格两个 crate 内入口；两类 filled 身份写入口收窄。没有添加正价或身份来源新拒绝来冒充可见性重构。
- N37：BookState 单独拥有 bids/asks/live_by_id/filled_orders，一致更新归于组合；OrderBook 保留 tick、next_seq 和撮合循环。既有部分失败行为没有被顺手改成原子保证。
- N38：两份 treap 的实质重复集中到私有 `OrderIdPersistentIndex<V: Copy>`；FilledOrders 仍无删除 API，RestingOrderIndex 仍有 remove/clear，无 trait object 或额外依赖。作为用户选定的可选动作，其范围与原动作一致。

public Rust API 已存在有意收窄：Account/Position 直接字段、AccountState 和 Market setter 的仓库外 caller 需要迁移；仓库搜索只能核对仓库内调用，不证明仓库外不存在消费者。serde 字段名、字段类型及 derive 保持原样；Position::from_restored_parts 仍接受旧结构体字面量能表达的事实，且返回独立值而非权威账户可变引用。它并不新增合法性保证。

### 3. 边界、跨层及复杂度

- Account 的 Arc::make_mut 保留 COW。买卖结算仍先完成 checked 计算再写状态；失败前不会复制/修改共享 AccountState。无锁定解锁保持共享 Arc。cfg(test) fixture 无生产构建暴露；追加 fixture_remove_position 仅移除指定 code，保留其他持仓。
- restore_balances 返回 `()`，GameSession::restore 在 validate_save_slot 之后恢复局部 sess；读取旧 cash 清空初始持仓的步骤与 baseline 一致。没有新增恢复校验或提前错误选择。
- AccountBook::get_mut/values_mut 原有 page.invalidate 机制保留。npc_state_projection 的策略替换通过 get_mut 再调用 restore_strategy，未绕过缓存失效。
- place 首错顺序仍为 duplicate ID → qty 为零 → filled_value 为负 → 数量进度 → price。taker 仍先写局部 filled_value，再 checked filled_qty，再减 qty；部分 maker 仍先减 qty，再 checked filled_qty，最后写 filled_value。Maker 金额 checked add 在 taker 金额 checked add 之前，且在簿更新之前。
- 全部 maker 成交仍按 pop_first → live remove → filled insert；cancel、restore_filled_orders、apply_changes 的分步更新顺序均与 baseline 相同。后项插入失败可能保留前项写入，但 next_seq 不推进；新增测试明确保留这一既有行为，不能将此局部 API 宣称为 tick rollback。
- treap 已逐分支对照两份原版本：priority 常量和运算、左右旋转、remove 缺失 key 保留原 root、merge 的 `<` 分支及其他分支、duplicate 的旧 root 保持、中序 entries 均等价。Arc root/node 的共享和路径复制没有改成全树深复制。
- 新测试覆盖 live/filled 身份转换、FIFO、clone 隔离、部分 maker 数量溢出的既有中途写入、maker/taker 金额错误次序、投影后项错误、合法 u32 数量临界、filled entries 排序与 duplicate、remove 缺失 root、恢复/T+1 COW 和失败结算 COW。未运行这些测试，不能以阅读测试断言替代执行证据。

## 有效发现与修复复核

### F1：taker 的 Money overflow 分支缺少直接保护测试（已修复）

初版 state_contract_tests 的 `maker_money_overflow_precedes_taker_money_overflow_without_book_write` 同时令 maker/taker 的累计金额溢出，实际两侧都会先在 maker 失败，无法保护 taker 的 `filled_value_after` 错误。原 orderbook 集成测试也未覆盖该 Money overflow 分支。

修法：增加双侧 `taker_money_overflow_leaves_book_status_and_cursor_unchanged`，maker 的金额累加正常，taker 使用 `i64::MAX`，实际成交额 2 分；断言错误为 `add` 且 operand 为 `MAX + 2`，盘口、filled 身份和 next_sequence 保持原值。已阅读追加测试全文，fixture 能经过原入口校验并到达 taker Money 错误，符合预期；没有删除或弱化原断言。另补的 `valid_fill_progress_reaches_u32_boundary_on_both_sides` 使用合法 original/filled/remaining 数量，覆盖 u32 上界与部分/完全成交。正常 taker 在入口满足 `filled_qty + qty == original_qty`，其后 fill_qty 不超过 qty，故正常入口的 taker filled_qty overflow 不可达，未要求构造虚假正常路径。

## 跨组接线初次待办与追加核销

以下为初次审查时的 caller 迁移快照，不是已冻结主体的算法缺陷；已告知 domain manager。随后追加核验确认本节列出的接线均已迁移，仍须纳入最终集中编译和测试。

- `session/persistence/v2.rs:189,282,301` 仍直接读取 Account.strategy，`:437` 仍直接赋值 strategy。应改为 strategy()/restore_strategy，保留原 getter/校验错误顺序和 get_mut 缓存失效边界。
- `session/account_book.rs:43`、`:143`、`:145` 的生产读取，以及该文件 unit fixture 中的 cash/positions/t1_locked 读写仍需 getter/cfg(test) fixture 迁移；get_mut/values_mut 的缓存失效设计无需另改。
- `tests/market/price_limits.rs:39,85,190` 仍调用 set_last_close。integration tests 不能访问 cfg(test) engine fixture，应按 N03 使用支持的 Market::new 或恢复途径，保留旧正常盘口断言；不能为测试便利重新添加 public 任意价格 setter。
- `session/failure_tests.rs:131` 和 `session/pipeline/decision_resources_tests.rs:299,371,444,532` 的 set_last_price 应迁到 crate 内 cfg(test) fixture。

这些位置基于读取时的当前工作区，其他 agent 后续可能已迁移；本记录不宣称该清单是全仓编译错误的穷举。

### 追加核销：caller 与 Market 测试完整搬移

2026-10-03 后续已读 AccountBook 完整 diff，并检查 persistence/v2 的相关生产读取与策略恢复：均改为 getter、restore_strategy 或 cfg(test) fixture，保留原错误消息/顺序与 get_mut 缓存失效。failure_tests 与 decision_resources_tests 的指定旧 setter 也已改为 fixture_set_last_price。本核销只涵盖前述已发现接线，不证明全仓已编译通过。

已逐项对照 baseline 的 `tests/market/price_limits.rs` 三个 case 与 `tests/market/main.rs` 的 mk_market/sell helper，以及最终 Market 新模块和 integration 删除 diff。三个 case 全文迁入 `#[cfg(test)] mod price_limit_state_tests`，正文仅 set_last_close 改为 fixture_set_last_close，fixture 数值、调用顺序、正常盘口、价格预期与错误断言均保持。helper 保留 600101、1000 分初始价、10% limit、1 分 tick 和 sell owner=2 等字段；无新增生产写口。

| case | 最终位置 |
| --- | --- |
| symbolic_limit_prices_resolve_at_the_current_authoritative_boundary | `market.rs:503`，`market::price_limit_state_tests` |
| price_limit_overflow_is_an_explicit_error_not_a_panic | `market.rs:546`，`market::price_limit_state_tests` |
| legal_limit_order_prices_keep_the_wider_ten_tick_cage_and_propagate_errors | `market.rs:553`，`market::price_limit_state_tests` |

旧 integration 文件已只删除上述三项及不再使用的 LimitPrice import，原十个 case 中七个继续留在 integration，三个在 lib tests，各恰好一次，没有删除断言或丢失覆盖。测试 discovery 路径改变，集中执行须包含 lib filter `market::price_limit_state_tests`，不能仅跑 `--test market` 后宣称覆盖全部原十项。本追加只进行了静态核对，未运行测试。

## 冻结文件指纹与执行边界

| 文件（engine/src 下） | SHA-256 |
| --- | --- |
| account.rs | `a4f183102d68e28885b2ddddbf465f2b9f3253558226c6784e984924802e8ae3` |
| market.rs | `00e2ccdb609e4d38c4b879b20580dfe56886030b9802fb545437b622bd8518c9` |
| orderbook.rs | `065f211533ee51595c35f10a31655733073be1622f142408be8d630366cd52f9` |
| orderbook/book_state.rs | `160e896ec0b10d3c27fc647fbc1157470b70119ebdf91f427baf73df2afd5a77` |
| orderbook/persistent_index.rs | `e958a4e88c57d25be13196128ce51693163ae4187c4c43e468a37a27f4eeed96` |
| orderbook/state_contract_tests.rs | `5131bafb1af4bf8fe7b4f1212ce00e0ea046695610a278543bb5535bd3470a9e` |
| orderbook/filled_orders.rs | `b981ad53b2a81f351d7fadeeb11a0910c542058acb20f25d5dfa4ab267a1809e` |
| orderbook/resting_index.rs | `907fa9277d6b4d62dbf9ab88ac15024f442cc2d3ec4c86ce6c78a81e2ee312cf` |

追加搬移后 `packages/engine/tests/market/price_limits.rs` SHA-256：`90d6a89eed8532ed08ca42292047252e9b7fe12df45c27cfc7866797114a0d5e`。Market 原审查指纹 `f8fafbc3449ed08e52961575658be70db3ce2686eaafe4cb98d62d35d5c2d6c7` 为追加前历史；表格已更新为搬移最终版本。

本 reviewer 只执行文件读取、rg、只读 git diff/show、sha256sum 和工作记录写入；未运行 Cargo、编译、测试、全仓 formatter，也未执行 Git 写命令。运行结果须由 root 集中验证登记；新增行为保护测试的 baseline 运行证据亦未在本审查中取得。
