# 桌面分时图独立复核

- 日期：2026-10-05。
- 复核者：未实施变更的 independent_review subagent。
- 范围：延续完整桌面导航 diff，重点新增 DesktopIntradayChart.tsx、desktop-intraday.css、desktop-intraday.test.ts，PriceChart 与 ConnectedChartPanel 接线，price-chart-runtime resize/刻度宽度，桌面时钟及窄窗口 CSS。
- 本轮只读源码并执行短测试；没有启动构建或修改实现。

## 初轮发现（均已修复，见最终复核）

1. **[P2] 指标纵轴固定至少 1 导致小幅 MACD 被压扁。** DesktopIntradayChart 使用 `Math.max(1, ...values)`，实际 DIF/DEA/柱在 ±0.01～0.1 时仍被迫使用上限 1，大部分画布空置，曲线压在底部，偏离原图表自动量程行为。建议按实际有限值 min/max，MACD 包含零基线，仅全等值时扩展明确非零范围；补小数幅度、全零、全负边界。
2. **[P2] 昨收 caption 从近似坐标反算，破坏原始报价精度。** `(p.scale.top + p.scale.bottom) / 2` 再 `toFixed(2)` 用图表 Number 坐标推导“昨收”。ADR-0031 只允许末端坐标近似，原始报价必须精确；例如 `last_close=9007199254740993` 的一分无法由 Number 准确保留。建议原始 Cents 传入 caption 并使用 centsToYuanText，图表刻度仍可近似；百分比宜直接复用 scale 已有 topPercent/bottomPercent，而非反算中心。需补大金额 caption 测试。

## 语义与必要性

- 复用 MobileIntradayProjection 和 intradayChartX，未新增竞价撮合算法；符合 ADR-0009 已有独立竞价区、连续 240 分钟固定槽位与午休压缩约定。auctionSegments 保留 null 断点，连续价格只绘制已发生点，未来区间留白。
- 竞价累计量和分钟成交量分开标注、独立量程，成交量由既有股转手格式化。没有调用 projection.averageLine/displayedAverage，因此未伪造 VWAP 或成交均价。
- 分时 SVG 适配同花顺式固定时轴与左右报价/百分比轴，属于本轮视觉与操作目标必要变更。日 K 保留原 Lightweight Charts，engine、订单、存档与金额权威运算不变。
- 新桌面时钟复用既有 formatGameClock，不新建时间换算；依赖默认 A 股交易日时序，与移动时钟相同。短周期测试 fixture 并不因此成为真实时钟正确性的证据。
- PriceChart 的隐藏旧图表节点保留实例；resize 仅在正宽高时更新尺寸并 fitContent，主/副图同时处理。minimumWidth=64 是最小刻度栏宽度，不能单凭源码宣称所有大数字量程下两图实际轴宽必然严格相等，最终仍需浏览器核对。

## 独立验证

执行：

```sh
node scripts/run-with-deadline.mjs 10000 -- node --test --test-timeout=10000 --test-concurrency=2 apps/web/src/components/desktop-intraday.test.ts apps/web/src/components/price-chart-runtime.test.ts apps/web/src/mobile/mobile-intraday-projection.test.ts
```

结果：16/16 通过，退出码 0，Node 报告约 325ms。覆盖空图、竞价缺价断点、固定槽位、图表创建/释放、指标切换、正高度 resize、隐藏保留尺寸、主副图 resize 后 fitContent，以及共享投影既有边界。当前新增桌面测试未覆盖 ready MACD/KDJ 渲染和大金额 caption，因此不能由这些通过结果否定上述发现。

最终 E2E、1020px 实测布局、分时/日 K 往返、macOS 生产制品部署仍由主 agent 继续。整个游戏此前既有回归与日终存档限制依然需独立报告。

## 最终 delta 复核

- 两项发现均已修复并通过独立复核，当前没有新增阻断性 finding。
- MACD 上限改为实际数值与 0 的最大值，下限使用实际数值与 0 的最小值；全零时固定在 50% 中线，避免除零。新增渲染断言覆盖 ±0.01 小幅、全零及全负值，确认小幅数据实际铺开，而非仅检查代码结构。
- 原始昨收自 ConnectedChartPanel 的 `market.last_close` 经过 PriceChart 的 Cents prop 传入 DesktopIntradayChart，并由 centsToYuanText 输出；仅进入原图表 runtime 时才转换近似坐标。大金额 `9007199254740993` 的 caption 确认输出 `90071992547409.93`，右侧百分比改为使用 scale.topPercent，不再反推中心。
- 独立重复上述相同短测命令，18/18 通过，退出码 0，约 301ms；10000ms 进程树 deadline、10000ms case timeout、concurrency=2 不变。`git diff --check` 通过。
- 主 agent 报告此前 4 项桌面 E2E 通过，包含 1020px 无页面横向溢出、价量边界同宽；本 reviewer 未重复浏览器操作，不能将报告扩大为所有视口/量程均通过。
- 用户最新要求热更新，主 agent 已将 3000 端口切为 Vite 开发服务。该选择取代本轮继续 Rust 打包的交付动作，不应继续把当前热更新地址称为新的 Rust 生产制品；本 reviewer 未重启服务或编译 Rust。

## WebKit 测速 timer 接线追加复核

- 已检查 useSpeedMetricsPolling 的单行 delta：把直接注入的 setTimeout/clearTimeout 改为调用 window.setTimeout/window.clearTimeout 的闭包。原 ports.schedule/cancel 以 ports 为 receiver 调用浏览器原生方法，闭包显式保留 Window receiver，解决主 agent 在实际 WebKit 报告的 Illegal invocation；轮询间隔、请求令牌、错误展示与清理顺序没有改变。
- 修复局限于 UI 适配层，没有改变实际倍率计算、tick、交易或存档语义，属于恢复顶栏真实测速所需的最小变更。useEffect 仅在浏览器执行，未将 window 访问移到模块或 SSR render 阶段。
- 独立执行 10000ms 进程树 deadline、10000ms case timeout、concurrency=2 的 speed-metrics-polling.test.ts：3/3 通过，约 142ms；git diff --check 通过。现有单测验证 polling 行为，不模拟 WebKit 原生 receiver 检查；浏览器 Illegal invocation 消失及 HMR 生效由主 agent 的实际检查提供证据。
- 此追加 delta 无新增 finding。

## 用户指定高低价贴边的最终复核

- 用户最新明确要求桌面分时上下界贴合截至当前的最高最低价，覆盖此前桌面昨收对称轴约定；DESIGN/UX 已注明桌面独立纵轴，移动投影与固定全天横轴未变。
- 初轮仅使用可见分钟点，无法包含同一分钟内已发生但未被末值保留的成交极值，已指出该缺口。最终接线从同一 MarketRuntime 的 `getActiveDailyCandles()[chartCode]` 经 PriceChart.dayRange 传入 DesktopIntradayChart；当日权威 high/low 与可见竞价/连续点共同决定域。
- 只有 active candle 的 volume 已定义且大于 0 才计入其高低价，排除零成交时的昨收占位；没有通过 OHLC 猜测极值时间或制造新曲线点。竞价指示价并集用于容纳真实绘图数据，未冒称这些指示价都属于已成交日高/日低。
- 已检查 runtime 的 replaceActiveCandles：按当前权威映射替换 activeDailyByCode，移除缺失证券，不在图表组件累积历史极值。新组件每次 render 重新计算域，因此当前实现没有自行保留上一证券/昨日极值的缓存。
- 空态使用昨收单刻度，唯一价格居中；非退化范围无价格 padding，昨收仅在域内显示参考线。精确昨收 caption 仍走 Cents 格式化。engine、订单、资金和存档未变。
- 独立运行 desktop-intraday.test.ts 与 price-chart-runtime.test.ts：15/15 通过，退出码 0，约 292ms，10000ms 进程树 deadline/case timeout、concurrency=2。覆盖权威 13/9 极值、零成交占位排除、无旧极值残留、单价不除零、单边行情不强制包含昨收，以及既有指标与 resize 边界。git diff --check 通过。
- 本 delta 独立复核通过，无未解决 finding。浏览器仍应核对贴边端点文字及线条不被裁切；这不要求改变用户指定价域，也不代表全局游戏回归通过。

## 竞价缺价区域说明追加复核

- 仅为 visibleAuctionPoints 中显式 value=null 的已发生槽位增加斜纹，不为不存在的未来槽绘标识，不将首个有效指示价倒填至 09:15。null 仍不进入 price 域或价格曲线，既有 auctionSegments 断线语义保持。
- 每个标记以既有 intradayChartX 槽中心计算半槽宽，左右裁剪到竞价区 0～16；不改共享横轴或实际事件时间。useId 为各实例生成独立 pattern 引用；CSS 使用现有 muted token 与低透明度，caption 和 title 明确“暂无竞价指示价”，并说明不是零价。
- 属于解释用户看到的空白的必要展示改动，不修改无交叉委托时的指示价语义，不添加价格或成交量，不将缺价标为数据传输失败。无新 finding。
- 独立重复 desktop-intraday.test.ts 与 price-chart-runtime.test.ts：16/16 通过，退出码 0，约 282ms，10000ms 进程树/case deadline、concurrency=2。新增断言验证两个 null 标识、一个真实有效价格点且没有倒填连线；git diff --check 通过。
- 未独立检查浏览器实际斜纹对比度及各缩放尺寸下的视觉效果；这部分由主 agent 的真实界面检查补足。

## 用户改为 0% 参考线与更新粗点的审查

- 用户明确撤销斜纹方案，要求无报价沿 0% 参考轴显示并同步手机版。共享 auctionDisplayPoints 只新建显示对象，原始 null 不被修改；首次有效指示或有效指示价/可匹配量变化才 updated，null 不冒充成交更新。连续点仅在本分钟 volume>0 时加粗点，title 明确“本分钟有成交”，没有冒称逐笔时间。
- 桌面域纳入上述显示参考值属于容纳用户明确要求的参考线，不改变真实当日 high/low 或 engine 数据；手机使用相同 helper，移动价格对称域仍保持。
- 独立短测：desktop-intraday、intraday-auction-display、price-chart-runtime 合计 17/17 通过，约 293ms。
- 补跑移动真实 render：第一次在仓库根目录执行因测试 Vite root 依赖 cwd 而全部 setup 失败，不是产品缺陷；随后在 apps/web 目录正确执行，9/10 通过，G47 旧“3 段不跨 null”断言失败（实际 1，预期 3）。这是用户新要求引起的测试契约变化，已要求实现者更新为参考轴及更新点断言，不能删除真实点/量能验证。
- 初读 DESIGN.md 旧正文仍要求 null 不画线，与末尾新规范矛盾，已要求同步旧正文。以上测试/文档同步完成前，此 delta 不记为最终通过；暂未发现新增实现语义缺陷。

### 0% 参考线最终复核

- 两项契约同步问题均已解决。DESIGN.md 原第 88 行正文明确显示层 null→昨收参考轴、原始 null 保持、参考线不作为真实报价/成交，与新规范一致。
- 移动实际 render 测试保留并重写为用户最新要求：1 条线、6 个坐标、4 个有效更新粗点，两个 null 槽的 y=50，原始 null 不变且全部 6 个量槽仍存在。不是删去关键验证来消除失败。
- 独立在 apps/web 工作目录重跑 desktop-intraday、intraday-auction-display、mobile-component-render：20/20 通过，退出码 0，约 410ms；10000ms 进程树/case timeout、concurrency=2。git diff --check 通过。读取文件时首次用了相对根目录路径导致 sed 找不到文件，随后从仓库根目录重新读取并核实，不影响测试运行结果。
- 主 agent 报告已在 390×844 浏览器确认移动零轴与粗点生效；本 reviewer 未重复实际浏览器操作。最终源码及契约独立复核通过，无未解决 finding。

### 连续竞价取消粗点追加复核

- 按用户新要求，仅保留集合竞价 updated 粗点；DesktopIntradayChart 与 MobileStockDetail 的连续竞价 volume>0 circle 均已移除。桌面单个连续点也统一存入 polyline 坐标，不额外绘圆点。价格、量能、时间轴与原始行情没有变化。
- 测试保留权威极值单点的 y=50 几何断言，只把检查目标由 circle 改为相同 polyline 坐标；两端新增连续成交仍无粗点断言。DESIGN/UX 已注明粗点仅用于竞价指示更新。
- 独立在 apps/web 运行桌面图表、共享 helper、移动 render 短测：22/22 通过，退出码 0，约 435ms，10000ms 进程树/case timeout、concurrency=2；git diff --check 通过。本小 delta 独立复核通过，无新增 finding。

### 图表品牌标志移位复核

- 两处 createChart 的 layout 均设置官方 attributionLogo:false，覆盖价格主图和指标副图，没有用覆盖层挡图或修改库源码；行情、指标和坐标逻辑不变。
- 已核对本机安装库 typings 的 attributionLogo 定义与 README 第 117～119 行：用户可访问页面须提供 NOTICE 署名与 TradingView 链接，已有链接时可关闭图中标志。当前 UserPanel 在数据管理后以原生 details/summary 提供“关于图表”、署名和官方链接，桌面游戏管理及移动“我的”均复用该组件。署名文本与主 agent 从官方 v5.2.0 NOTICE 读取的记录一致；本 reviewer 未再次联网读取 NOTICE。
- 独立运行 price-chart-runtime.test.ts：8/8 通过，退出码 0，约 112ms；新增断言确认主副图两个 option 均为 false。10000ms 进程树/case timeout、concurrency=2，git diff --check 通过。
- 此小 delta 范围必要、无交易语义变化、无新 finding。实际浏览器中两个图标消失及折叠署名可访问性仍由主 agent 检查。

### 日 K 均线多选复核

- 用户明确 20/60/120/240/360 日按钮是均线开关，当前改动将 movingAverageDays 与原 klineDays 显示窗口分离，按独立 aria-pressed 开关和固定颜色显示。切换均线没有调用 updatePrice 或 fitContent；侧栏切页保持 ConnectedChartPanel 状态，分时移除均线 series，返回日 K 按保留选项重建。
- klineMovingAverage 按权威日 K 收盘价计算完整 N 根 SMA，先使用全历史滚动累加，再只输出可见窗口时间点；不足 N 根不补值。数值仅用于图表显示，不写回权威价格、资金或订单。不同周期独立 series，不会相互覆盖；取消与 dispose 清理句柄。
- 独立运行 price-chart-runtime.test.ts：9/9 通过，退出码 0，约 101ms，10000ms 进程树/case timeout、concurrency=2；git diff --check 通过。新增测试覆盖 20/60 同时存在、首完整均值、单独关闭、历史不足空线、分时移除、K 线原数据及 fit 次数保持。
- 代码审查确认裁剪使用计算前全历史；建议后续增加 visibleDays 小于 days 的代表性断言，防止未来重构先裁剪后计算，但当前未发现该缺陷。E2E 多选按钮、颜色及浏览器曲线验证由主 agent 继续。
- 本 delta 语义符合用户明确要求，范围必要，无阻断性 finding。UX-CONTRACT 已明确按钮为多选均线、不改变显示范围。

### 均线与指标配色追加复核

- MA20/60/120/240/360 的共享颜色定义依次为紫、橙、蓝、青、绿；按钮和 runtime series 均读取 KLINE_MOVING_AVERAGES，未出现图例/曲线颜色漂移。均线选中态保留 aria-pressed、字重与底部色条，不只依赖色相；指标按钮增加 aria-pressed，使用中性底色与蓝色 accent。桌面周期样式限定 layout-desktop，未修改移动行情涨跌颜色。
- 此 delta 仅调整显示常量、按钮 class/CSS 与参考文档，没有修改 SMA 算法、蜡烛窗口、原始数据、交易制度或资金单位。符合用户现场参考要求，范围必要。参考截图由主 agent 核实，本 reviewer 未重新操作两个参考 app。
- local-amount-render 删除不再存在的 setKlineDays props，盘口标题断言改为当前的价格元/数量手标题，仍保留 5 手与 2.5 手以及卖盘 rank 验证，不属于弱化领域断言。
- 独立在 apps/web 运行 price-chart-runtime 与 local-amount-render：16/16 通过，退出码 0，约 421ms；整命令进程树 deadline 和 case timeout 均为 10000ms、concurrency=2。git diff --check 通过。主 agent 报告 5 项桌面 E2E 已过，本 reviewer 未重复执行。
- 本配色 delta 无新增阻断性 finding；此结论不代表此前完整回归中的既有失败已解决。

### 仅参考股票主力模拟器配色的复核

- 源码检查确认本次仅更换颜色：SMA、MACD、KDJ 计算及正负判断、成交量股转手、权威输入没有变化。MA20/60 对应粉紫/蓝，其余周期的色板映射已在 DESIGN 中注明，未冒称参考软件原有配置。颜色测试继续断言原有量值与时间槽。
- 独立运行 price-chart-runtime、price-chart-indicators、volume-histogram：13/13 通过，退出码 0，约 192ms；10000ms 进程树 deadline/case timeout、concurrency=2。git diff --check 通过。
- 有效发现 P2：浅色 --down=#5caf61 同时作用于盘口/涨跌文字，白底对比度 2.71:1、浅蓝选中底 #a8ccfa 上 1.63:1。应将柔和图形色与可读文字色分离，或加深行情文字 token，避免密集小字难以辨认。
- 有效发现 P2：新增 DIF/K/MA120 深灰 #505050 在暗色 card-bg=#182231 上对比度 1.99:1；新 accent=#27669c 未被暗色覆盖，与暗色选中底 #3b3020 的对比度 2.13:1。需为新增深灰图线及选中按钮补暗色适配。以上比值按 sRGB 相对亮度计算；本 reviewer 未重复浏览器视觉验证。
- 两项已消息通知主 agent。当前领域语义通过，配色可读性修复后需再次复核，尚不记为最终完成。

### 仅保留均线参考配色的最终复核

- 用户收窄为仅均线参考后，独立确认 index.css、price-chart-indicators.ts、volume-histogram.ts 及 volume-histogram.test.ts 的 git diff 均为空；runtime 的蜡烛/涨跌线/MACD/KDJ 颜色恢复原值，桌面分时指标恢复此前颜色，指标按钮恢复红色选中，桌面 chart-tab 蓝色覆盖已移除。DESIGN 明确其余色彩不跟随均线参考。
- 前次浅绿文字及暗色 accent/深灰指标回归已随撤回解决。保留 MA120 改 #909090，在白底对比度 3.19:1、暗色 #182231 上 5.01:1，满足本次灰色曲线两种图表底色的辨识要求；其余 MA 仍共享按钮与 series 颜色来源，计算未变化。
- 独立短测 price-chart-runtime、price-chart-indicators、volume-histogram：13/13 通过，退出码 0，约 192ms；10000ms 进程树 deadline/case timeout、concurrency=2。git diff --check 通过。
- 本次撤回与均线配色 delta 复核通过，前两项 finding 关闭；无新增领域语义或范围问题。不将本短测结果描述为完整回归通过。

### 顶部 MA 数值图例复核

- 最新实现提供 MA5/10/20/30/60，默认全选；图例与 series 继续共用周期/颜色。数值从完整权威日 K 收盘价计算最新 SMA，三位小数仅显示精度，不改变价格单位或资金；不足周期使用“—”，不补值。此范围符合用户最新截图覆盖要求。
- chart-toolbar 独占 grid 首行 auto，price-chart 位于第二行 minmax(320px,1fr)，图例 flex-wrap 可参与首行高度；静态未见图例绝对定位遮挡图表。旧 kline-period-bar grid-row:3 已因嵌套在非 grid toolbar 下而失效，可删除；真实窄屏几何由主 agent 验证。
- 发现待修复：UX-CONTRACT 第108行仍写下方20/60/120/240/360，与当前顶部5/10/20/30/60冲突。另按钮 aria-label 仅 MA 名称覆盖了可见数值，读屏不能获取新核心图例数值，应包含值/不足周期说明或使用可见文字作为 accessible name。已通知主 agent。
- 独立运行 runtime 与 local-amount-render：17/17 通过，退出码0，约1215ms；10000ms进程树 deadline/case timeout、concurrency=2。待上述同步后再次复核。

### 顶部 MA 数值图例最终复核

- 独立确认 UX-CONTRACT 第108行已改为顶部 MA5/10/20/30/60、默认全显、最新 SMA 三位数值和不足周期说明，与实现一致。
- aria-label 现包含 MA 周期及三位数值/“历史不足”，读屏可以获得核心图例数值；E2E 使用相应名称前缀匹配，保留五按钮、默认开启、独立关闭和切换分时后的状态验证。失效 grid-row:3 已删除。
- git diff --check 通过。该修复限文档、accessible name 和失效 CSS 清理，本 reviewer 不重复此前17项短测；主 agent 报告本轮5项桌面 E2E（workers=3）通过，最终重跑由主 agent 跟进。
- 前两项 finding 已关闭，本批最终独立复核通过，无新增领域语义或必要性问题；不代表完整回归通过。

### 桌面/移动共用 K 线与盘口的初次复核

- MarketKlinePanel 与 FiveLevelBook 真实供两端调用；盘口仍以100股换手，保留真实rank及空档，不虚构报价。SSR测试只换共享DOM定位，关键5/2.5手和卖盘rank断言保留。SMA先全历史计算后按窗口裁剪，不足完整周期不补值，分时继续独立保留桌面用户要求的极值纵轴。
- 发现 P2：桌面条件挂载 MarketKlinePanel/PriceChart 会在日K↔分时卸载实例，重置MA开关、viewport及分时指标。需要稳定挂载或提升状态，并覆盖关闭MA20后往返仍关闭；既有E2E仅查始终开启的MA60，无法防此回归。
- 发现 P2：MobileKlineProjection价格域仅包含可见蜡烛high/low，新MA60可能受窗口前历史影响超出域；例如下跌行情缩到30根时MA60可高于全部可见蜡烛，图线出SVG被裁。应将已选均线可见值纳入价格域并补短fixture。
- 发现 P2：共享msd-kline-meta仍单行flex且gap13px；五个带三位数值按钮在手机/窄桌面有横向溢出风险。旧toolbar图例wrap选择器对新共享组件不适用，需要共享换行及明确按钮样式，并实测320/390及1020桌面。
- 三项已通知主 agent。独立运行MarketKlinePanel、local-amount-render、mobile-component-render：19/19通过，退出码0，约617ms，10000ms进程树deadline/case timeout、concurrency=2。上述状态/极值/布局边界不由当前SSR覆盖，修复后需再次复核。

### 共享图表修复与 MACD 增量复核

- 桌面日K/分时已改稳定挂载并由hidden父容器切换；E2E补MA20关闭后往返仍false。overlayPrices把已选MA可见值纳入MobileKlineProjection价格域；图例共享样式增加wrap、明确button间距，缩放工具正常文档流。前次三项实现finding已修复，浏览器几何由主agent验证。
- 独立运行共享组件、mobile-kline-projection、local-amount-render及mobile-component-render：25/25通过，退出码0，约502ms；10000ms进程树deadline/case timeout、concurrency=3。
- 新MacdPanel静态逻辑正确：全历史close送入Rust，DIF/DEA/hist按同visibleWindow切片，与蜡烛同slot，价格域包含0和全部三组值，全零居中且小幅不以1压缩。无撮合、金额单位或权威行情变化；保留原桌面指标功能有必要。
- 测试门禁遗漏：现MarketKlinePanel测试只覆盖默认KDJ的SSR，新MACD分支尚无直接验证。已要求补ready的小幅/全零等几何、窗口同槽及error/unavailable短测试后再次复核。此时暂不宣称最终完成。

### 共享图表最终测试复核

- 新MacdPanel直接SSR测试已覆盖35条历史裁剪为30根histogram、共用slot横坐标、全零y=36、小幅0.001撑满4..68且无NaN/Infinity；error/unavailable明确alert且不绘SVG。测试直接执行新增MACD renderer，不是只检查按钮。
- 独立重跑共享组件、mobile-kline-projection、local-amount-render及mobile-component-render：27/27通过，退出码0，约537ms；10000ms进程树deadline/case timeout、concurrency=3。git diff --check通过。
- 主agent报告5项桌面E2E通过、1020桌面截图正常，320/390手机图例clientWidth与scrollWidth分别同为309/379，确认换行无溢出；本reviewer未重复浏览器操作。
- 本批三项实现finding与新增MACD测试遗漏均关闭，最终独立复核通过；未发现新的A股语义、单位、跨层数据或不必要范围问题。此前完整回归既有失败不在此结论中声明解决。

### 共用 K 线手势增量复核

- 两端使用相同useKlineGestures与viewport reducer；滚轮阈值/Shift平移/双指阈值仅改变窗口，不改行情、SMA或股手单位。点按按固定capacity映射真实槽，空白区不制造蜡烛；selectedTime在窗口变化后按同一time查找，跨图竖线使用同slot中心。
- 原生监听只挂于共享section，wheel与touchstart/move明确passive:false；只有SVG滚轮和双指路径preventDefault，单指位移>8px清除tap且不preventDefault，保留页面滚动。touchend/cancel清理手势状态并抑制600ms合成click；effect清理使用与注册相同handler，六个监听均移除。callbacks ref读取最新viewport/数据，未见闭包过期或重复注册。
- 独立短测gestures、MarketKlinePanel、mobile-kline-projection：12/12通过，退出码0，约287ms；10000ms进程树deadline/case timeout、concurrency=3；git diff --check通过。新增E2E源码验证窗口档位、平移offset、三图同x和双指路径，但运行结果由主agent继续取得。
- 未发现阻断性实现问题。建议后续以事件defaultPrevented断言锁定单指不拦截/双指拦截，补touchcancel与合成click抑制边界；当前静态路径正确。合成TouchEvent验收不能等同真实设备浏览器滚动/缩放手感验证，报告需区分。

### K 线详情与跟随手势初次复核

- 选中时间映射展示索引，顶部MA/量/KDJ/MACD取同一选中位置；鼠标mousemove仅following时更新，触屏选中后拖动preventDefault、未选中仍保留滚动；关闭清除selectedTime。新增mousemove与其余监听均有cleanup。蜡烛实体rect及相关SVG图形补non-scaling-stroke，未改OHLC坐标或实体涨跌逻辑。
- 发现P2：KlineDetails将用于图表的number直接toFixed(2)展示OHLC、number相减显示涨跌额，没有使用已有rawPrices精确Cents。高位金额会产生显示失真，需读取原始OHLC及前收；图形坐标近似不能扩散到数值详情。
- 发现P2：小窗固定近白背景rgba(248,249,251,.94)却用主题text，暗色主题为近白字，详情不可读。需主题背景或相配文字颜色。
- 语义同步项：周/月K的OHLC/量是5/20游戏交易日聚合，当前单列“交易日”却显示group首日，需注明周期及起始日/范围，避免把多日量价当单日。已通知主agent。
- 独立短测gestures、MarketKlinePanel、mobile-kline-projection：13/13通过，退出码0，约286ms；10000ms进程树deadline/case timeout、concurrency=3，git diff --check通过。待修复精确值/主题/周期标识后再次复核。

### 详情精确值第一次修复复核

- 主题card-bg/text配对、周/月起始日标识均已修复；日K通过rawPrices与previousRawClose使用精确格式化/差额。
- 精确值finding尚未完全关闭：aggregateCandles的周/月聚合仍只返回number OHLC/volume，丢弃输入rawPrices，因此实际周/月详情仍回退number.toFixed。新增直接给KlineDetails传rawPrices的周K测试绕过真实聚合链，不能证明端到端精确。
- 已要求在原始价格完整时聚合rawPrices（首open、末close、精确比较high/low），并补真实aggregateCandles→KlineDetails高位Cents测试。尚不宣称完成。

### 详情精确值与透传最终复核

- aggregateCandles在整组rawPrices完整时保留首open/末close，并以compareMoney选择原始high/low；实际周/月聚合链能向详情及previousRawClose传递精确值。新增10条日K→2根周K→详情测试验证高位收盘9007199254740993.02及差额+0.01，未再绕过聚合链。
- 详情card-bg/text主题配对及周/月起始日标识保持正确。小窗pointer-events:none、关闭按钮auto，鼠标跟随可透过小窗到下方SVG，关闭按钮仍可操作。真实跟随/关闭交互最终E2E由主agent完成。
- 独立短测gestures、MarketKlinePanel、mobile-kline-projection：15/15通过，退出码0，约294ms；10000ms进程树deadline/case timeout、concurrency=3；git diff --check通过。
- 本轮精确值、主题与周期标识finding全部关闭，最终源码及短测复核通过。未发现新的交易语义或不必要范围变化，不宣称全部回归已通过。

### 点击/点按切换对齐线复核

- 初始selectedTime=null不绘对齐线；click在following时调用dismiss，否则选择真实槽；touchend仅未移动点按且following时dismiss，移动超过8px的已选中拖动继续选择、不关闭。moved一旦置true不因拖回起点复位，符合拖动与点按区分。双指路径不执行toggle，合成click继续受600ms抑制。
- MarketKlinePanel显式传入清selectedTime回调；无效/空白槽不伪造数据。新增逻辑集中于共享hook与接线、E2E，未改行情、精确金额、MA或股手单位；范围必要，未发现阻断性实现问题。
- 独立短测gestures、MarketKlinePanel、mobile-kline-projection：15/15通过，退出码0，约294ms；10000ms进程树deadline/case timeout、concurrency=3；git diff --check通过。E2E源码新增初始无对齐线、二次click和二次touch关闭断言，实际运行由主agent跟进。
- 建议非阻断补充已选中单指移动>8px并touchend后仍显示且x已变化的断言，以明确锁定本轮“拖动不关闭”。本delta源码独立复核通过，不将未取得结果的E2E或全回归宣称通过。

### HEAD 至完整工作区提交前复核

- 延续各批独立审查，复核HEAD至工作区完整文件范围（包括暂存与未暂存）及E2E迁移、启动/测速小修、共享组件、文档证据。未发现未解决的阻断性实现问题；没有engine/server/Cargo/依赖修改。现有E2E对资金、存档、报告的核心断言保留，调整为新导航入口而非删除验证。
- 本轮onDismiss已必填，touch拖动后仍显示与合成click忽略断言已补；主agent报告7项桌面E2E通过，本reviewer未重复全量验收。各轮独立短测证据见上文，不能将此结论替代全回归通过。
- 提交准备事项已通知主agent：MM/AM文件须重新stage，确保包含最终详情、精确聚合与toggle修复；webui-service.pid属本机临时进程文件，不建议提交；local-deployment末尾当前静态服务描述应补Vite热更新现状。
- git diff HEAD --check发现两处EOF空白（webui-headers.txt第10行、FiveLevelBook.tsx第18行），需清理后重stage。此前普通git diff --check只覆盖未暂存变更，不能用来声称完整暂存补丁无空白问题。
- 整批代码独立复核结论：可提交；先完成上述暂存与证据文件整理。所有实现finding均已修复并复核，未发现新的A股语义漂移或无关实现范围。
