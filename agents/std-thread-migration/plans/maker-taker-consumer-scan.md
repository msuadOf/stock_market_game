# Trade 事件 maker/taker 语义变更 — 消费方全量扫描

- 扫描日期：2026-10-16；分支：`exp/std-thread-model`（worktree `.worktree/std-thread-model`）
- 语义变更（已拍板）：maker/taker 字段含义从「到簿序（maker=被动方/先挂，taker=主动方/新进）」改为「腿方向（**maker=卖腿，taker=买腿**）」
- 关键背景：**runtime 新管线（std-thread）已经是新语义**——`DeskTradeFact`（runtime/protocol.rs:636-659，注释"maker 恒为卖方"）在 desk.rs:1131-1135 构造时已按 `taker_side==Buy ? (maker,taker) : (taker,maker)` 归一化。本次变更是把**旧管线**（`orderbook::Trade` → `Event::Trade`）对齐到同一语义；runtime 侧读点全部无需改。
- 字段名不变（maker/taker/maker_order_id/taker_order_id/maker_filled_value_before/taker_filled_value_before），仅赋值含义翻转 → 序列化契约、TS 类型、parse 校验不受影响。

## 一、语义翻转的实现点（旧管线构造侧，必改）

| # | 位置 | 用途 | 受影响 | 适配建议 |
|---|------|------|--------|----------|
| 1 | packages/engine/src/orderbook.rs:305-322 | `Trade` struct 定义+字段文档（"被动方/主动方"） | 是（文档+语义） | 字段注释改为"卖腿/买腿"；`price` 注释"= maker 挂单价"需改（连续撮合成交价仍取 resting 价，但拍卖清算价不再是任何一方的挂单价语义） |
| 2 | packages/engine/src/orderbook.rs:588-597 | 连续撮合 Trade 构造：`maker=resting.owner, taker=incoming` | **是（核心）** | incoming=Buy 时新旧行为一致（resting=卖方）；incoming=Sell 时 resting 是买方 → maker/taker 及 maker_order_id/taker_order_id、maker/taker_filled_value_before 三对字段须**成组互换** |
| 3 | packages/engine/src/orderbook.rs:706-720 | 拍卖清算 Trade 构造：`maker=seq 先到簿者` | **是（核心）** | buy/sell 变量已在手（677-686），改为无条件 `maker=sell.owner, taker=buy.owner`，删除 seq 比较 |
| 4 | packages/engine/src/session.rs:286-296 | `Event::Trade` 变体定义（注释"maker/taker 双方结算"） | 是（文档） | 注释补"maker=卖腿/taker=买腿"；ts_rs 重导出 Event.ts |

## 二、packages/engine 内部消费点（非测试）

### 旧管线消费侧

| # | 位置 | 用途 | 受影响 | 适配建议 |
|---|------|------|--------|----------|
| 5 | packages/engine/src/session/pipeline/continuous_matching.rs:1151-1174 | `ContinuousFillReceiptProjection::run`：按 `draft.side()`（incoming 方向）分支推导 buyer/seller 及 EnvelopeKey（含 maker_order_id/对方账户/买卖 filled_value_before），喂收据/费用/结算投影 | **是（最重）** | 新语义下判定与 draft.side 解耦：buyer=`{account: trade.taker, order: trade.taker_order_id, side: Buy}`+`taker_filled_value_before`，seller=`{account: trade.maker, order: trade.maker_order_id, side: Sell}`+`maker_filled_value_before`，删掉 Side::Buy/Side::Sell 分支。市价单余量释放（1122 注释"仅加载实际成交的 maker"）逻辑需一并复核 |
| 6 | packages/engine/src/session/pipeline/book_auction_boundary.rs:69-108 | 拍卖清算腿校验：按 maker/taker_order_id 找回 Envelope，校验 account 一致+两腿反向，逐腿 `fill_receipt` | 否（行为等价） | 双腿各按自身 order_id 查找+各自 side 计费，标签互换后校验仍成立；仅收据顺序（先 maker 后 taker）对应变卖腿→买腿，建议更新注释 |
| 7 | packages/engine/src/session/pipeline/adaptive_plan_chain.rs:854-862 | 连续成交 → `CausalFactKind::Execution{maker: maker_order_id, taker: taker_order_id, side: 触发意图方向}` | 是（值随 #2 互换） | 透传无需改代码；与 #8 的 side 校验联动适配 |
| 8 | packages/engine/src/diagnostics/causal/aggregate.rs:202-254 | `reconcile_execution`：`side != taker_row.origin.side`（228 行）假设 taker=主动方 | **是** | 卖主动（side=Sell）时 taker 已是买腿 → 误报 `ExecutionMismatch`。改为按 side 选主动腿校验：side=Buy 查 taker_row、side=Sell 查 maker_row（两腿仍须反向，227 行保留） |
| 9 | packages/engine/src/diagnostics/causal.rs:87-96 | `CausalFactKind::Execution` 定义（maker/taker: OrderId） | 是（文档） | 字段注释同步"卖腿/买腿订单" |
| 10 | packages/engine/src/session/pipeline/execution_fact_producers.rs:126-139 | 旧管线 `Event::Trade` 构造（透传 trade.trade.maker/taker） | 否（透传） | 无代码改动；发射值随 #2/#3 翻转 |
| 11 | packages/engine/src/diagnostics.rs:704-723 | `observe_committed_events`：maker/taker **对称**各 record(qty) 参与者执行统计 | 否 | 对称消费，互换无影响 |
| 12 | packages/engine/src/verification_evidence.rs:992-1006 | `project_event`：maker/taker 序列化进证据 JSON | 是（基准值变化） | 代码不改；卖主动场景 maker/taker 值互换 → 证据 JSON/manifest 哈希基准需重算 |
| 13 | packages/engine/src/verification_evidence.rs:919,958,974；session/protocol/commit.rs:192；session/pipeline/event_collection.rs:74；session/pipeline/event_key.rs:131 | `Event::Trade` 模式匹配，只用 seq/code/`..` | 否 | 无需改动 |
| 14 | packages/engine/src/diagnostics/causal/microstructure.rs:45-115 | Execution 消费只用 `side`（方向持续性/报价冲击），忽略 maker/taker | 否 | 无需改动 |

### runtime 域（已是新语义，无需改）

| # | 位置 | 用途 | 受影响 |
|---|------|------|--------|
| 15 | packages/engine/src/runtime/protocol.rs:636-659 | `DeskTradeFact` 定义（maker 恒卖方注释已在） | 否 |
| 16 | packages/engine/src/runtime/desk.rs:1129-1160 | DeskTradeFact 构造（1131-1135 已归一化 maker≡卖/taker≡买） | 否 |
| 17 | packages/engine/src/runtime/event_bridge.rs:24-39 | DeskTradeFact → `Event::Trade` 映射 | 否（变更后两管线语义统一，建议更新头部注释"语义对齐"段） |
| 18 | packages/engine/src/runtime/drive.rs:323-338, 544-555, 563-589, 843-879 | DeskTradeFact 消费（买腿 VWAP/账户触达/计划执行腿/期末投影），注释均"maker≡卖方/taker≡买方" | 否 |

### 内部命名（非事件字段，不受影响；可选重命名防混淆）

| # | 位置 | 说明 |
|---|------|------|
| 19 | packages/engine/src/orderbook/book_state.rs:124,231（`apply_maker_fill(maker:&Order)`=resting）；market.rs:303（`record_makers`） | 内部形参/开关名 |
| 20 | packages/std-thread-proto/src/book.rs:478-510,629-788；oworker.rs:442-465,630-719 | `settle_fill/settle_desk` 局部变量 maker=挂单方，按 `maker.side` 解析买卖腿——与事件字段无绑定 |

## 三、apps 消费点

| # | 位置 | 用途 | 受影响 | 适配建议 |
|---|------|------|--------|----------|
| 21 | apps/web/src/App.tsx:336 | `event.Trade.maker===0 \|\| event.Trade.taker===0`（玩家是否参与成交） | 否 | 对称判定，互换无影响 |
| 22 | apps/web/src/components/player-orders.ts:60 | 同上对称判定（玩家委托相关成交） | 否 | 同上 |
| 23 | apps/web/src/host/protocol/parse.ts:91-92 | Trade payload 字段名契约校验+解析（maker/taker 整数） | 否 | 字段名与类型不变 |
| 24 | apps/web/src/host/protocol/stock-identity.ts:149 | `CodedTradeEvent` 类型（maker/taker: AccountId） | 否 | 类型不变 |
| 25 | apps/web/src/types/generated/Event.ts:27-28 | ts_rs 生成类型 | 否（再生成） | 结构不变；Rust 侧注释更新后重跑导出即可 |
| 26 | apps/web/src/store/priceHistorySlice.ts:16-17（`buy` 染色标志）；app/market-chart-projection.ts:49,69 | "主动买入"量柱染色——由**价格对比**（value>=previous）推导，不读 maker/taker；MobileStockDetail.tsx:140 明示"引擎暂未提供主动买卖方向" | 否 | 本次不受影响；新语义落地后 taker=买腿可直接给出真实主动方向，属后续增益项（顺带可在文档登记） |
| 27 | apps/web 逐笔成交 UI（MarketTradeTape.tsx、TradesPanel、store/store.ts tradesSlice） | 只展示 price/qty，透传不读 maker/taker | 否 | 无 |
| 28 | apps/server（src 无引用）；apps/server/tests/actor.rs:174-177,729-740 | 集成测试对称断言（maker≠taker、玩家参与） | 否 | 无 |
| 29 | apps/desktop/src-tauri/src/actor.rs:1333 | `Event::Trade { .. }` 仅收集索引 | 否 | 无 |
| 30 | apps/web-wasm | 无任何 maker/taker 引用 | 否 | 无 |

**前端"主动买/主动卖"标签反转风险：未发现。** 全仓唯一按角色不对称读 maker/taker 的 UI 路径不存在（仅 #21/#22 对称判定）；量柱染色走价格对比。反转风险集中在 engine 内部（#5/#8）。

## 四、测试断言（需按新语义复核）

engine tests/（integration）：
- tests/orderbook.rs:52-57,249-250（卖10先挂+买20新进：新语义下 maker 仍=10，**不变**）；:302（拍卖"先挂的 A 先成交"断言 maker——需按卖/买腿复核）；:421-426（全 0 fixture，对称）
- tests/extraction_replay.rs:283-291,325-326 — 对称（变量名 buyer 实为"对手方"，仅命名误导）
- tests/civil_clock.rs:182-189,984-985 — 对称（变量名 seller 实为"对手方"）
- tests/causal_diagnostics.rs:53-54；tests/support/event_mapping.rs:49-50；tests/runtime_flip_batch1.rs:209-219（对称）；tests/common/flip_helpers.rs:168-175（对称）

engine src 内联测试（约 76 处引用 / 16 文件）：
- 角色定向、需逐条复核/翻转：continuous_matching_tests.rs（19 处）、execution_fact_producers_tests.rs（10）、continuous_trade_acceptance_tests.rs（6，`maker: SELLER, taker: PLAYER` 买主动场景新语义恰好一致，卖主动场景需翻）、continuous_projection_transaction_tests.rs（6）、book_auction_boundary_tests.rs（2）、continuous_tick_*transaction/finalizer_tests.rs（各 1）、plan_owner_tests.rs（1）、session_execution_transaction_tests.rs（1）、orderbook.rs:1032-1034、session.rs:4560-4568、decision_chain.rs:2156-2158、verification_evidence/tests.rs（8）、diagnostics.rs:2120-2162、causal aggregate/microstructure 内联（6）
- 已是新语义、无需改：runtime/bridge_tests.rs（6）、drive_tests.rs（4）、equivalence_tests.rs（4）

apps/web 测试 fixture（9 文件 18 处，均对称使用）：player-orders.test.ts:32,93、protocol-effects.test.ts:18,43,76、protocol-parse.test.ts:81、event-buffer.test.ts:85、runtime-snapshot-policy.test.ts:13、local-amount-render.test.ts:66、market-model.test.ts:216,228,280,293-294、mobile-component-render.test.ts:62、mobile-intraday-projection.test.ts:76,90

## 五、汇总

- 生产消费点：30 项（engine 20 + apps 10）；其中**必改 5 处**（#1-#4 构造/定义/文档 + #5 结算投影 + #8 causal 校验 = 实际 6 处代码）、**值变化需重算基准 1 处**（#12 证据）、其余不受影响或仅注释/再生成
- 测试：engine 约 26 文件需复核（其中角色定向断言约 10 文件）、apps 侧基本无需改（对称）
- 适配量级：**中**（生产改动集中在 orderbook 两处构造 + continuous_matching 买卖腿分支消除 + aggregate 校验分支 + 注释/生成类型；主要工作量在旧管线约 10 个内联测试文件的角色断言复核与翻转，以及证据基准重算）
