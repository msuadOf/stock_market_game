# 移动 UI 与 Host 消费独立复核（luna02）

## 范围与方法

- 目标基线：`08e4fc75b52a71a3262a8a938c57b44f8b5b4960`。当前 worktree HEAD 为 `a7c7ce3`；`git diff --name-status 08e4fc7 HEAD -- apps/web/src/App.tsx apps/web/src/app apps/web/src/mobile apps/web/src/store/store.ts apps/web/src/components/MarketGrid.tsx` 无输出，故下列目标生产源码行与基线相同。
- 连续读至 EOF：`DESIGN.md` 129 行、`UX-CONTRACT.md` 96 行、`design/ui/mobile/qa/README.md` 5 行，共 230 行；同时读取 `AGENTS.md` 123 行与 `docs/principles.md` 92 行。
- 仅静态追踪生产 UI 与其 Host / protocol / Redux 消费关系；没有运行测试、构建、浏览器或完整回归，没有修改产品代码或 Git 状态。本记录为唯一新增文件。
- 本复核涉及的产品契约不引入新的 A 股交易规则：交易事实继续来自 engine / `Snapshot` / `PriceTick` / `AuctionTick` / `TradeEvent`；前端绘图及显示不得伪造事实。文档/注释使用中文，代码标识保持原英文。

## 逐章矩阵

| 连续阅读的章节与原文行 | 当前生产组件与 Host 消费 | 复核结论 |
|---|---|---|
| DESIGN front matter，1–36 | `apps/web/src/index.css:3` 与 `mobile/MobileStockDetail.css:1` 提供运行时 token；移动详情组件消费后置样式。 | Token 映射存在，不代表视觉/对比度验收完成。 |
| Overview、Creative North Star、Product context，40–56 | `App.tsx:524` 保留桌面工作台；移动详情走 `ConnectedMobileDetail` → `MobileStockDetail`，四象限由 `.msd-market-composite` 布局。 | 信息结构主干符合；HTML locale 和初始标题另见 N05/Q04。 |
| Colors，58–60 | 报价颜色由 `MobileStockDetail.tsx:219-221` 消费；盘口颜色/深度见 `:63-67` 与 CSS `:115-116`；成交量由 `point.buy` 决定 class，CSS `MobileStockDetail.css:125-126`。 | 红涨绿跌报价已用；移动分时量方向/空心表现仍有 G12；颜色 AA 候选 C01。 |
| Typography，62–64 | 基础字号使用 `cqw` / `clamp()`，但 CSS `MobileStockDetail.css:199-207` 在较后规则固定若干行情字号。 | G33 仍成立，未做 320/390/430px 浏览器核验。 |
| Layout，66–68 | 移动详情顺序见 `MobileStockDetail.tsx:218-237`；图、盘口、量、逐笔两行布局见 `MobileStockDetail.css:74`。 | 主结构存在。 |
| Elevation & Depth，70–72；Shapes，74–76 | 数据结构使用细分隔线；交易底页阴影/圆角在 `App.css`。 | 静态主干存在，未做像素验收。 |
| Components / Foundational visual states，78–82 | 标签与周期状态在 `MobileStockDetail.tsx:223-234`；禁用按钮、焦点规则在 `MobileStockDetail.css:171-173,218`。 | 图表窗口 focus token 未定义，G30 仍成立；其它状态不能替代键盘实测。 |
| Navigation and data display：顶栏、点击热区、时钟、倍速，84–96 | `App.tsx:463` / `LocalRefreshViews.tsx:62-65` 装配全局时钟；详情由 `MobileStockDetail.tsx:204-206` 装配同组件，二者读 `day/tick`。共享暂停/倍速控件在详情 `:209-216`。 | 时钟与控件消费接通；标题栏返回/切股按钮宽度仍不足 44px（G25）。 |
| Navigation and data display：周期/信息独立、分时与集合竞价，84–96 | `useMarketChartRuntime.ts:51-75` 消费 `ProtocolReduction`；`MarketChartProjection` 映射 frame；`MobileIntradayProjection` 映射槽位和 scale；`MobileStockDetail.tsx:127-138,142-154` 绘制。信息/周期 reducer 在 `mobile-ui-state.ts:73-76`。 | G10–14、G32 与 sweep02 N01/N04 仍成立；图表/信息状态独立已实现。 |
| Navigation and data display：主导航、列表缩略图、叠层，84–96 | `App.tsx:667-684` 详情/主导航/遮罩；`MarketGrid.tsx:193-210` 将行情数据转成缩略 SVG；全局层级在 `index.css`。 | N02 来源主页面丢失仍成立；列表迷你图生产链存在。 |
| Scroll ownership，98–100 | `.app-grid` / `.mobile-detail-page` / `.order-panel.mobile-sheet` 样式与 `App.tsx:667-684` 主体装配；底页内部关闭/焦点逻辑在 `useMobileUiController.ts:25-56`。 | 滚动分区主干存在；信息切换主动滚动违反保持位置条款，G24。 |
| Iconography and motion，102–104 | 导航图标配文字；`useMobileUiController.ts:31-55` Escape/焦点边界；底页 300ms transition 在 `App.css`；减动效规则仅在 `.mobile-stock-detail` 子树。 | G34 仍成立；详情焦点恢复 G23 仍成立。 |
| Content and data visualization：文本替代、单位、深度、大数，106–112 | 五档 `FiveLevelBook` 消费 `market.bids/asks` (`MobileStockDetail.tsx:59-68`)；行情量格式化在 `utils/format.ts`；资金页读 active candle `:166-178`。 | 移动盘口/量单位及权威累计消费主干存在；桌面档位标签 N03 仍成立；G22 字段错误未关联仍成立。 |
| Content and data visualization：午休横轴，114 | `market-model.ts:intradayChartX` 定义共享映射；`:127-138` SVG 网格与轴、`:142-153` 量槽消费同一 x。 | 共轴与午休压缩主干存在；G32 中轴对齐问题仍成立。 |
| Content and data visualization：K 线槽、蜡烛、窗口与历史，116–122 | `KlinePanel` (`MobileStockDetail.tsx:71-98`) 消费日 K 投影及 Rust KDJ；`MobileKlineProjection` 管理窗口、实体/影线/量柱；history 经 `MarketChartProjection.replaceSnapshot`。 | 固定槽/窗口/权威历史消费存在；零量仍被画正柱 N01；G30 焦点规则仍不成立。 |
| Do's and Don'ts，124–129 | 同列图量盘口由前述组件消费；资金方向无权威事件时显示明确说明 `MobileStockDetail.tsx:178`。 | 禁止伪造资金栏已落实；G13/G14 逐笔问题仍成立。 |
| UX Product context，3–10 | 交易日/240 分钟事实来自共享时钟、图表投影；`index.html:2` 声明 `lang="en"`。 | N05 仍成立。 |
| Business-context sources，12–19 | 引用 ADR、架构与产品来源，是约束来源而非独立 UI 组件。 | 没有额外生产调用点。 |
| Visual contract，21–26 | `data-theme` / CSS 变量实施主题；移动详情固定浅色。 | 静态规则存在，C01 的小字号对比冲突待设计裁决。 |
| Canonical UI Map，28–36 | 表单在 `App.tsx:553-566`；Toast `:688`；原生 select 在控件组件；层级全局管理。 | G22、G30 仍成立；选择器原生平台差异按合同接受。 |
| Navigation and responsive behavior：全部条目，38–60 | 远程控制由 `LocalRefreshViews` 按 Host capability 提供；行情基线/增量由 `ProtocolCoordinator` → runtime → Redux / 投影 → 组件消费。 | G10–14/G22–25/G30–34、N01–05、C01–03 与 Q04/Q07/Q08 逐项见后表；没有依据把已禁用周期升级成现行承诺。 |
| Flow ledger 六项操作，62–71 | 详情选择 `App.tsx:202-208`；返回 `:669`；周期/信息 handler `useMobileUiController.ts:71-74`；交易底页焦点 `:25-56`。 | N02、G23、G24 各自边界不同，均未核销。 |
| Async and resilience，73–82 | Host update 由 ProtocolCoordinator 处理；runtime 区分 civil update / frame (`useMarketChartRuntime.ts:51-75`)；effects 的 TradeEvent 入 store `:37-39`。 | UI消费链存在；G11 的日界风险要看实际 frame 重建与清理，不因存在 CivilUpdate 就核销。 |
| 局部刷新边界，84–89 | `MarketRuntimeProvider` 暴露 runtime selection/data/actions；详情读市场、账户、market code 和 trade slices (`LocalRefreshViews.tsx:170-180`)；runtime 每次 applied reduction 重取三个投影数组 (`useMarketChartRuntime.ts:72-74`)。 | G31 仍成立；引用变化的性能幅度未运行测量。 |
| Verification，91–96 | 文档要求短测、lint/build、视口/桌面/辅助技术和视觉检查。 | 本复核没有运行这些验证，不能声称通过。 |
| Mobile reference QA 全文，1–5 | 截图须从同一存档与 engine Snapshot 生成；动态行情区排除或单独验证。 | 契约正确；N01/N04 是绘图伪事实问题，不是静态截图基线问题。 |

## 既有 G 条目复核

| ID | 当前生产消费证据与判断 |
|---|---|
| G10 | **仍缺。** `market-chart-projection.ts:14,34-40` 用当前累计量减此前累计量，再以 `mergeMinutePoints` (`market-model.ts:566-570`) 按 minute 覆写。若同分钟先累计增加、随后持平，最终保留 0 而非本分钟总增量；`continuousVolumes` 也未由集合竞价初始化，首个连续量可能包含竞价累计量。生产路径是 `ProtocolReduction.frames` → `MarketChartProjection.upsertFrames` → `useMarketChartRuntime` → `MobileIntradayProjection.volumeMarks` → `MobileStockDetail.tsx:153`。 |
| G11 | **仍缺，且旧记录应精确限定。** `useMarketChartRuntime.ts:53-55` 在 `civil-update` 重建历史；`MarketChartProjection.rebuildHistory` (`market-chart-projection.ts:67-72`) 只以传入 intraday 重建旧/当前槽。项目另有 `currentTradingDayEvents` (`market-model.ts:572-576`)，但全仓生产搜索仅定义与测试引用，没有 UI/Host caller；不能据该 helper 认定生效。相邻交易日携带的完整历史或空屏障分别按各自 protocol 事实处理；不能从“完全无日界逻辑”推断，实质风险是新日按相同 minute key 合并时没有天然日期维度。保留 sweep02 所述相邻日复用旧槽风险，避免将非相邻休市日清屏案例当作反证。 |
| G12 | **仍缺。** `market-chart-projection.ts:15` 连续 `buy: true`；竞价 `:24` 只用 indicative price 是否 null；CSS `MobileStockDetail.css:125-126` 红涨量柱实心且无边框。投影消费在 `MobileStockDetail.tsx:153`。颜色方向是价格方向，不可解释为主动买卖方向。 |
| G13 | **仍缺。** `store.ts:116-120` 按最新优先存最多 100 笔；`LocalRefreshViews.tsx:177-178` 按股票过滤；`market-model.ts:655` 对最新优先数组用 `slice(-7).reverse()`，因而取得较旧而非最近七笔。 |
| G14 | **仍缺。** `MobileStockDetail.tsx:158-160` 每行使用同一个 `projection.tradeTime`；`market-model.ts:663` 从当前 elapsed minute 计算，`TradeEvent` 入 store 的 `useMarketChartRuntime.ts:37-39` 没有附事件时间。 |
| G22 | **仍缺。** `App.tsx:565-566` 价格、数量均无 `aria-invalid`、字段关联错误或字段级消息；提交失败由 `useTradingCommands` 显式 notice。属于反馈关联不足，不是吞错。 |
| G23 | **仍缺。** `App.tsx:202-208` 选股后只更新 chart/trade code 并 dispatch `open-detail`；`:669` 返回只 dispatch `back`。`useMobileUiController` 焦点陷阱只为交易底页管理，未发现打开详情聚焦返回键或返回后还原来源股票行的逻辑。 |
| G24 | **仍缺。** `useMobileUiController.ts:71-74` 信息标签每次切换均 `scrollIntoView({block:"start"})`，与 UX 保持详情滚动位置直接冲突。 |
| G25 | **仍缺。** `MobileStockDetail.css:186-195` 返回网格列宽 30–35px、切股按钮 22–26px；`:190` 的高度 44px 不补横向热区。未做浏览器 hit target 实测。 |
| G30 | **仍缺。** `MobileStockDetail.css:218` 将图表工具焦点 `outline` 指向未定义 `var(--msd-focus)`，覆盖通用按钮 outline；基准样式未定义此变量。 |
| G31 | **仍缺。** 每个 applied reduction 均调用 `setChartData` / `setAuctionChartData` / `setDailyChartData` (`useMarketChartRuntime.ts:72-74`)；`market-chart-projection.ts:94-99` 总返回新数组，变更普通未选证券时当前图表仍换引用。代码证据确定，实际渲染成本未测。 |
| G32 | **仍缺。** 绘图 SVG (`MobileStockDetail.css:80`) 高度扣除 23px 时间轴；CSS 零轴 `:79,93` 以整个 `.msd-intraday-chart` 50% 定位，而价格投影 `priceY` 使用 SVG 内 8..92 坐标 (`market-model.ts:667-669`)。SVG 有效绘图区的昨收中轴与背景零轴不重合。 |
| G33 | **仍缺。** 基础容器字号有 `cqw`，但后置参考尺寸规则 `MobileStockDetail.css:199-207` 对报价/摘要/盘口设置固定 px。仅这些关键字号局部未随 320–430px 容器按规范响应，不泛称全页固定。 |
| G34 | **仍缺。** 减少动效仅覆盖 `.mobile-stock-detail` 子树 (`MobileStockDetail.css:171-173`)；交易底页是独立 App 节点，`App.css:515-528` 的 transform transition 仍为 300ms。 |

## sweep02 候选逐项复核

| 项目 | 复核结论与当前证据 |
|---|---|
| N01 零成交量正柱 | **仍成立。** `MobileIntradayProjection.volumeMarks` 对 0 量用 `Math.max(1, ...)` (`market-model.ts:686-694`)；`MobileKlineProjection.volumeMarks` 同样 `Math.max(1, ...)` (`:771-772`)；组件按 height 绘制 `MobileStockDetail.tsx:153,96`。真实零量槽可以存在，但图形高度不应伪装成非零。与 G10 的增量计算错位独立。 |
| N02 自选进入详情返回变行情 | **仍成立。** `reduceMobileUi` `open-detail` 强制 `primaryTab:"market"` (`mobile-ui-state.ts:67-72`)，`back` 仅清 `detailCode` (`:81-84`)。即使列表保持挂载，当前导航身份已经改变。 |
| N03 桌面卖档标号倒置 | **仍成立。** `LocalRefreshViews.tsx:113` 对 `market.asks.slice(0,5)` 按现有顺序渲染，却标 `卖${5-index}`。移动端 `buildFiveLevelBook` 以 `asks[0]` 为最优档；桌面 `ConnectedChartPanel` 与移动端因此不一致。此处仅核对显示序号，撮合档序仍属权威 engine。 |
| N04 竞价 null 价槽被连线 | **仍成立。** `MobileIntradayProjection` 将 null 点过滤到 `visibleAuctionPricePoints` (`market-model.ts:645-647`)，再将剩余所有点 join 成单串 (`:671-673`)；组件 `MobileStockDetail.tsx:133` 绘单 polyline。有效价、null、有效价之间仍会跨槽绘线。 |
| N05 中文页面 lang 英文 | **仍成立。** `apps/web/index.html:2` 固定 `lang="en"`；本复核未发现 UI/Host 对 documentElement.lang 改写。 |
| C01 规范颜色与 WCAG AA 对比 | **候选仍成立。** 原记录给出红/绿/金/蓝小字号对比值；当前设计 token 与盘口小字消费点仍在。没有独立重算对比或做 computed-style 验收；不能擅改领域涨跌色。 |
| C02 导航/行情分类缺辅助技术选中态 | **候选仍成立。** `App.tsx:674-680` 主导航仅有 `active` class，无 `aria-current/pressed/selected`；`MarketGrid.tsx:177-182` 行情分类同样仅 CSS class。周期/信息 tab 有 `aria-selected`，所以范围限这两处。 |
| C03 涨跌停快捷价静态默认证券规则 | **候选仍成立，产品边界需要主审裁决。** 正式委托提交使用 `activeSetup`；但 `LocalRefreshViews.tsx:129` 从 `DEFAULT_SETUP.stocks` 取证券 category。动态证券能显示行情而未必有默认规则；不能由此推导新增证券类别产品需求。 |

## sweep15 与 sweep41 候选复核

| 项目 | 当前生产证据与结论 |
|---|---|
| sweep15 C15-01 Web 成本与 Account 派生契约 | **仍是可验证跨层候选，未核销。** `portfolio-selector.ts:9-17` 从权威 account / held market 构造输入；`LocalRefreshViews.tsx:23-46` 用未舍入 `netInvested / qty` 派生成本、用市值减未舍入净投入派生浮盈亏，`:137` 展示。相对 account 领域约定的 half-to-even `cost_price` 和按已舍入成本计算的 `unrealized_pnl`，该 UI 不消费同一派生值。旧记录的小额合法例不依赖大金额范围 Q01。是否 UI 合同要求复用 Account 派生仍需主审决定；不将自己计算出来的例子说成已运行测试。 |
| sweep15 C15-02 “负成本显示 -” 文档歧义 | **仍为歧义，不确认代码缺陷。** 现持仓 UI 展示成本金额而无成本收益率百分比 (`LocalRefreshViews.tsx:137`)；旧句中金额/百分比对象含混，ADR-0012 对成本收益率的解释不等于要求把负成本金额抹除。 |
| sweep41 S41-N1 AG Grid 辅助文本英文 | **仍成立，需辅助树/运行态验收。** `MarketGrid.tsx:157-170` 无 `localeText/getLocaleText`，源码只有中文列标题不够证明 AG Grid 内置辅助说明中文。当前没有检查或运行可访问树，不断言具体运行时文字已被读出。 |
| sweep41 S41-N2 桌面行情行键盘选股 | **仍为强候选。** 桌面行只通过 `onRowClicked` 选股 (`MarketGrid.tsx:139-144,165`)，同时启用 `suppressCellFocus` (`:169`)，未发现 key handler 或行内链接/按钮替代；移动端按钮不能证明桌面可用。未运行浏览器，结论不升级为完整 WCAG 失败证明。 |

## 独立补漏与反证

- 独立复查重现了“启动/非详情标题实际是 `股票模拟游戏`、详情标题动态变成 `股票名 — 股票模拟游戏`”与 UX-CONTRACT `:95` 固定文档标题冲突：生产在 `useMobileUiController.ts:19-23` 写 title；初始 HTML 为 `web` (`index.html:7`)，StartupScreen 可在 GameWorkspace/controller 挂载前显示，故初始窗口也有另一标题范围。此项归既有 Q04，不新增重复编号或把未决契约擅自裁决。
- `currentTradingDayEvents` (`market-model.ts:572-576`) 当前只被测试和定义引用，生产 UI/Host 路径未消费；这补强 G11 的 caller 证据，避免以 helper 名称当已实现。
- 反证：`MobileStockDetail.tsx:237` 有公司财务内容入口，不能因 DESIGN/UX 信息 tab 示例没逐项列财务，就断言功能缺失；更多/五日周期和均线设置有明确禁用说明，不算当前遗漏。逐笔数据不能拿每笔共同当前时刻冒充 event time，也不应前端捏造时间。资金方向当前明确不提供，不应用产品草图强造栏位。
- 新的独立生产缺口方面，本次没有找到证据足以超过已有 N/G/C/Q 的确定范围；额外发现主要是既有 Q04 启动标题范围以及 G11 未调用 helper 的佐证，不虚增编号。

## 结论边界

当前 G10–14、G22–25、G30–34 都仍有源码级证据；sweep02 N01–N05 仍成立，C01–C03 需保持候选/契约冲突级别；sweep15 C15-01/02 与 sweep41 S41-N1/N2 按上述分类保留。G11 的“完全没有日界处理”旧表述不准确，应按 CivilUpdate 重建与非日期分钟槽的具体消费风险窄化。本记录没有运行短测、构建或浏览器，也不声称通过验证或穷尽任意运行时缺陷。
