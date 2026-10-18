# 重集成设计输入②：web 功能线盘点

> 生成：2026-10-18。方法：以 `agents/main-line-reintegration/retired-manifest.md` 的
> 269 个 web 退役文件为底，逐文件解析 `git show 18725d07:<path>` 的 import，与
> HEAD / a6001494（fork 点）三树比对依赖存在性与导出面；结论全部有 git 证据可复现。
> 只读分析，未改任何入库文件。

---

## 0. 三条总括结论

1. **ADR-0042（Trade maker/taker=腿方向）对退役 web 文件影响为 0。**
   全部 269 个退役文件中引用 `maker`/`taker` 的为 **0 个**（逐文件 grep 验证）。
   ADR-0042 自身的前端扫描也确认消费面全是对称判定/纯透传且都在**在树**文件里。
   唯一接触点是 `types/generated/Event.ts`（本就靠 ts_rs 重生成，不靠 checkout 取回）。
   真正要防的协议面变化是 **ADR-0041 数值股票 id → Coded\* 视图词表**（见 §2.4）。
2. **"可直接取回"的分界不是文件归属，而是三张依赖网**：
   - 引擎查询网（`MarketHistory`/`MinuteBar`/`CurrentMinuteHistory`/`PersonalTradeHistory`/
     `IntradayAverage`）：fork 点 **0 符号**，属 main 分叉后新建，且引擎侧
     `session/retained_history.rs`、`session/trade_confirmation_query.rs`、
     `session/protocol/civil/session.rs` 在 HEAD **全部消失**（引擎 0 符号）。
     依赖它的功能（分钟K线/保留历史/个人成交历史/成交确认数据面）必须先重建引擎查询 API。
   - 公共文件接线网：HEAD 仍在树的 `App.tsx`(+323)、`engine-host.ts`(+121)、
     `store/store.ts`、`store/chart-settings-slice.ts`、`PriceChart.tsx`、
     `mobile/calendar-candles.ts`、`ChartPeriodTabs.tsx`、`LocalRefreshViews.tsx`、
     `MarketKlinePanel.tsx`、`MobileStockDetail.tsx` 的 main 侧增改随合并退役，
     取回靠 `git diff HEAD 18725d07 -- <path>`，不能 checkout。
   - server/native-store 网：远程登录需要 `apps/server` 12 文件 + `native-store` 5 文件。
3. **清单缺登记（manifest gap）**：`apps/web/src/utils/turnover.ts` 在 fork 点不存在、
   18725d07 存在、HEAD 消失、**不在 retired-files.txt**（"影子退役"）。
   它被 K 线周期聚合（calendar-candles main diff）、分钟K线、host/market-history、
   host/intraday-average 四处 import。按 manifest 的取回命令取不到它，必须补
   `git checkout 18725d07 -- apps/web/src/utils/turnover.ts`。

---

## 1. 功能分组与逐组判定

### G1 K线周期（日/周/月/季/年聚合）— 3 文件
`components/KlinePeriodSelector.tsx`、`components/kline-periods.ts`（+2 test）。
- 判定：**纯前端自洽 + 一处小公共 diff**。
- 依赖面：`kline-periods.ts` 只 import `mobile/calendar-candles.ts` 的
  `CalendarCandlePeriod`。HEAD 的 calendar-candles **没有**该类型：HEAD 版只支持
  日/周/月 + `CandleAggregationBasis`（calendar|trading-days）参数；main 版换成
  日/周/月/季/年 五档、删 basis 参数、新增 `tradeStats`（成交额/笔数）聚合并 import
  `utils/turnover.ts`。取回 = 2 个退役文件 + calendar-candles main diff（需裁决
  basis 参数去留）+ 影子 turnover.ts。
- 数据面无引擎依赖：HEAD `wire-values.ts` 的 `parseDailyCandle` **已解析**
  `trade_stats`（turnover_cents + trade_count），`KlinePoint.tradeStats` 在 HEAD
  PriceChart 已存在。
- 接线面：`ChartPeriodTabs.tsx`（HEAD 在树）main diff。
- ADR-0042：无关。ADR-0041：无关（纯前端聚合，不碰 id 词表）。

### G2 分钟K线（1~120 分钟 bar）— 5 文件
`components/MinuteKlinePanel.tsx/.css`、`components/minute-kline.ts`（+2 test）。
- 判定：**依赖协议字段（引擎查询网），组件层可先取回但无数据**。
- 依赖面：`host/market-history.ts`（MarketHistoryRequest/Page/MinuteBar，六位 code
  寻址）、`host/current-minute-history.ts`（当日分钟历史）、`host/personal-trade-history.ts`
  （仅用其 `tradeHistoryDate`）。引擎侧 `MinuteBar`/`CurrentMinuteHistory*` 在 HEAD
  与 fork 点均为 0 符号 —— 整条查询 API 是 main 线新建且已全灭。
- 在位依赖全部可用：`parseDecimalId`（HEAD wire-values 在）、`KlineCoordinatePlot.tsx`、
  `kline-coordinates.ts`、`mobile/market-model.ts`、`utils/format.ts` 均在 HEAD。
- normalize 层自带 A 股时段校验（OpenAuction=09:25、Continuous 09:30–11:30/13:00–15:00、
  ClosingAuction=15:00），语义可直接复用。
- ADR-0042：无关。ADR-0041：无关（code 串寻址，不走数值 id）。

### G3 均线设置 + 指标源策略 — 8 文件
`MovingAverageSettings.tsx`、`moving-average.ts`、`useMovingAverageSettings.ts`、
`config/moving-average-settings.ts`、`indicator-source-policy.ts`（+3 test）+
`store/indicator-source.test.ts`。
- 判定：**组件半可直接取回；接线半在公共 diff**。
  - 可直接取回：`MovingAverageSettings.tsx`（只 import config 文件）、
    `config/moving-average-settings.ts`（零 import，localStorage 自洽）、
    `moving-average.ts`（import HEAD 在位的 `PriceChart.KlinePoint` + `utils/money`）。
  - 需公共 diff：`useMovingAverageSettings.ts` 依赖 `chart-settings-slice.setChartAverageSettings`
    与 `store.IndicatorDataSource` —— 两者 HEAD 均无（main diff 把均线周期从固定
    `KLINE_MOVING_AVERAGES` 白名单改为用户配置 `averagePeriods`）；`PriceChart.tsx`
    main diff 用 `exactMovingAverage`/`MOVING_AVERAGE_COLORS` 重画 MA 覆盖层；
    `indicator-source-policy`（前端算 vs Rust 指标源，rust 源不支持 MA 显式报错不回退）
    挂在 store 词表上。
- HEAD 现状：PriceChart 有 `movingAverageDays` prop + runtime `updateMovingAverages`
  （固定周期），MA 设置是给它换"用户可配"外壳。
- ADR-0042/0041：无关。

### G4 远程登录/登出 — 14 文件
`app/RemoteLoginScreen.tsx`、`app/remote-logout.ts`、`auth/credential-store.ts`、
`host/remote-auth.ts`、`host/remote-market-context.ts`、`host/remote-test-context.ts`、
`store/remote-membership.ts`（+7 test）。
- 判定：**深耦合旧会话 API + server 面，需重接**。
- 协议面：HTTP fetch（`/api/market/context` 等普通 JSON + token 头），不碰
  Trade 事件流 → ADR-0042 无关。**但 ADR-0041 有影响**：main 侧 store 用裸
  `RuntimeDelta/Snapshot/TradeEvent`，HEAD store 已换成
  `CodedRuntimeDelta/CodedSnapshot/CodedTradeEvent`（stock-identity.ts 摄入层翻译）；
  `remote-membership.ts`（选玩家账户/控制权）取回时须对齐 Coded\* 词表。
- 耦合链：`RemoteLoginScreen` import `config/seed-draft.ts` →
  `parseCompanySystemConfig`（save/schema/company/system-config.ts，公司线）→
  新局创建表单把公司配置当 prop 传给远程登录。**公司耦合可裁剪**：远程登录本质
  只需要 `parseSessionSeed` + setup，公司配置参数可先降级为可选。
- server 面：`apps/server/src/identity_routes.rs`、`identity.rs`、`routes/archives.rs`
  （+9 test）与 `packages/native-store` 5 文件（身份/归档原生存储）一起退役，
  重集成需整面取回。
- 接线面：`App.tsx` main diff（RemoteLoginScreen + ArchiveManager 挂载）。

### G5 成交确认表 — 2 文件（+1 schema test）
`components/TradeConfirmationTable.tsx`（+test）、`save/schema/personal-trade-confirmations.test.ts`。
- 判定：**可直接取回（纯展示组件）**。唯一 import 是 `utils/money.centsToYuanText`
  —— HEAD 在位且导出相同。
- 但其数据源 = G6 的 `PersonalTradeHistoryPage`，单独取回只能编译不能出数。
  语义注意：成交确认（买卖双腿+费用）若未来展示主动方向，ADR-0042 勘误明确
  **腿方向不能推导主动方向**，主动方向需价格对比信号（在树 priceHistorySlice 的
  染色做法可参考）。

### G6 个人成交历史 — 4 文件
`components/PersonalTradeHistoryPanel.tsx`、`host/personal-trade-history.ts`、
`app/private-history-query.ts`（+test）。
- 判定：**依赖协议字段（引擎查询网）**。host 层归一化自洽（import 自身 + 
  `intraday-average.ts` 的游标归一），但 `PersonalTradeHistoryRequest/Page` 类型由
  ts_rs 生成、引擎查询在 `session/trade_confirmation_query.rs`（HEAD 消失）。
- 视图类型挂在 `engine-host.ts`（main diff +121 行，含 `IntradayAverage*` 与
  `PersonalTradeConfirmation`），取回需连该 diff 一起评估。

### G7 存档管理 — 5 文件
`components/ArchiveManager.tsx`、`save/archive-store.ts`、`save/indexeddb-save-repository.ts`
（注意：无连字符，与 HEAD 在树的 `indexed-db-save-repository.ts` 是两个文件）（+2 test）。
- 判定：**可直接取回（组内自洽、在位 API 兼容）**。
  依赖的 `save/save-schema.ts:parseSaveSlot`、`save/day-end-candidate.ts:validateDayEndArchive`、
  `save/schema/root.ts:StrictSaveEnvelope` 全部在 HEAD 且导出名一致。
- 需裁决（非阻塞）：与 HEAD 现存 `save-repository.ts` / `indexed-db-save-repository.ts` /
  `day-end-persistence.ts` 存档栈**功能重叠**（两套 IndexedDB 仓库）。建议取回时
  以 ArchiveManager 的 UI + 现树 repository 后端做合并，而不是双栈并存。
- 接线面：`App.tsx` main diff（`managedArchives` 状态）。

### G8 保留历史（近五交易日）— 7 文件
`components/RetainedHistoryPanel.tsx/.css`、`retained-history-model.ts`、
`save/schema/retained-history.ts`（+3 test）。
- 判定：**依赖协议字段（引擎查询网）**。面板与 model 只依赖
  `host/market-history.ts` 的 MarketHistory 类型；引擎侧对应
  `session/retained_history.rs`（HEAD 消失）。与 G2 共享同一查询 API。

### G9 公司系统 web 面 — ~80 文件
`components/company/` 18、`config/company-initial-preset.ts`/`seed-draft.ts`、
`save/schema/company/` 24、`save/fixtures/` 14、`host/` 公司视图 8、
机制开关/面值/税制/报告频率输入件 8、若干 schema test。
- 判定：**深耦合引擎公司模块（116 文件）**，与公司线重集成战役绑定，单独取回
  无意义。注意 HEAD 的 `save/schema/company/` 基座（value.ts、accounting/journal、
  books/industrial 等）**仍在树**（fork 前共有），退役的只是 main 侧 24 个增量
  schema 文件 —— 重集成冲突面比想象小。
- 接线面：`engine-host.ts` 公司查询视图（main diff）+ App 路由。

### G10 生成类型 — 108 文件（types/generated/）
- 判定：**不取回，重生成**。ts_rs 产物必须从当前 engine 重新导出（公司 ~88 个 +
  功能线 ~20 个：MarketHistory\*4、MinuteBar、MinuteHistorySession、
  CurrentMinuteHistory\*3、PersonalTradeConfirmation/Page/Request、
  IntradayAverage\*2、RetainedHistoryDay、TradeHistoryReceiptCursor、
  MarketMembership\*2、HistorySessionStatus/TradingPhase、FeeComponents 等）。
  checkout 旧文件会覆盖新架构生成物。
- ADR-0042 落点即在此层：`Event.ts` 注释变更后重跑导出即可，无手工改动。

---

## 2. 协议契约对照明细

### 2.1 ADR-0042（maker/taker 腿方向）
| 面 | 状态 |
|---|---|
| 退役 web 269 文件 | **0 引用** maker/taker（逐文件 grep） |
| 在树消费点（App.tsx/player-orders/parse.ts/…） | ADR-0042 已自证必改 0 处 |
| Event.ts | 随 ts_rs 重生成，无手改 |
| 存档/证据基准 | verification_evidence 的 Trade 基准值需重算（ADR 已登记，非 web 文件） |

### 2.2 ADR-0041（数值股票 id → Coded\* 视图）
受影响取回面：`store/remote-membership.ts`（裸 RuntimeDelta/Snapshot 词表）、
`host/intraday-average.ts`/`engine-host.ts` main diff 里的视图类型。取回时按
HEAD 的 `host/protocol/stock-identity.ts` Coded\* 形态对齐。
`MarketHistory` 系查询用六位 code 串寻址，不受影响。

### 2.3 引擎查询网（HEAD=0 的契约面）
`MarketHistoryRequest/Page/Entry/Availability`、`MinuteBar`、`MinuteHistorySession`、
`CurrentMinuteHistoryRequest/Response/Phase`、`PersonalTradeHistoryRequest/Page`、
`PersonalTradeConfirmation`、`TradeHistoryReceiptCursor`、`IntradayAverage*`、
`HistorySessionStatus/TradingPhase`、`RetainedHistoryDay` —— 引擎 `packages/engine/src`
在 HEAD 全部 0 符号；对应实现在 18725d07 的 `session.rs`、`session/retained_history.rs`、
`session/trade_confirmation_query.rs`、`session/protocol/civil/session.rs`。

### 2.4 在位且兼容的契约面（取回即可用）
`host/protocol/guards.ts`、`wire-values.ts`（parseMoney/parseDailyCandle/parseDecimalId）、
`save/save-schema.ts`、`save/day-end-candidate.ts`、`save/schema/root.ts`、
`save/schema/market.ts`、`utils/money.ts`、`utils/format.ts`、`PriceChart.KlinePoint`、
`KlineCoordinatePlot`、`kline-coordinates`、`mobile/market-model`、`types/engine.ts`
的 `SessionSetup/Cents`、`config/defaults.ts`/`session-seed.ts`。

---

## 3. 重集成排序建议（先易后难）

**批次 1 — 零公共面直接取回（`git checkout 18725d07 -- <显式 pathspec>`）**
```
apps/web/src/components/TradeConfirmationTable.tsx apps/web/src/components/trade-confirmation-table.test.ts
apps/web/src/components/MovingAverageSettings.tsx
apps/web/src/components/moving-average.ts apps/web/src/components/moving-average.test.ts
apps/web/src/config/moving-average-settings.ts apps/web/src/config/moving-average-settings.test.ts
apps/web/src/components/ArchiveManager.tsx
apps/web/src/save/archive-store.ts apps/web/src/save/archive-store.test.ts
apps/web/src/save/indexeddb-save-repository.ts
```
（组件级自洽可编译；显示价值需后续批次接线。存档管理与现树 repository 的合并策略
在取回时一并裁决。）

**批次 2 — 小公共 diff 应用（K线周期 + 均线接线）**
- 影子补取：`git checkout 18725d07 -- apps/web/src/utils/turnover.ts`（manifest 缺登记）
- K线周期：`KlinePeriodSelector.tsx`+`kline-periods.ts`+`git diff HEAD 18725d07 --
  apps/web/src/mobile/calendar-candles.ts apps/web/src/components/ChartPeriodTabs.tsx`
  （裁决 basis 参数与季K/年K/tradeStats 的合并）
- 均线接线：`useMovingAverageSettings.ts`+`indicator-source-policy.ts`+`git diff` 应用到
  `chart-settings-slice.ts`/`store.ts`/`PriceChart.tsx`/`MarketKlinePanel.tsx`

**批次 3 — 引擎查询网重建（一次战役带四个面板）**
重建 `MarketHistory`/`MinuteBar`/`CurrentMinuteHistory`/`PersonalTradeHistory`/
`IntradayAverage` 查询 API（参考 18725d07 session 侧实现 + ADR-0041 数值 id 适配），
ts_rs 重生成 ~20 个功能线类型，然后取回 G2/G5数据面/G6/G8 的 host 层与面板，
接线 `LocalRefreshViews.tsx`/`MobileStockDetail.tsx`/`engine-host.ts` diff。

**批次 4 — 远程登录（server + native-store 面）**
取回 server 12 文件 + native-store 5 文件 + web 14 文件；裁剪 seed→公司配置耦合
（公司参数降级可选）；`remote-membership` 对齐 Coded\* 词表；App.tsx 接线。

**批次 5 — 公司线绑定（随公司重集成战役）**
G9 全部 + 生成类型 ~88 公司文件 + engine-host/App 公司接线。

---

## 4. 数字汇总

| 指标 | 值 |
|---|---|
| 功能组数 | 10（G1–G10） |
| 可直接取回（组级） | 3 组可整组/主体取回：G5 成交确认表、G7 存档管理、G3 组件半（6 文件） |
| 受 ADR-0042 影响 | **0**（maker/taker 零引用） |
| 受 ADR-0041 影响（需词表对齐） | 2 面：remote-membership/store、engine-host 视图类型 |
| 依赖引擎查询重建 | 4 组：G2 分钟K线、G6 个人成交历史、G8 保留历史、G5 数据面 |
| 深耦合 server/native-store | 1 组：G4 远程登录 |
| 深耦合公司线 | 1 组：G9（~80 文件）+ G10 公司部分（~88 生成类型） |
| manifest 缺登记（影子退役） | 1 文件：`apps/web/src/utils/turnover.ts` |
