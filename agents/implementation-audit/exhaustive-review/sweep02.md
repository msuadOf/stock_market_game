# Sweep02：移动设计与交互全文复核

## 基线与覆盖

- 目标源码：`b76ece3`。实际只读检查时 worktree HEAD 为 `4ad5a2e`；`git diff b76ece3 HEAD --` 本文涉及的 `apps/web/src/mobile/`、App、样式、UI controller、chart projection/runtime、LocalRefreshViews、engine CivilUpdate 路径没有产品差异，因此以下生产行号适用于目标基线。
- 已读根 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`、`docs/architecture.md`；相关 ADR-0009、0010、0014、0023、0025用于解释现行契约。交易规则只依据仓库已有官方核对记录理解，不声称重新联网核验。
- 连续全文阅读至 EOF：`DESIGN.md` **129 行**、`UX-CONTRACT.md` **96 行**、`design/ui/mobile/qa/README.md` **5 行**，共 **230 行**。下面逐章覆盖，不能将标题/关键词搜索当成全文覆盖。
- 已对照主台账、`reaudit-ui.md` 与 `coverage/r17.md`。本文件仅新增审计记录；未改产品、未运行测试/浏览器/完整验收、未写 Git 状态。
- 分类：已有生产链只核销对应窄项；仍缺不等于已运行复现；未来、契约冲突与验证证据分别记载。独立审查由主控统一安排。

## 逐章覆盖

| 文档章节、原文行 | 生产调用与实际消费 | 当前状态 |
|---|---|---|
| DESIGN front matter 1–36 | `MobileStockDetail.css:1` 定义移动端颜色、字体及容器，`index.css:3` 定义全局变量；组件在 `MobileStockDetail.tsx:202` 消费样式 | token 主干存在；规范颜色与 AA 的冲突见 C01，不把设计 token 全部存在冒充验收 |
| Overview / Creative North Star / Product context 40–56 | `App.tsx:524` 桌面多面板，`:667` 手机详情；`MobileStockDetail.tsx:108` 图/盘口四象限；中文界面与 engine 数据相连 | 主结构存在；页面语言属性漏实现 N05 |
| Colors 58–60 | `MobileStockDetail.css:22` rise/fall，`:85` 分时/均价线，`:115` 盘口深度色块；`MobileStockDetail.tsx:219` 报价实际消费涨跌 | 红涨绿跌基本存在；分时量方向与空心形态 G12，文字对比 C01 |
| Typography 62–64 | `MobileStockDetail.css:14` 字体与 tabular-nums、`:15` 容器字号；`:199` 后置 px 覆盖关键行情 | 部分，G33；不误称整页均不响应 |
| Layout 66–68 | `MobileStockDetail.tsx:218` 报价→`:223` 周期→`:231` 图表→`:234` 信息；CSS `:73` 同列四象限 | 结构存在；列宽与视觉还需浏览器检查 |
| Elevation & Depth 70–72 | `MobileStockDetail.css:73` 平面分隔；`App.css:515` 底页阴影 | 主干存在，未做像素验收 |
| Shapes 74–76 | `MobileStockDetail.css:67` 选中下划线、`:218` 圆形工具按钮；`App.css:521` 底页圆角 | 主干存在，未做像素验收 |
| Components / Foundational visual states 78–82 | `MobileStockDetail.tsx:226` 禁用未来周期，`:97` Rust 指标 pending/error；CSS `:25` 焦点、`:21` 禁用 | 部分，G30；选中导航仅 CSS 的辅助技术状态见 C02 |
| Navigation and data display 84–96：顶栏/44px/时钟/倍速 | App→`LocalRefreshViews.tsx:62` / `:187`→`MobileGameClock.tsx:11`→`market-model.ts:596`；`MobileRunToggle`、`MobileSpeedSelect` 共用；头部 CSS `:177` | UI接线存在，G25；宿主倍率失败及远程路径仍由 G01–05/G19负责 |
| 同章：周期/信息独立、分时真实时间、竞价量槽、未来留白、对称轴 | `useMarketChartRuntime.ts:51`→`MarketChartProjection.upsertFrames`→`MobileIntradayProjection`→`MobileStockDetail.tsx:127`；reducer `mobile-ui-state.ts:73` / `:75` 独立 | 部分，G10–14/G32；新增零量正柱 N01、竞价 null 槽被连线 N04 |
| 同章：常驻导航/统一叠层/列表迷你图 | `App.tsx:672` 常驻导航、`:684` 遮罩；`index.css:26` 叠层；`MarketGrid.tsx:193`→`sparklineGeometry`→`:206` SVG | 主干存在；返回原列表 N02、标签状态 C02；240槽与昨收渐变有真实生产调用 |
| Scroll ownership 98–100 | `App.css:709` 详情滚动，`:783` 冻结但保持列表；`:515` 底页内部滚动、`:562` 遮罩 touch-action:none | 主要指针/触摸链存在；未运行键盘滚动/辅助技术隔离验证，不声称完整 AA |
| Iconography and motion 102–104 | `App.tsx:678` 图标+文字；`useMobileUiController.ts:25` 底页焦点、Escape；`App.css:524` 300ms | 部分，G34 reduced-motion作用范围不足 |
| Content and data visualization 106–112：文本替代、行情手/委托股、共享大数、盘口真实深度 | `MobileStockDetail.tsx:59` 实际盘口，`:166` activeDailyCandle累计；`utils/format.ts:118` 股→手；`LocalRefreshViews.tsx:135` 持仓股；`App.tsx:566` 委托股 | 单位/资金统计主干存在；桌面卖档错标 N03；字段错误 G22；大数来源仅展示不回写engine |
| 同章 114：分时午休共轴 | `market-model.ts:192` 固定阶段横轴；`MobileStockDetail.tsx:130` 网格、`:138` 午休复合标签，`:153` 量柱共用坐标 | 主要算法存在；G32垂直中轴；不把尾盘独立曲线列为现行漏实现 |
| 同章 116–120：K槽/MA/KDJ/真实蜡烛/窗口边界 | `MobileStockDetail.tsx:71` KlinePanel→`MobileKlineProjection`→`:94` 蜡烛、`:96` 量、`:97` KDJ；`KlineViewportControls.tsx:20` 各按钮消费 viewport边界 | 窗口和红空心绿实心K柱存在；G30；K量零量正柱 N01；Q07/Q08口径仍开放 |
| 同章 122：360日历史真源 | engine candles→Snapshot→`kline-sync.ts`→`MarketChartProjection.replaceSnapshot:74`→`KlinePanel`，Web未生成历史 | UI消费已有；生成与恢复深度校验由engine分组复核，ADR-0023禁止恢复真实行情校准待办 |
| Do's and Don'ts 124–129 | 同列图量/盘口/逐笔生产结构如上；排除资金方向伪造由 `MobileStockDetail.tsx:178` 明示不提供 | 部分/验收债；G13/G14/G33；不新开已明确排除栏位 |
| UX Product context 3–10 | 简体中文UI、交易时钟与240分钟映射；根 HTML 页面语言由 `apps/web/index.html:2` 决定 | N05；AA颜色冲突 C01，其它行业语义本组不改规则 |
| Business-context sources 12–19 | 规范引用 ADR/architecture/设计来源 | 引用清单，非新增可执行功能 |
| Visual contract 21–26 | App `data-theme`→`index.css:91`；移动详情CSS固定浅色 | 主干存在；静态颜色对比 C01；暗亮整体仍需浏览器验收 |
| Canonical UI Map 28–36 | `App.tsx:553` 字段、`:688` notice；原生选择器 `MobileSpeedSelect.tsx:14`；全局叠层token | 主干存在；G22与G30；不得要求原生select跨平台弹层几何完全相同 |
| Navigation and responsive behavior 38–60：全部23条 | 按上述对应生产链逐条核对；远程控件在 `LocalRefreshViews.tsx:154` 按 capability条件消费；周期/信息独立、盘口与资金累计来自权威值、K窗口和日内缓存如上 | G10–14/G22–25/G30–34；新增N01–05；Q04标题/Q07周月/Q08展示派生保持开放；快捷交易规则 C03 |
| Flow ledger 62–71：六个操作 | `App.tsx:202` 开详情、`:669` 返回/周期/信息；`useMobileUiController.ts:25` 交易焦点/关闭、`:71` 信息滚动；远程刷新handler及host | 返回原主列表 N02；G23详情焦点、G24滚动；底页确实有焦点还原，不误称全部焦点能力缺失 |
| Async and resilience 73–82：完整8条 | protocol reducer/normalize→`useMarketChartRuntime.ts:51`；CivilUpdate安装完整状态，tick消费帧；App持久化按ADR-0025；成交100条见store | 分时分钟/日界G10/G11；host与持久化完整性交相应分组，不用UI文字宣称宿主已完整 |
| 局部刷新边界 84–89：四条 | `MarketRuntimeProvider.tsx:79` 数据context；`LocalRefreshViews.tsx:23` portfolio比较；`MarketGrid.tsx:47` rowSynchronizer→AG Grid事务 | 订阅主干存在；G31未变图表数组换引用；详情额外订阅全部股票keys/玩家账户/成交slice为进一步性能检查边界，不凭此断言实测性能回归 |
| Verification 91–96 | 已有web短测/e2e/构建入口；实际浏览器和视觉矩阵不在本次运行范围 | 证据债，未声称验收通过 |
| QA README 1–5 | 视觉数据从engine Snapshot及formatter生产路径消费；固定390CSS px同档、动态区排除是截图验证契约 | 规则已有；N01/N04属于绘图事实错误，不是要求复制截图数据；没有执行截图或mask对比 |

## 既有 G/Q 当前复核

| ID | 当前源码证据与变化 |
|---|---|
| G10 | `market-chart-projection.ts:14` 逐帧累计差、`:37` 按minute覆盖；`market-model.ts:568` 同槽替换。仍缺。同分钟100→200→200最后为0；首连续点previousVolume未承接竞价累计。 |
| G11 | **仍缺，但旧文字需收窄**。`useMarketChartRuntime.ts:53` 的 CivilUpdate确实调用rebuildHistory，并非完全没有日界处理；`protocol/validate.ts:164` 要求AfterClose携带旧日全量帧，`protocol/normalize.ts:55` 原样进入投影。`CivilBoundary:19` 的相邻交易日为同一AfterClose+BeforeOpen屏障，因此`:54`重建旧日后，下个交易日仍按槽合并，新日未覆盖槽仍旧。仅纯非AfterClose屏障携带空intraday、或初始化/读档reset清空。不能用休市日空屏障核销相邻交易日错误。 |
| G12 | `market-chart-projection.ts:15` 连续方向恒true、`:24` 竞价以非null判方向；`MobileStockDetail.css:125` / `:126` 量柱实心。仍缺。 |
| G13 | `store.ts:119` 最新优先100条，`LocalRefreshViews.tsx:178` 按股过滤，`market-model.ts:655` slice(-7).reverse。仍缺。 |
| G14 | `market-model.ts:663` 统一当前tradeTime，`MobileStockDetail.tsx:159` 每笔共用。仍缺；TradeEvent未带时间消费，必须在权威帧/成交关联链补对应时刻，不能造随机时间。 |
| G22 | `App.tsx:565` / `:566` 价格/股数输入缺字段级错误关联，`useTradingCommands`显式notice。仍缺，非静默吞错。 |
| G23 | `App.tsx:202` 开详情只派发状态、`:669` 返回只派发back；UI controller焦点管理仅交易底页。仍缺，与N02原列表身份错误不同。 |
| G24 | `useMobileUiController.ts:73` requestAnimationFrame scrollIntoView。仍缺。 |
| G25 | CSS `:186` 返回首列30–35px、`:194` 切股22–26px，高度44不替代宽度。仍缺；未测浏览器热区。 |
| G30 | CSS `:218` `var(--msd-focus)` 未定义，覆盖通用焦点outline；仍缺。无效变量导致computed-value无效，不能靠SSR名称认定焦点可见。 |
| G31 | runtime `:72` / `:74` 每批setState；projection `:94` / `:99` accessor总建新数组→Provider数据context变。仍缺，未测幅度。 |
| G32 | CSS `:80` SVG框底部排除23px轴；`:93` 百分比0%按整容器50%。仍缺，静态差11.5px；尚未浏览器测量。 |
| G33 | CSS `:199` / `:207` 固定关键px覆盖早期cqw，局部其它组件仍响应。仍缺。 |
| G34 | CSS `:173` reduce仅详情子树，交易底页在App独立节点、App.css `:524` transform 300ms。仍缺。 |
| Q04 | UI controller `:20` 详情股票title仍与UX `:60` 固定标题冲突；`index.html:7` 初始web。当前StartupScreen还在GameWorkspace挂载前，故初始title不只可能出现在JS尚未运行窗口；见App `:726`启动选择与 `:185` controller位置。并入旧Q04，不新增产品编号。 |
| Q07 | `market-model.ts:338` / `:344` 5/20交易日按数组分组，未按公历周月；仍待口径裁决，不假称自然周月已有。 |
| Q08 | `market-model.ts:654` / `:680` 分时价格点算术均价、`:726` MA前端派生；KDJ来自Rust结果。仍待展示派生与权威指标边界裁决，不称全部指标伪造或全部来自Rust。 |

## 主台账未覆盖的明确生产缺口

### N01：零成交量被绘制为非零量柱

- 原文：`DESIGN.md:88` 要求真实累计量、不伪造额外成交量；`:110` 行情量真实；QA `:3` 量/K线不得为截图补造数据。ADR-0023决策4明确零成交占位不能生成成交量。
- 生产：`MobileIntradayProjection.volumeMarks` (`market-model.ts:693`) 对真实0量执行 `Math.max(1, ratio*100)`，以1%高度经 `MobileStockDetail.tsx:153` 画出；`MobileKlineProjection.volumeMarks` (`market-model.ts:772`) 对真实0量执行 `Math.max(1, ratio*66)`，经 `MobileStockDetail.tsx:96` 画1px柱。
- 影响：无交叉委托的竞价、连续零成交分钟、日K昨收零量占位均可出现正量柱。可保留0量时间槽，但正高度应仅适用于真实非零量；保留槽位不等于可画虚假正量。
- 现有测试 `mobile-intraday-projection.test.ts:50`、`mobile-kline-projection.test.ts:24` 已冻结该正高度，不能以这些测试存在核销真源要求；本轮未运行。修复应增加生产组件对零/非零量的区分验证，不删除零量边界。
- 与G10独立：即使一分钟量累计修正确，真实零值仍画正柱。

### N02：从“自选”进入详情后返回“行情”，未返回原列表身份

- 原文：UX `:44` “回到原列表”；Flow ledger `:67` 成功目的地“原行情列表”；`:40` 行情、自选是分别列出的主导航。
- 生产：`mobile-ui-state.ts:71` 的open-detail无条件 `primaryTab:"market"`，`:83` 的back只清detailCode；`App.tsx:202` 股票点击进入此路径，`:669` back直接派发，`:674` 底栏高亮取primaryTab。
- 触发：主导航选择自选→点股票→详情返回。即使MarketGrid保持挂载和内部筛选/滚动，当前主页面身份已经从watchlist改成market，返回后底栏与标题仍显示行情。
- 需要保存来源主页面或等价状态；与G23焦点遗漏、G24主动滚动不同。旧R17“返回原列表已实现”窄项不能继续核销本边界。

### N03：桌面盘口把最优卖价标为“卖5”

- 原文：DESIGN `:110`、UX `:48` 要求真实五档盘口；DESIGN `:68` 保留桌面工作台不取消A股档位语义。
- engine：`orderbook.rs:646` / `:651` ask_depth按价格低→高，最优卖价在asks[0]；`session/snapshot.rs:171` 将其作为MarketSnap.asks。
- 消费：`ConnectedChartPanel` (`LocalRefreshViews.tsx:113`) 直接按 `market.asks.slice(0,5)` 顺序map，却把label写成 `卖${5-index}`。最优价因此叫卖5，第五档叫卖1；只有一档时仍叫卖5。
- 移动路径 `buildFiveLevelBook` (`market-model.ts:371`) 正确按rank-1取数组，与桌面发生跨层显示漂移。无需改变撮合，只应让显示顺序/档号一致并保持空档真实。
- 代表性验证：一档、两档、五档，各label应对应权威价格rank；当前不声称已执行。

### N04：竞价无指示价的时间槽仍被价格线跨越

- 原文：DESIGN `:88` “竞价事件没有可成交指示价时不绘制或伪造价格线”；UX `:50` 无交叉委托为空态、不以前端估值替代。
- 生产：`market-model.ts:645` 先filter所有value=null点；`:671` 再把剩余有效点join为一个polyline；`MobileStockDetail.tsx:133` 单条polyline实际绘制。
- 示例：有效价(time0)→null(time1)→有效价(time2)会生成0至2的连线，在time1画出引擎未提供的价格。整段null时无价格线已正确实现，遗漏是间断null槽。
- 应按连续有效片段绘制，真实0量/null槽仍保留；与Q08算术均价是否允许、G11跨日残留、尾盘图未来范围均独立。

### N05：全中文页面声明为英语

- 原文：UX `:8` active locale zh-CN、`:10` WCAG2.2AA；DESIGN `:50` 简体中文。
- 生产：`apps/web/index.html:2` 固定 `<html lang="en">`。已查UI controller/Main/App无documentElement.lang改写；中文正文在该页面下挂载。
- 影响：辅助技术默认以英语页面语言朗读中文，页面语言声明不符合现行界面。应对应实际简体中文locale。与Q04标题策略不同，标题是否动态不决定lang。
- 代表性验证：启动页及游戏页实际documentElement.lang与中文locale一致；本轮静态确认，未运行浏览器。

## 新候选与契约冲突

### C01：规范颜色与WCAG AA文字对比冲突

- UX `:10` 承诺WCAG2.2AA；DESIGN `:5`至`:16` 同时约定精确色值。CSS `:3` / `:4`、`:22` / `:23` 将#EF3F49/#009B22实际用于白底盘口小字号文字 (`MobileStockDetail.tsx:65`, CSS `:207` 11px)；金色均价文字也出现。
- 按sRGB相对亮度公式静态计算，白底对比：红3.834:1、绿3.677:1、金2.329:1、蓝3.871:1；11px普通文字的AA要求4.5:1。未做浏览器computed-style或辅助工具验收，这些不是实测截图结果。
- 这不仅是缺一项测试：直接照精确token渲染也不能同时满足文字AA。建议将文字颜色与图形/品牌色作用边界明确后登记；不擅自改设计色或弱化AA目标。

### C02：主导航和行情分类未向辅助技术表达选中状态

- UX `:10`、`:95`要求辅助功能；主导航 `App.tsx:674` 只有active class，无aria-current/aria-pressed等；行情分类 `MarketGrid.tsx:181` 也只active class。图表/信息标签则已使用aria-selected，不能说全部tab语义缺失。
- 当前可访问名称保留且按钮可操作；需要核对是否应以导航current或切换pressed表达状态。作为完整AA语义候选，未用SSR存在就宣称屏幕阅读器已通过。

### C03：交易快捷涨跌停价读取静态默认证券规则

- 相关跨层契约：`docs/trading-rules.md:3` 参数为权威会话配置，前端只预检查；DESIGN `:108`要求诚实文本报价。可编辑存档范围还须由主控与config分组复核。
- `App.tsx:212` 正式submit传activeSetup；`LocalRefreshViews.tsx:129` 快捷价却固定找DEFAULT_SETUP.stocks，未接activeSetup或protocol securities。读入同code不同category时，快捷按钮可能仍算默认涨跌停；动态code无默认定义时只显示缺规则。
- 表单证券选项也固定STOCK_LIST (`App.tsx:555`)，行情却已发现动态code (`market-model.ts:258`)。是否登记现行代码缺口，需与严格存档允许的证券配置编辑范围一起确认；不能仅因支持行情显示动态代码就发明证券增发需求。

## 未来、已实现与证据债核销

- 五日分钟线、更多周期、均线配置按钮明确disabled/title尚未开放 (`MobileStockDetail.tsx:93`, `:225`, `:229`)；看点/资讯/社区/简况明确占位 (`:238`)；不重复升级为必做功能。
- 财务信息tab是已实现公司内容入口，六项旧清单少写财务属于文档范围漂移，不直接判代码漏实现。
- 收盘竞价事件在projection `:20` 显式过滤，ADR-0014规定尾盘独立可视区未来；不得把ClosingAuction画到开盘区域或要求当前必画尾盘。
- 当日量/额/笔数实际读activeDailyCandle，不从100条trade cache反向累加；合成前史无tradeStats不伪造本局统计。金额手/股语义无需重开。
- 交易底页确实有Escape关闭、首字段聚焦、焦点回触发器；详情导航焦点另缺G23。指针/触摸遮罩存在，完整浏览器键盘/辅助技术行为仍缺验收证据。
- 不恢复真实历史导入/校准、日内持久保存或旧兼容档路线；分别遵循ADR-0023/0025。
- 320/390/430px、固定390同档截图与结构mask、减少动画偏好、键盘焦点、三宿主真实倍率与刷新模式全链仍需要代表性验收，本报告未运行这些验证。

本分组新增明确生产缺口5项；3项候选/契约冲突；既有14项G仍有缺口，其中G11证据表述收窄；Q04/Q07/Q08保持未决。全文覆盖限于上述230行及实际追踪链，不声称整个仓库已数学意义穷尽。
