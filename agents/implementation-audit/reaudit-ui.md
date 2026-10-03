# UI/host/store 实现复审

## 范围与判定口径

按任务要求只做静态源码复核，不实现游戏代码、不运行测试或全量回归。以当前工作树消费链及 CSS 为准；测试源码只用于确认可能存在的覆盖，不能代替消费行为。行号指当前工作树。

状态含义：**仍缺**为生产行为仍违约；**部分修复**为局部行为改善但完整要求未满足；**旧误判**为基线报告的断言已不再成立；**仍开放**为需求/口径尚无裁决，不能替项目决定。

## G10–G14：分时与逐笔

| ID | 结论 | 当前生产证据 |
|---|---|---|
| G10 | **仍缺** | `apps/web/src/app/market-chart-projection.ts:8-16,28-43` 每帧以累计成交量减上一帧值作为该分钟量，再由 `mergeMinutePoints` 按分钟覆盖旧点（`apps/web/src/mobile/market-model.ts:565-570`）。同一分钟多个帧的差值不会累加，最后只保留最后一个小增量；`continuousVolumes` 在集合竞价时没有用集合竞价累计量初始化，首个连续点仍可能把竞价量算入连续量。实际路径是 protocol frame → `MarketChartProjection.upsertFrames` → hook 更新 `chartData` → `MobileIntradayProjection.volumeMarks`，故不是仅旧 collector 的问题。连续点方向目前固定 `buy: true`（projection:15），也影响 G12。 |
| G11 | **仍缺** | `continuousPoint` 和 `auctionPoints` 把 tick 映射到日内 minute/auction 槽（`apps/web/src/app/market-chart-projection.ts:8-25`）；跨自然日持续更新时没有日界清理或按日期隔离。`mergeMinutePoints` 用槽位 `time` 作为唯一键（`apps/web/src/mobile/market-model.ts:565-570`），同分钟新点覆盖旧日点，尚未到达的新日分钟槽则保留旧日点。完整快照重装/重建路径不能证明普通日界增量路径已清理。
| G12 | **仍缺** | 连续点涨跌方向被固定成 `true`（`apps/web/src/app/market-chart-projection.ts:15`）；竞价点的 `buy` 仅由指示价是否为空决定（同文件:24），都不是按相邻有效价格判定。移动分时量柱 CSS 对 `.rise` 仍显式 `border:0` 且实心背景（`apps/web/src/mobile/MobileStockDetail.css:218`）；渲染消费 `point.buy` 决定 `.rise/.fall`（`MobileStockDetail.tsx:153`）。因此并未实现红色空心涨柱、绿色实心跌柱。这里的颜色是价格方向，不代表主动买卖方向。
| G13 | **仍缺** | store 维持最新成交在前且最多 100 条（`apps/web/src/store/store.ts:99-121`）；详情先按股票过滤（`apps/web/src/app/LocalRefreshViews.tsx:177-178`），随后 `MobileIntradayProjection` 对数组执行 `slice(-7).reverse()`（`apps/web/src/mobile/market-model.ts:655`）。这是从最新优先数组尾部取较旧成交，再倒序展示；实际并非最近七笔。
| G14 | **仍缺** | 逐笔列表每行显示同一个 `projection.tradeTime`（`apps/web/src/mobile/MobileStockDetail.tsx:158-160`）；该字段由当前 `elapsedMinutes` 生成（`apps/web/src/mobile/market-model.ts:663`），而非逐笔事件自身时间。当前 `TradeEvent` 映射未见时间字段消费，旧成交会随当前时间显示。

## G22–G25：表单、焦点、滚动与点击区

| ID | 结论 | 当前生产证据 |
|---|---|---|
| G22 | **仍缺** | 数量输入仍是普通 `InputGroup`，没有 `aria-invalid`、字段错误关联或字段级消息（`apps/web/src/App.tsx:566`）；提交错误由 `useTradingCommands` catch 后写入全局 notice（`apps/web/src/app/useTradingCommands.ts:74-83`）。错误并未被静默吞掉，但没有即时/字段级反馈。
| G23 | **仍缺** | 打开详情仅调用 `selectChart`、更新交易代码并派发 `open-detail`（`apps/web/src/App.tsx:202-208`）；详情返回仅派发 `back`（同文件:669）。移动 UI controller 的焦点管理只覆盖交易底页（`apps/web/src/app/useMobileUiController.ts:27-57`），没有详情进入后聚焦返回按钮或返回后恢复原股票行焦点的实现。
| G24 | **仍缺** | `showDetailInfo` 在更换信息 tab 后仍对 `.msd-info-tabs` 调用 `scrollIntoView`（`apps/web/src/app/useMobileUiController.ts:71-74`），会主动改变纵向滚动位置，违背保持详情滚动位置的约定。
| G25 | **仍缺** | 后置移动端 CSS 将切股按钮宽度限制在 22–26px（`apps/web/src/mobile/MobileStockDetail.css:194-195`），返回按钮所在首列为 30–35px（同文件:188-193）；按钮高度虽为 44px（同文件:190），但横向热区不足 44px。CSS 只能支持源码尺寸判断，本次未做浏览器像素测量。

## G30–G34：图表、局部更新与视觉契约

| ID | 结论 | 当前生产证据 |
|---|---|---|
| G30 | **仍缺** | 通用按钮有可见焦点样式（`apps/web/src/index.css:118-122`），但图表窗口按钮的更具体规则将 outline 颜色设为未定义的 `var(--msd-focus)`（`apps/web/src/mobile/MobileStockDetail.css:218`），该声明无效并覆盖通用 outline；基准 CSS 未定义 `--msd-focus`。SSR/按钮名称不能证明键盘焦点可见。
| G31 | **仍缺** | 每个 applied reduction 后都无条件对所选股票调用 `projection.pricePointsFor`、`auctionPointsFor`、`candlesFor` 并 setState（`apps/web/src/app/useMarketChartRuntime.ts:72-75`）；这些 accessor 每次都创建新数组（`apps/web/src/app/market-chart-projection.ts:94-99`）。因此即使当前股票数据未改变也会换引用、更新 DataContext（`MarketRuntimeProvider.tsx:78-84`），Provider 拆分没有实现当前股票图表数据引用隔离。
| G32 | **仍缺** | 分时 SVG 的可绘图框上下扣除 23px 时间轴（`apps/web/src/mobile/MobileStockDetail.css:80`），但昨收/0.00% 轴用整个图表容器的 50%（同文件:79、`MobileStockDetail.tsx:121-126`）。按现有尺寸，中心相差时间轴高度的一半；设计稿的昨收中轴与零轴不能严格重合。未做浏览器像素验收。
| G33 | **仍缺（部分组件原已响应式）** | 基础报价、盘口等存在 `clamp()`/`cqw`（`apps/web/src/mobile/MobileStockDetail.css:30-62,76`），但后置参考尺寸规则又固定报价、摘要和盘口字号为 px（同文件:199-207）。组件部分仍响应式，不能据此说全页固定；这些局部能力不是本次重构新修复。关键行情字号在 320–430px 并未完整响应容器宽度。未做该宽度矩阵实测。
| G34 | **仍缺** | `prefers-reduced-motion` 覆盖位于 `.mobile-stock-detail` 子树（`apps/web/src/mobile/MobileStockDetail.css:171-173`）；交易底页由 `.layout-mobile .order-panel.mobile-sheet` 控制并保留 300ms transform transition（`apps/web/src/App.css:515-528`），不在详情子树内，因而不受该规则约束。

## Q04/Q07/Q08：未决口径

| ID | 结论 | 当前证据与边界 |
|---|---|---|
| Q04 | **仍开放** | `apps/web/src/app/useMobileUiController.ts:20-22` 将详情时的 `document.title` 设为“股票名 — 股票模拟游戏”，而 UX-CONTRACT 的文档标题条款写应用级标题保持“股票模拟游戏”。这是代码与规范的冲突，不能将代码现状当成裁决；须由需求方确定页面 title 要不要随个股详情变化。
| Q07 | **仍开放** | 周/月 K 线聚合仍由前端 `aggregateCandles`（`apps/web/src/mobile/market-model.ts:329-354`）决定组周期。UX-CONTRACT 只列周期名，未给出自然周/月边界及合成历史衔接规则；现实现不足以证明与预期口径一致。
| Q08 | **仍开放** | 移动详情均价由可见分时价格点算术平均推导（`apps/web/src/mobile/market-model.ts:644-646`，消费于 `MobileStockDetail.tsx:118-120`），代码注释说明它不是撮合均价或 VWAP；没有从引擎消费权威均价。该实现透明标出局限，但“允许展示派生值还是必须引擎权威”的移动 QA 与 UX 口径冲突仍需裁决，不能宣称真实均价已实现。

## `b89afb3..8cf34a1` 差异复核

已按只读方式检查该区间 UI、host、store 生产源码 diff；没有用主工作树与本工作树的差异代替提交区间。`store/store.ts`、`App.css`、`index.css`、`MobileStockDetail.css` 及 `useMobileUiController.ts` 在该区间没有改动；因此 G13 的最新优先顺序、G22/G23 的表单/详情处理、G24 的主动滚动、G25 与 G30/G32/G33/G34 的样式问题均非本区间新增，也没有本区间修复后又被移除的证据。

G10–G14 的旧生产逻辑从 `useMarketChartRuntime.ts` / `MobileStockDetail.tsx` 拆分到 `MarketChartProjection` 与 `MobileIntradayProjection`；diff 显示量差值按每帧累计量计算、按分钟覆盖、连续方向固定 `buy: true`、最近成交仍 `slice(-7).reverse()`、逐笔仍共用当前 `tradeTime`。抽取未改变这些行为，新增 projection 测试也没有改变生产消费链。因此这些仍缺项已存在于 `b89afb3`，本区间没有移除一个已正确实现的版本。

`App.tsx` 中宿主生命周期、委托、存档与速度/暂停逻辑迁入 hooks；检查对应新文件与原调用边界，未发现会改变上述 UI 条款的新增遗漏。host 适配器将连接、基线游标、请求注册等状态提取为独立 owner；`store/store.ts` 没有提交区间改动，UI/store 行情字段契约未被本区间重写。该次 refactor 未发现对 G10–G14、G22–G25、G30–G34 或 Q04/Q07/Q08 新增的行为回归。

结论仍以静态 diff 与当前生产源码为限；未运行测试、浏览器验收或回归。CSS 像素与交互手感仍需实际浏览器矩阵验证。
