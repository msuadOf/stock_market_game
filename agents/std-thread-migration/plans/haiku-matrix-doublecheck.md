# Haiku 矩阵核验产物二次核对（员工复核）

- 复核对象：`.tmp/haiku-matrix-verification.md`（41 行具名测试锚表）
- 复核方式：`shuf` 随机抽 8 行逐行独立验证（测试存在性 + 断言存在与相关性），另做全表 MISSING/可疑引用扫描
- 复核约束：只读分析（rg / 读文件），未跑构建与测试
- 日期：2026-10-17

## 一、抽样 8 行独立验证结果

抽样行号（表内行）：7, 9, 14, 17, 19, 21, 31, 33。锚点行号约定经核实 = 测试 `fn` 声明行。

| 行 | 规则面 | 测试名 | 存在性 | 断言核实 | 相关性 | 备注 |
|---|---|---|---|---|---|---|
| 7 | R04 | `orderbook_new_validates_tick` | ✅ orderbook.rs:90 | ✅ `matches!(err, OrderError::InvalidTick { .. })` 在 :97 | ✅ tick 校验 | 实习生备注"只验证 tick 为正"准确：用例仅用 `Money::ZERO` 触发，不证 0.01 元下限 |
| 9 | R05a | `a_share_buy_quantity_must_be_a_board_lot` | ✅ session.rs:3950 | ✅ `RejectionReason::InvalidQuantity` 在 :3972（IntentRejected 匹配臂内） | ✅ 99 股买单被拒 | — |
| 14 | R06 | `odd_lot_sell_allowed_only_as_full_remainder` | ✅ desk_semantics_tests.rs:469 | ✅ `skipped_orders, 1, "非整手且非全部零头 ⇒ skip"` 在 :488-491，逐字一致 | ✅ 卖 150 拒 / 卖 1050（全零头）受理 | "同时断言整笔零股余数可成交"属实（:492-497） |
| 17 | R07b | `a_share_t1_locked_shares_unlock_at_the_day_boundary` | ✅ session.rs:4008 | ✅ `sellable_qty(&code), 100` 在 :4041-4044，与"多行合并展示"声明相符 | ✅ t1_locked=100 跨日界后可卖 100 | — |
| 19 | R08 | `orders_are_rejected_during_the_0925_to_0930_preopen_window` | ✅ auction.rs:1628 | ✅ `AuctionOrderEntryClosed` 在 :1652 | ✅ PreOpen 窗口拒单回执 | — |
| 21 | R08 | `cancel_window_closes_after_first_third_of_opening_auction` | ✅ desk_semantics_tests.rs:367 | ✅ `a0.cash, 1_000_000, "拒撤+Drain 回流复原"` 在 :388，逐字一致 | ⚠️ 锚点偏弱 | 实习生的偏弱判定**经独立核实成立**：本测试无卖对手盘，撤单成败现金都复原，且无簿状态或拒撤回执断言 |
| 31 | R13 | `price_cage_rejects_beyond_wider_bound_in_continuous` | ✅ desk_semantics_tests.rs:203 | ✅ `skipped_orders, 3, "笼子外 1021/979/980 拒收"` 在 :221，逐字一致 | ✅ 逐 tick 笼子拒收 | :219-220 注释解释了 3 而非 2 的成因（参考价上移），锚点成立 |
| 33 | R16 | `market_order_remainder_refunds_instead_of_resting` | ✅ desk_semantics_tests.rs:418 | ✅ `foreign_active[0], 0, "Drain 后在途清零"` 在 :433，逐字一致 | ⚠️ 不覆盖市价路径 | 实习生"限价等价路径、市价待接线"的判定**直接由测试自身注释（:419-421）证实** |

抽样 8/8 通过：测试全部存在、引文全部找到（含声明的多行合并）、规则面相关性成立；实习生自报的两处弱点（行 21、行 33）均属实且措辞克制。

## 二、全表扫描

- 行数 = 41，唯一测试名 = 40（`grep -oP` 提取去重），与实习生"R17/R32 重复引用"结论一致。
- `MISSING / 未找到 / NOT FOUND` 扫描：零命中。
- 40 个唯一名逐一 `rg "fn <name>\b"` 于 `packages/engine`：全部 ≥1 命中，无 ZERO-HITS。
- 表内引用的 13 个 .rs 文件全部存在。
- 未抽样锚点抽查 3 处均中：R32 断言 `capture_runtime_state(&restored)` 在 saved_runtime_tests.rs:919（测试 fn @:900）；R34 `same_price_level_fills_fifo_by_arrival` @desk_semantics_tests.rs:124；R19 `routed_price_covers_fixed_highest_lowest_market` @protocol_tests.rs:124。

## 三、可信度判定

**可信**。理由：

1. 抽样 8 行零造假、零错引：文件/行号/引文全部对上（锚点 = fn 声明行，全表一致）。
2. 全表 40 名无 MISSING、无幻觉测试名、无不存在文件。
3. 自报弱点（R16 市价路径未接、R19 仅 enum 变体、R08 撤单窗与 R09 断言偏弱、R17/R32 同测重复）经抽查核实均属实——自曝缺陷与实际相符，是诚实报告的强信号。
4. 附带发现（非扣分项）：锚点行号指 `fn` 声明而非断言行，属约定差异，引用时按测试名定位即可。

保留意见（供后续消费该表时注意）：R16/R19/R08-撤单窗/R09 四处锚点弱是实习生已声明的真实缺口，不得当作语义已验证的证据使用。
