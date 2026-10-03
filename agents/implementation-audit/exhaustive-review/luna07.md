# ADR-0009／0014／0022 独立全文实现复核

日期：2026-10-03。基线 `08e4fc7`，工作树 `/data1/baiyifan/workplace/stock_market_game/.worktree/implementation-reaudit`。依要求先读根 `AGENTS.md` 与 `docs/principles.md`，并连续从首行读至 EOF：ADR-0009 38 行、ADR-0014 25 行、ADR-0022 73 行，合计 136 行；未以搜索片段代替决策全文。对照旧总账 `agents/implementation-audit/implementation-audit-2026-10-02.md` 第 56–60 行与 `agents/implementation-audit/reaudit-ui.md` 第 11–20 行。静态复核生产实现与实际 caller；未运行测试、联网或执行 Git 写操作。本记录是本轮唯一写入。

## 条款矩阵

| 来源条款 | 当前生产实现及调用链 | 判定 |
|---|---|---|
| ADR-0009 §1–2：默认 900 开盘窗口 tick、2/3 申报期、余下 PreOpen、配置边界 | `packages/engine/src/session.rs:948-985` 校验 `auction_ticks < ticks_per_day` 及开/收盘窗口总和；`:1987-2008` 以 `tick % ticks_per_day` 派生四阶段及 `auction_entry_ticks = auction_ticks - auction_ticks / 3`。`packages/engine/src/session/persistence.rs:226-254` 用同一阈值重建保存阶段。 | 默认数值 900/15,300 的配置另由 `apps/web/src/config/defaults.ts:1-10,105-114` 给出；阶段与恢复阈值一致。短周期边界需保持 `auction_ticks=0` 的有效行为。未发现偏离。 |
| ADR-0009 §3：申报累计、09:25 一次撮合、按交易所规则定价、原序余单转连续簿 | `packages/engine/src/session/pipeline/stock_auction.rs:305-365` 构造清算输入并按价格优先、到达序排序；同文件 `:370-430` 产出成交收据并核对清算量；`:445-479` 仅 Opening 余单进入连续簿、Closing 余单走 DayEnd。价格选择委托 `select_clearing(... exchange, price_tick)`，并非 UI 算法。实际 finish caller 为 `auction_day_end.rs:1417-1480`。 | 清算到价及分所选择由 engine 权威执行。输入 `StockSpec.exchange` 的有效性属于 setup / stock spec 校验；本轮未发现交易所身份被前端推断或开收盘共用错误分流。 |
| ADR-0009 §4–5：事件权威、空指示价、开盘价/OHLCV | `packages/engine/src/session/pipeline/auction_day_end.rs:1378-1398` 每 tick 发 `AuctionTick`，含 phase、可空指示价和量；`:1422-1425` 有 clearing price 才应用竞价价格；`:1481-1491` 生成成交事实与 phase 标识的 `AuctionCompleted`。 | 未交叉时不伪造前端价格；清算结果由 market/日 K 传递。未发现新偏差。 |
| ADR-0009 §6–7：固定开盘竞价区域、PreOpen 不绘制、对称轴 | `apps/web/src/app/market-chart-projection.ts:19-25` 仅过滤 ClosingAuction；`apps/web/src/mobile/market-model.ts:525-559` 再过滤 ClosingAuction、拒绝开盘申报期以外事件；`:643-650` 只对有效点定标。开盘/连续坐标由 `market-chart-projection.ts:8-25` 映射。 | Opening 区域沿用固定槽；Close 明确不挤入开盘区域。该条只核 UI 竞价范围，不代表 G10–G14 展示实现完整。 |
| ADR-0009 后果：压缩事件保留竞价分钟最新事件和完成事件，完成后带余单快照 | Engine 事件收集在 `packages/engine/src/session/pipeline/event_collection.rs:83-84` 将竞价事件作为 typed events；协议 normalized frame 带 events/竞价时序 payload（`apps/web/src/host/protocol/normalize.ts:8-20`），reducer 消费 TickBatch frames（`apps/web/src/host/protocol/reduce.ts:57-80`）。Closing/Opening auction completion 的 market 更新先于 snapshot/事实收集（`auction_day_end.rs:1417-1491`）。 | 只确认实际消费不按旧 snapshot 接口名判断；未找到足以成立的新增丢事件或余单丢失证据。事件压缩是否全路径保留的历史结论不在 G10–G14 内，本次不外推。 |
| ADR-0014 §决策：尾盘 180 tick、限价申报、禁撤、共用清算、日界失效 | 阶段由 `session.rs:1990-1999` 确定；auction router 的 `stock_auction_adapter.rs:104-114` 将 Closing 映射为 `AuctionPhase::Closing`；`stock_auction.rs:17-37,161-220` 仅 Opening 且未过禁撤点允许撤单，Closing 禁撤；`auction_day_end.rs:946-979` 收盘 auction 最后 tick 同时触发 `finish_auction` 与 `finish_day`；`stock_auction.rs:459-479` closing remainders 发 DayEnd release。 | 日界在 closing completion 后清理收盘竞价与连续簿余单。事件 phase 沿用 `trading_phase`，消费者可辨识开收盘。未发现将尾盘错路由成连续撮合的生产 caller。 |
| ADR-0014 §边界：开盘分时不可承载尾盘指示价 | `apps/web/src/mobile/market-model.ts:533-536` 显式跳过 ClosingAuction；projection 亦按 `point.phase !== "ClosingAuction"` 过滤（`market-chart-projection.ts:19-21`）。 | 实现符合“忽略尾盘、日 K 传收盘”的当前契约；尾盘单独曲线属未来扩展。 |
| ADR-0022 §用户选择及价格含义：Highest/Lowest 是游戏 limit 入口，按方向与笼子在受理时解析 | `packages/engine/src/market.rs:190-207` 分解 Fixed/Highest/Lowest 和买卖方向；`price_resolution.rs:9-24` 仅对待解析 draft 调 resolve，`:25-74` 发非成交 PriceResolved 收据并释放多占现金。Opening caller `auction_day_end.rs:556-600` 传 `apply_price_cage=false`；连续撮合实际处理器调用位于 `continuous_matching.rs` 的受理路径（`rg` 命中 `:888` 起）。 | 符号价不是交易所市价单。笼子差异按 phase 处理；并非给旧单追价。 |
| ADR-0022 §资金并发：足额预留、不得借款/减量、释放不可在同 tick 重用、receipt 可重放 | `packages/engine/src/session/pipeline/account_validation.rs:595-596` Highest/Lowest 最坏价格预留；`price_resolution.rs:25-74` 解析后账本 receipt 显式调整；`ledger_validation.rs:60-88` 核验 pending 请求、价格方向和账本变更；`auction_day_end.rs:592-600` 将解析 receipt 应用到 auction envelope；连续 worker 禁止未解析单进入撮合（`continuous_matching.rs:888` 起）。 | 现有路径显式持有预留并记录解析；清算/撮合前要求实际价格。未发现“只改撮合线程”或 restored 订单重新解析。 |
| ADR-0022 §NPC、排队、存档/恢复：新请求保留符号，已接纳单保存固定价格 | Engine intent 类型在 `packages/engine/src/strategy/mod.rs:128-140`；恢复校验从 `persistence.rs:258-271` 开始校验完整保存状态；Web订单 schema 精确接受符号价见 `apps/web/src/save/schema/orders.ts:20-30`（含 `price` parse）；已有挂单 envelope 在 auction input 中从 ledger 投影并校验 `stock_auction_adapter.rs:175-220`，受理挂单禁止 `pending_price`，见 `:324-343`。 | 与“未受理/已受理”边界一致。恢复与竞价转簿不重新解析已定价挂单。 |
| ADR-0022 §验证范围：开/收盘竞价、边界量、NPC 工作单 | `packages/engine/src/session/pipeline/auction_tick_transaction_tests.rs`、`continuous_matching_tests.rs`、`ledger_receipt_tests.rs`、`reconciliation_plan_tests.rs`、`reconciliation_plan_phase_tests.rs` 是相邻静态测试源码。 | 未运行测试，测试代码不当作本轮行为证明；仅作为覆盖位置索引。 |

## 旧 G10–G14 逐条复核

| ID | 旧结论 | 本轮复核证据 | 结论 |
|---|---|---|---|
| G10 | 累计量差被同分钟覆盖，且竞价累计量未初始化连续累计基线。 | `market-chart-projection.ts:8-16` 每帧算 `cumulative_volume - previousVolume` 并硬设 `buy: true`；`:28-44` 对每帧处理，`:36-40` 只在连续点更新 `continuousVolumes`，auction points 不设置基线；`market-model.ts:565-570` 以 minute 为 key 替换。同分钟 frame 的增量不能相加，后续相同累计值会把槽量替为 0。调用链 `useMarketChartRuntime.ts:51-75` 收到 protocol frame 后 upsert 并驱动图表。 | 仍缺，旧判断准确。 |
| G11 | 普通跨日增量未按日隔离，新日未采样槽保留旧点。 | `reduce.ts:57-80` TickBatch 将新 frame 追加到 `state.intraday`；`useMarketChartRuntime.ts:57-64` 对普通 TickBatch 走 `projection.upsertFrames`，不 rebuild。projection 使用日内槽作为 `time` (`market-chart-projection.ts:8-25`) 并通过 `mergeMinutePoints` 合并 (`:37,42`)；除 CivilUpdate 明确 rebuild (`useMarketChartRuntime.ts:53-55`) 外，TickBatch 本身无清日步骤。旧 helper `currentTradingDayEvents` 目前只有定义（`market-model.ts:572-577`），不是该生产路径的 caller。 | 仍缺，须表述为 ordinary TickBatch incremental path 缺日界隔离；完整 CivilUpdate refresh/rebuild 不等于该路径清理。 |
| G12 | 连续方向固定 true；竞价依据价格空值；量柱未按红空心涨、绿实心跌样式。 | `market-chart-projection.ts:15,24` 可直接证实两个方向代理值不比较前值；renderer 将 `buy` 映射到 rise/fall（`MobileStockDetail.tsx:153`）；当前分时 volume selector 明确 `.msd-minute-bars i.rise { background:var(--msd-rise); border:0; }`（`MobileStockDetail.css:123-126`），量柱基础规则实心背景。 | 仍缺，旧判断准确。涨跌是价格方向展示，并非真实主动买卖方向。 |
| G13 | 最近七笔从 newest-first trade list 取尾部，再反转，错取更旧笔。 | `store.ts:106-120` incoming 逆序后前置，`state.items[0]` 保持最新且最多 100 笔；`market-model.ts:655` `slice(-7).reverse()` 从 newest-first 尾部取较旧区间。 | 仍缺，旧判断准确。 |
| G14 | 每笔显示统一当前 `tradeTime`，不是该成交时间。 | `MobileIntradayProjection` 仅按 `elapsedMinutes` 生成单一时间（`market-model.ts:663-664`）；渲染每行复用（`MobileStockDetail.tsx:158-160`）。类型映射的 `TradeEvent` 是 engine `Event.Trade`（`apps/web/src/types/engine.ts:65`），Trade event 用 `seq` 而没有 tick/time 字段；`effects.ts:18,59-69` 将 trade fact event 单独转换成 effect，`useMarketChartRuntime.ts:31-39` 再写 store，未附 frame tick。 | 仍缺，旧判断准确；补时间需沿权威 frame/event 消费链关联 tick，不能用 UI 当前时钟伪造逐笔时刻。 |

## 新候选及反证

本轮未发现已满足“当前生产路径可定位、违反这三份 ADR 且非既有 G10–G14/未来范围”的新缺口，故新候选为 0。重点排除了以下容易误报的路径：

- `ClosingAuction` 的完成由 phase 路由到 closing 清算，日界同 tick 做 DayEnd；而非只因旧实现名/旧快照接口变化判定完成链消失。
- Closing `AuctionTick`/`AuctionCompleted` 都携带 phase；mobile projection 和 collector 都显式排除 ClosingAuction。ADR 明确尾盘图尚属未来能力。
- Pending symbolic price 在受理时经同一订单账本形成解析记录，resting envelope 必须已无 pending price；不重新解析已有订单。
- 源码中的测试覆盖仅登记位置，未据测试名称/断言推定生产调用已接线，也未执行测试。

除 G10–G14 外无本轮新候选。G12 的 CSS 子证据行号需按当前实际分时 selector 单独复核；这不影响方向语义缺口。
