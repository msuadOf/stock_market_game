# sweep07：阶段、集合竞价、分时轴与符号限价全文核对

审计对象：产品源码 `b76ece3`；本轮 worktree HEAD 为 `4ad5a2e298d086024e6b0069b98b4a6b195a0a01`，已用只读 `git diff --name-only b76ece3 -- packages apps` 确认产品树无差异。已读根 `AGENTS.md` 与 `docs/principles.md`。本文件只记录审计证据，未修改产品代码，未运行测试；测试名称仅为已读代码证据。

## 全文覆盖

| 文档 | 连续全文读取范围 | 行数 | 审计内容 |
| --- | --- | ---: | --- |
| `docs/decisions/0009-call-auction-and-intraday-axis.md` | 1–38 | 38 | 上下文、决策1–7、四条后果、2026-09-08修订 |
| `docs/decisions/0014-closing-call-auction.md` | 1–25 | 25 | 背景、setup/phase、五条行为、跨宿主后果 |
| `docs/decisions/0022-symbolic-limit-prices.md` | 1–73 | 73 | 公共API、受理时选价、四组合、占款、解析收据、NPC、恢复、验证范围、规则依据 |

共 136 行；逐条映射如下。路径均相对仓库根，行号指当前产品树。

## ADR-0009 条款映射

| 原文 | 追到的实现 | 判断 |
| --- | --- | --- |
| 10：旧局直接连续竞价却显示集合竞价 | `packages/engine/src/session.rs:1989` 的 `phase()`，`pipeline/authoritative_tick.rs:25` 三路实际调度 | 旧引擎缺口已实现；UI固定“集合竞价”标签是开盘区域标题，不能据此重新判断全部竞价未实现。 |
| 14：900开盘窗口、600申报/300 PreOpen、15300总tick | `apps/web/src/config/defaults.ts:15` 定义一分钟60tick、240交易分钟、10/5分钟开盘窗口；`:111` 配置开盘/收盘窗口；`session.rs:1989` 阶段切换、`:2003` 申报期换算 | 默认配置与阶段推进已有；ADR-0014将最后180tick显式改为ClosingAuction，不能把早期“14400连续”字面作为当前尾盘应连续的要求。 |
| 15：显式auction_ticks、0关闭、小于日长度 | `session.rs:927` setup字段，`:970` 边界及开收盘窗口和校验 | 已实现，短周期fixture并非默认局。 |
| 16–20：累积不提前成交、exchange正交、沪深选价及价时优先、余单保原序转簿 | `pipeline/auction_day_end.rs:964` 仅末tick完成，`:1378` 先产生指示；`pipeline/stock_auction.rs:798` 清算，`:817` 沪市最小未成交量/中间价，`:837` 深市价优差/昨收距离/低价末级；`:909` 价优量必须可全部成交；`:357` 价格/arrival_seq排序；`:445` 开盘余单输出；`auction_day_end.rs:1479` 转连续簿 | 已实现。`tests/auction.rs:472`、`:495`、`:520`、`:552`、`:577`、`:626`、`:1143` 有对应行为测试。 |
| 21：权威phase/AuctionTick/AuctionCompleted、无交叉空指示价零量 | `session.rs:197`、`:219`、`:238`；`auction_day_end.rs:1389`、`:1483` 由提交候选产生phase事件；`stock_auction.rs:811` 无候选返回None | 已实现，`tests/auction.rs:601` 验证无假指示价。 |
| 22：真实成交定义开盘并计入OHLCV，无成交昨收零量占位 | `session/candles.rs:132` 日K更新、`:151` 首次真实量重建占位OHLC；竞价matches经交易投影提交；`protocol/commit.rs:53` 携权威活动日K | 已实现，`tests/auction.rs:673` 明确核对真实开盘/量，`:705` 明确验证PreOpen存档完整活动K。 |
| 23：100个六秒竞价槽、PreOpen无行情宽度、午休压缩、240分钟固定槽/未来留白 | `apps/web/src/app/market-chart-projection.ts:20` 竞价每6tick换算槽；`mobile/market-model.ts:192` 竞价0–99映射0–16%、连续0–239映射剩余84%；`MobileStockDetail.tsx:128` 固定分区及`:138` 午休共点刻度；PreOpen生产路径`pre_open_transaction.rs:1` 无行情交易 | 固定轴主体已有。G11日界污染仍未解决，污染后的未来槽不再留白；不能用固定轴函数核销生产分时问题。 |
| 24：昨收为0%、全部有效点最大绝对偏差、上下同距 | `market-model.ts:160` symmetricIntradayScale、`:649` 合并竞价及连续有效点，`:667` priceY | 数值尺度已实现；G32仍涉及DOM/SVG高度体系中的0.00%视觉偏移，函数通过不能核销像素位置。 |
| 28：四宿主共用引擎事件 | `pipeline/authoritative_tick.rs:25` → `protocol/commit.rs:7` → `protocol/commit.rs:53`；Server `apps/server/src/actor.rs:1148`，Tauri `apps/desktop/src-tauri/src/actor.rs:873`，WASM `apps/web/src/host/wasm-tick-loop.ts:79` → 公共protocol | 已接同一公共提交链，没有前端独立竞价清算。 |
| 29：高倍率保留竞价指示/完成、完成后同步余单转簿 | `apps/server/src/actor.rs:1155` 保留整个TickFrame，Tauri `actor.rs:880` 同样保留；`protocol/commit.rs:62`/`:85` 同时投影Indication/Completion；`protocol/civil/session.rs:333`→`protocol/delta.rs:182` 发布账户及working_orders；Web `host/protocol/reduce.ts:71`应用delta，`useMarketChartRuntime.ts:64`应用权威market | 新提交帧/RuntimeDelta替代旧裸事件压缩+补snapshot实现，同步余单事实已有。不能将runtime_snapshot=None单独列为缺口。保全每tick强于旧按竞价分钟保留；G18背压/G19发布节奏仍为另项。 |
| 30：默认显式900窗口 | `apps/web/src/config/defaults.ts:111`、`types/generated/SessionSetup.ts:21`；Rust setup无缺字段补齐 | 已实现。 |
| 31–38：严格完整存档、无旧格式迁移 | `session.rs:407` SaveSlot deny_unknown_fields；`apps/web/src/save/schema/root.ts:49`完整根键；`schema/market.ts:19` exchange/category、`:45`setup键、`:81`账户冻结/T+1/日K；`schema/orders.ts:97`委托/价格窗口/RNG；`session/persistence/v2.rs:228`运行态交叉校验、`:372`恢复完整策略 | 完整字段与拒绝旧缺字段已有。当前schema_version/runtime_v2是后来提交证据契约，不构成旧格式兼容实现；不能要求恢复已经淘汰的无版本格式。 |

## ADR-0014 条款映射

| 原文 | 当前生产链 | 判断 |
| --- | --- | --- |
| 13–15：closing_auction_ticks/ClosingAuction、默认180tick | `session.rs:932`、`:1995`、`:2007`；`apps/web/src/config/defaults.ts:20`及`:112` | 已实现。 |
| 16：只接受限价、不撤单 | `pipeline/stock_auction.rs:29` Closing不允许撤单、`:183`拒绝；`auction_day_end.rs:1220`附近Place校验/解析 | 已实现，不能据PlaceMarket公共类型存在就认定竞价受理市价。 |
| 17：复用交易所清算、日界余单失效 | `auction_day_end.rs:968` 收盘末tick完成且结束日；`stock_auction.rs:330`共用选择器、`:461` Closing释放；`auction_day_end.rs:1442`同时清理连续余单 | 已实现。 |
| 18：两个竞价事件均携phase | `auction_day_end.rs:1392`、`:1486`生产phase；`protocol/commit.rs:75`、`:97`投影phase；Web `host/protocol/parse.ts:87`、`:90`严格解析 | 已实现。 |
| 19–20：前端忽略尾盘指示，日K收盘价传真实结果 | `market-chart-projection.ts:20`过滤ClosingAuction；`protocol/commit.rs:55`活动日K、`:128`收盘K；日终CivilUpdate带刷新，Web `useMarketChartRuntime.ts:53`重建并换权威snapshot | 尾盘指示隔离已接；独立尾盘竞价曲线是明确未来需求，不新增缺口。 |
| 24–25：存档/三宿主新字段及phase、未来曲线扩轴 | `schema/market.ts:45`、`SessionSetup.ts:26`；四宿主共用protocol；`session.rs:932`严格字段 | 已实现当前范围；扩尾盘轴明确尚未要求。 |

## ADR-0022 条款映射

| 原文 | 当前生产链 | 判断 |
| --- | --- | --- |
| 9–15：Fixed/Highest/Lowest公共类型、实际受理时单次解析、入簿固定、玩家UI可后续 | `strategy/mod.rs:128`定义符号类型；`pipeline/account_validation.rs:589`保留requested_price，仅用极值预留；`continuous_matching.rs:449`在股票实际操作位置调用resolve_draft；`auction_day_end.rs:1238`竞价同一解析入口 | 生产接线已有，不把玩家表单未提供符号选择列为本ADR遗漏。 |
| 19–26：四组合、笼子仅连续、Fixed独立校验、对手/清算价成交 | `market.rs:191`四组合、`:180`笼子开关；`continuous_matching.rs:454`仅Continuous启笼子；`auction_day_end.rs:1238`禁用笼子解析；Fixed不创建解析收据；`stock_auction.rs:392`附近按selection.price成交 | 已实现，`tests/session.rs:4962`测试四组合/开关，`tests/auction.rs:1175`及`:1212`测试开收盘。 |
| 30–36：跨股占款、最高买涨停含费、最低买跌停含费、卖占股、明确不足/NPC估量 | `account_validation.rs:595`/`:596`选择预留基准；AccountValidatorDriver账户预算；`local_admission.rs:104` Cash/账户-股Shares资源lane；`strategy/hot.rs:30`按daily_upper_limit估量、`:35`Highest；`strategy/retail.rs:47`主动路径 | 已实现；`tests/session.rs:4990`跨股同轮资金竞争与下一轮释放，`pipeline/account_validation_tests.rs:9`保留符号与预留。 |
| 37–41：明确非成交解析收据、保留原意/前价/后价、零费用量交付、禁止固定单/旧单改价、重放核对 | `pipeline/price_resolution.rs:9`检查draft/envelope一致、`:57` PriceResolved原意/before/after、`:62`数量/价值不变、`:66`只释放多余现金、`:67`至`:72`费用/交付为零；`ledger_validation.rs:49`新pending首转换限制及方向验证 | 已实现；`ledger_receipt_tests.rs:9`附近伪造拒绝/链核对，`:40`重复拒绝，`:58`Fixed及tick-start旧单拒绝。 |
| 42–48：同tick释放不回补预算、最高买不足不擅自减qty、Fixed按己价 | `account_validation.rs:593`Fixed和极值分支；解析释放在后续receipt settlement而非重新注入本轮budget；`tests/session.rs:4990`建立同轮不足/下一轮足够对照 | 已实现；预算争用以账户lane分配，不能强加跨账户全局FIFO。 |
| 52–54：主动散户/动量游资符号单、机构/反转保Fixed、正常撤旧再申报、不丢候选 | `strategy/retail.rs:54`/`:76`、`strategy/hot.rs:35`/`:49`；`session/decision_chain/quote.rs:192`基本面报价决策→`session/plan_chain_candidates.rs:65`构造Fixed；`pipeline/continuous_lifecycle_projection.rs:343`符号工作单对齐；`local_admission.rs:178`严格同账户同股票Cancel→Place依赖 | 已实现当前选价/工作单链，不能因机构报价保持Fixed认定缺漏；机构/散户其他能力缺口仍按G07等单列。 |
| 56–58：待处理保符号与撤换依赖、受理单存数字、恢复/转簿不重解析、分单位、拒绝旧数字请求 | `apps/web/src/save/schema/orders.ts:24`只接受两符号或Fixed结构；该文件`:98`的auction.limit及`:99`的resting.price为数值；`tests/session.rs:4890`待处理Highest存档→恢复→解析，`:4908`已受理10.10恢复仍10.10；`tests/auction.rs:1175`开盘恢复/转簿保已定价 | 已实现。 |
| 62–66：验证范围、不能无测量宣称更快/确保每股成交 | 上述实时/四组合/开收盘/余单/重复伪造/跨股同轮测试均已存在；本轮只读，未跑资金差一分等所有历史验证矩阵 | 验证条款是验收范围，不能将测试文件存在说成当前测试全通过；未新增性能或真实性结论。 |
| 70–73：沪深官方规则来源及显式游戏简化 | ADR沿用0021官方来源，代码明确price_cage_enabled开关、固定价结果，未增加新市价类型 | 本轮没有变更交易制度，也没有补查外部官方材料；不存在新规则实施结论。 |

## 与既有缺口核对、候选反证

- G10仍成立：`market-chart-projection.ts:14`用帧累计差，`:37`以mergeMinutePoints覆盖同分钟；首帧previousVolume为0，也可能把竞价累计量当连续分钟量。
- G11仍成立：`:28`逐帧仅按日内槽合并，prices/auctions/continuousVolumes没有按day切换；`:67`rebuildHistory只重建给定帧集合。日终CivilUpdate可刷新，但不能保证每个先行TickBatch/跨日historical frames先清除旧日点。
- G12仍成立：`:15`连续buy硬编码true，`:24`用竞价价格非空判涨；`MobileStockDetail.tsx:153`直接使用该方向生成量柱。其语义是涨跌显示，不是真实主动成交侧。
- G13仍成立：`mobile/market-model.ts:655`仍取`slice(-7).reverse()`；需结合最新优先交易带修正。
- G14仍成立：`mobile/market-model.ts:663`仅算当前tradeTime，`MobileStockDetail.tsx:159`每笔复用该时间。
- “收盘没有独立曲线”已由ADR-0014:19及:24明确作为当前范围/未来扩轴，不新增G项。
- “竞价完成后没有补runtime_snapshot”候选排除：帧自身每tick携权威market，RuntimeDelta携working_orders及账户；消费者实际应用。不能继续以早期裸事件宿主接口审计当前提交帧协议。
- “旧compact_fastest_events函数有测试，所以压缩生产链完整”不能作为反证：当前Tauri `actor.rs:907`发布完整protocol批次，旧函数不在该生产入口调用链；本审计依据实际发布链保全竞价事实。
- “schema_version与ADR-0009删版本分支冲突”不新增G项：当前严格当前结构和runtime_v2提交证据校验保留，不支持旧格式迁移；旧ADR文字应按后来持久化协议理解。

本轮三篇全文的新增独立代码遗漏候选为 **0**。已实现反证不能核销总账G10–G14、G18/G19或G32；也不表示全部历史文档、所有运行路径或平台视觉验收已穷尽。
