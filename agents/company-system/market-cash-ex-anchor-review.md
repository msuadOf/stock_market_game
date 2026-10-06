# Market 现金除息锚独立复核

复核范围：本批完整工作树差异中 `packages/engine/src/market.rs`、`packages/engine/src/session/pipeline/continuous_matching.rs`、`packages/engine/src/session/pipeline/auction_day_end.rs` 的市场状态/订单活动相关变更，以及 `MarketSnap`、`SaveMarketSnap`、snapshot/parser、restore、hash、`MarketDelta` 和调用链。未审查 UI 纯 parser 业务以外的公司会计实现；未修改代码，未运行测试或 Cargo 回归。当前工作树仍在变更，记录结论针对复核时的状态。

## 规则依据与判断

- 已阅读 `AGENTS.md`、`docs/principles.md`、`docs/trading-rules.md` 及 `agents/remaining-questions-and-features/dividend-research.md`。研究记录称已核对 2026-07-06 生效的沪市《交易规则（2026年修订）》4.3.1—4.3.3、深市对应规则 4.4.1—4.4.3，以及即时行情前收盘处理；标准纯现金、无股份变动的除息参考价为前收减税前每股现金红利，作为除息日涨跌幅基准/行情前收。记录包含官方原文访问方式及适用日期。
- 当前 Market 改动没有覆写 `last_price` 或历史行情，也没有制造成交；在 pending 且无盘口时，连续竞价参考回退到 `last_close`，涨跌停基准也读取该字段，方向符合规则。`prepare_ex_date_reference` 对正值及最小价位对齐有校验；标准公式依然显式排除交易所批准的特殊调整。

## 发现

1. **阻断：没有会话/公司行为调用点安装锚。** 全仓搜索 `prepare_ex_date_reference` 只有 `Market` 方法、单元测试和测试 fixture；`cash_dividend_ex_reference_price` 也没有从 session 日终/开市路径调用。新增纯计算结果没有流入实际证券市场，故实际会话的除息日前收、涨跌幅边界和无成交笼子回退均不变。需要将已批准的标准纯现金公司行为事实接入对应证券除息日首次开市前；调用必须尊重该证券所属交易所日历，并在任何本日受理委托/竞价处理之前完成。

2. **阻断：连续无成交跨越后续除息日会永久拒绝新锚。** `end_of_day` 在 pending 时保留前收并将 `day_market_activity` 清零；但 `prepare_ex_date_reference` 的安装 guard 同时拒绝任何 `cash_ex_reference_pending_trade == true` 的状态。若某证券除息日及之后一直无成交，已有锚正确保留，却无法被更晚一个除息日的新锚替换。需区分“该日是否已开始活动”与“旧锚尚未被成交消耗”：较晚 ex-date 且当日未活动时应允许按新公司行为更新当前参考价，同时保留最近已应用事实的日期顺序/幂等冲突检查。对不同交易所休市及 next-zero-trade 场景也应覆盖。

3. **高：严格存档目前未验证三字段之间的事实一致性。** Rust `validate_save_slot` 对市场快照仍仅校验 `last_price/last_close > 0`；Web `parseSaveSnapshot` 也只校验新增 anchor 的参考价为正并做字段精确匹配。于是手工编辑的存档可表达例如 `pending=true, last_cash_ex_reference=null`、pending 锚与 `last_close` 不同、anchor 价不符合该证券 tick，或 anchor 日期越过当前会话日期等矛盾事实，随后被 `restore_prices` 原样安装。新增三个字段已进入完整/最小 snapshot、restore 和 hash；需在 Rust 权威恢复校验中定义并强制一致性，不能依赖 TS parser 或推断默认值。Web parser 应与同一契约对齐。

4. **高：集合竞价 activity 以完成时订单簿推断会漏掉“曾受理后全部撤销”。** `auction_day_end.rs` 在消费完成时仅当 `input.completion.state.orders()` 非空才调用 `mark_day_market_activity`。若本轮存在已受理 auction placement，随后成功撤销使最终订单集为空，Market activity 仍为 false；晚到的锚安装会被错误放行。activity 应由已受理操作事实驱动，包括已受理后立即撤完/未成交的 auction 委托，而非由最终挂单集合反推。作者正在修此项；修复后须复核增量 auction continuation/finish 是否完整携带 activity。

## 增量状态与其他边界

- `MarketDelta` 已纳入 pending、最近锚和 activity 的 before/after，并在 `apply_changed_orders` 核对基态；`Market::hash_projection` 也包含三个字段。连续成功 Place 会由 `Market::place` 记录 activity，即时全成同样记录；拒单路径不触碰 Market。`last_close` 在 delta 中作为基态/不变事实检查，但不是可应用的 after 字段，符合当前连续轮不安装除息锚的结构；若未来在 worker 内安装锚，必须扩展 delta 传输而非只改 candidate。
- `Market::end_of_day` 对无成交 pending 保留 last_close，而不复权 last_price，语义符合“原始历史不复权”；成交后 pending 清除，正常日终 last_close 才跟随 last_price。
- `prepare_ex_date_reference` 对同一完整事实重试幂等、同日不同价格冲突、日期倒退冲突、较晚日期可安装均有测试。但当前“较晚日期可安装”测试先调用日终且首个锚并未保持 pending 以外的合法性，实际实现仍在 pending guard 处拒绝；该测试应覆盖真实跨日零成交流程，避免测试仅验证有成交/无 pending 的路径。

## 结论

除息锚的核心价位应用点本身保持真实成交价与历史不变，官方依据与税前纯现金基线相符；但当前批次尚未接入真实 session，且 pending 跨期、auction 全撤 activity 和存档一致性有缺口，不能通过独立复核门禁。未运行测试（按委托要求）。

## 最新增量复核（2026-10-06）

静态重审最新完整工作树差异后，确认前述大部分实现缺口有修正：

- `GameSession::step_inner` 已在交易 tick 开始前调用 `prepare_cash_ex_references_for_current_date`；它按证券/登记日汇总同日现金红利并调用标准纯现金计算，使用证券 exchange 日历。批准的特殊公式在审批入口被拒绝。市场更新先在 clone 中完成，再整体写回；真实 `last_price` 和历史成交没有变更。
- Market 安装 guard 已允许旧 `pending` 锚在后续无活动交易日被较晚 ex-date 替换；同一事实重试、同日异价冲突、日期顺序仍由最近锚事实校验。
- `Market::validate_restored_facts` 已由 Rust `validate_save_slot` 调用，校验正价、reference tick 对齐、anchor 不晚于 current date、pending 必须有 anchor 且与 `last_close` 相同。Web strict-save root 也调用跨字段校验。
- 集合竞价 `StockAuctionState` 新增单向 `had_market_activity` 标记；成功 Place 即设为 true，不随 Cancel 归零，finish 阶段据此标记 Market，逻辑上闭合“受理后全部撤光”的 activity 遗失路径。

之前识别的 session 同日重复计算阻断已由按 ex-date 跳过重算修复。该规则依赖同日已批准/登记的公司行为在首个市场 tick 前已完整存在；当前审批/登记时序符合这一前提。若将来允许日内补录已登记的同日计划，需要对公司行为事实集合绑定/比对已安装锚，避免静默跳过不同的同日组合。

补充核对作者指出的 `auction_day_end_tests.rs::opening_before_0920_cancels_existing_envelope_through_sealed_receipt`：fixture 仅有 `OrderId(10)`，本轮成功取消后断言 `session.state.auction_orders.is_empty()`，并断言 Market activity 为 true、晚到除息锚安装失败。`stock_auction_adapter.rs` tick-start seeding 通过 `StockAuctionState::apply_operation(Place)` 复用 sticky 标记，后续成功 Cancel 不清除，因此该用例确实覆盖了 accepted placement 后清空订单集的完整日终事务路径；撤销前后 activity 问题已关闭。

静态范围内，前述实现阻断项已关闭。用户要求的 targeted 运行日志尚未提供，按委托未自行运行 Cargo；最终运行验证结果待 root 提供后复核。本次不对超出现金除息锚业务范围的完整公司行为差异作总体验收。

## Market 核心与 Delta 限定签核（2026-10-06）

Root 提供的日志摘要：`build-red-stable.log` 对应目标测试二进制构建成功；修正过滤器后的 `market-fresh.log` 显示 `price_limit_state_tests` 8/8、0.00s。之前错误过滤器得到的 `market-first.log` 0 tests 不作为通过证据。未在本 agent 再运行 Cargo。

结合当前 `Market`/`MarketDelta` 完整差异及上述目标日志，限定签核通过：参考锚仅作用于 `last_close`（涨跌停基准）及 pending 无盘口时的笼子回退，不覆写 `last_price`，不制造交易；pending 在零成交日终保留，后续较晚日期可替换；正值、tick 对齐、幂等/冲突/日期顺序与恢复事实有显式校验。`MarketDelta` 对 last price、last close、pending、latest anchor、activity 的输入基态做严格比较并传递增量状态，hash projection 也包括新增权威事实；连续 Place 成功（包括全成）会被记录为 activity，错误/拒绝路径不更改 Market。

本签核严格限于 Market 核心与 Delta，未签核会话首次/重复除息日编排、完整存档跨层生成、公司分红/公告整体流程，也不替代 auction cancel-to-empty 的独立目标测试。Root 说明 auction case 将另行使用准确过滤器验证；Session 两 tick 示例仍因 fixture 停在 `CompletedSession0` 失败，该失败不作为端到端会话通过证据。
