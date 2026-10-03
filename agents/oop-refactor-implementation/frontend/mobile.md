# Mobile 与原型 OOP 实施记录

## 范围与依据

- 动作：`frontend-N08`、`frontend-N09`、`web-08-A01`，包括完整扩展 `frontend-E03`。
- 已阅读：`AGENTS.md`、`docs/principles.md`、`docs/testing.md`、`docs/architecture.md`、`docs/open-questions.md`，相关 ADR-0009、ADR-0014、ADR-0023，分配动作全文及待改源码至 EOF。
- 本轮仅聚合 Web 呈现投影与静态原型的现有行为，没有改变交易受理制度。交易时段依据沿用 ADR-0009、ADR-0014 与 `docs/trading-rules.md` 登记的沪深现行规则，核对日期 2026-10-03；算术均价、5/20 游戏交易日聚合及原型动画均继续按游戏现状解释，不冒充真实成交均价或日历周/月。
- projection class 放在 `market-model.ts`，直接复用该模块现有 primitives，避免拆出 class 再重新导出造成循环依赖。未新增依赖。

## frontend-N08

- owner：`MobileIntradayProjection` 为 `IntradayPanel` 每次 render 建立的只读值；借用输入行情，按既有 slice/filter 建立可见点。没有跨 tick 可写 history。
- 迁移方法：`fromInputs` 聚合可见连续点、竞价点、有效竞价价格点、昨收对称 scale、phase volume scale、progress、displayedAverage、signature、最近七笔倒序 trades，以及统一明细时刻与时钟文字；`priceY/auctionLine/continuousLine/averageLine/volumeMarks` 共享同一个实例的数据。
- caller：`MobileStockDetail -> IntradayPanel` 已用同一 projection 输出三条线、单点竞价圆点的纵坐标、成交量细线与 metadata；原 inline slice、scale、y、线、phase height、signature 全部移除。FiveLevelBook、React JSX 与 ARIA 仍归组件。
- 领域边界：MarketSnap 价格分转元；PricePoint 与 AuctionPoint 的 value 已为元；竞价 volume 累计值与连续 volume 分钟增量分别缩放；null 竞价价格仍保留量。平均值照搬价格点算术均值；最近七笔成交仍以 elapsedMinutes-1 的统一分钟展示。ClosingAuction 过滤继续由上游负责。
- 精确测试：`mobile-intraday-projection.test.ts` 四项组合用例，覆盖空、单竞价价、null 价格大累计量、独立 phase scale、午休坐标、均价、signature、最近七笔、slice 边界、显式异常、输入未变。

## frontend-N09

- owner：`MobileKlineProjection` 聚合本次 normalized window、visible candles、价格域、MA5/10/20、Rust KDJ 切片、成交量及 latestSignature。KlinePanel 继续持有 React viewport 与 useIndicatorResults；allCandles 输入仅借用。
- 迁移方法：`fromInputs`、`priceY`、`slotFor`、`candleBodyAndWick`、`movingAverage`、`movingAverageLine`、`indicatorLine`、`volumeMarks`；`visibleWindow/latestSignature` 只读观察。
- caller：`MobileStockDetail -> KlinePanel` 的实体、上下影线、三根 MA、volume 与 KDJ 都读取同一 projection；移除原 window/slice/price domain/y/slotFor/MA/indicatorLine/volume-height/signature 闭包。既有 viewport reducer、code-period key reset 和指标计算宿主请求顺序保留。
- 领域边界：MA 先计算全历史再截取可见 window；Rust 是 KDJ 真源，不新建 MACD/KDJ 算法。aggregateCandles 按 5/20 游戏交易日分组；volume 事实仍是股，组件以手展示。实体仍只连 open/close，影线在实体边界截断。
- 精确测试：`mobile-kline-projection.test.ts` 四项组合用例，覆盖空、单根、容量大于历史、offset clamp、MA 窗口前历史、KDJ pending/error/unavailable/J 极值、共享 slots、影线、5/20 聚合及输入未变。

## web-08-A01 与 frontend-E03

- owner：原型页面唯一 `MobileTradingConceptController` 持有 root、panels、chart/nav buttons、progress/clip/dot/output，以及扩展所要求的 detail/watch/title refs。DOM hidden/class/value 仍为唯一视图事实。
- 迁移方法：`bindEvents/selectChartView/navigateTo/renderProgress`；纯 `conceptProgress` 保留 x=3.6*v、round(570+2.4*v)、42/60/80 分段和 HH:MM 图稿动画映射。
- caller：原 HTML 第一个交互 script 当场构造并 bindEvents，仍使用 onclick/oninput 赋值与立即初绘；后续 DOM 拼装脚本顺序保留。book 分支仍调用 chart button.click，保留标题；重复 data-p=watch 按钮仍仅选中实际按钮；未知 chart panel 返回现状保留。
- 精确测试：`mobile-trading-concept.test.ts` 执行真实 HTML 中交互 script 与后续两段节点搬移 script，使用只实现所需 DOM 操作的隔离 fixture。覆盖 time/day/book、未知 panel、watch/detail/title、重复 watch 身份、book 程序化 click、初始化、端点、三个分段、搬移后的原节点 refs。不是浏览器验收；硬编码原型样例不作为 A 股 domain 断言。

## 红绿、运行与限制

- N08/N09 首跑确认两种 class 缺失的 owner 断言失败，随后实现、迁 caller，8 项组合用例通过。原型先运行旧 script：3 项行为 baseline 通过，controller owner 断言失败；迁移后四项全部通过。
- 初次 Node 默认 process isolation 只报告文件失败，改用仓库惯例 `--test-isolation=none` 后取得真正的 owner 缺失红灯；其中一次 prototype test 使用 Node strip-only 不支持的 parameter property，修正 fixture 后得到预期红灯。一次并行短测中 supervisor 相对路径错误（尚未启动测试），修正后重跑通过；这些不记为产品行为失败。
- 最终以两个真实 Node 进程并行运行：
  - SSR 与 viewport shard：`node ../../scripts/run-with-deadline.mjs 10000 -- node --test --test-isolation=none --test-timeout=10000 --test-concurrency=4 src/mobile/mobile-component-render.test.ts src/mobile/kline-viewport-controls.test.ts`，7/7 通过；Node test duration 811.79ms，命令 wall 1.02s。
  - 模型与原型 shard：同样 deadline/case timeout/concurrency 参数，执行 `mobile-trading-concept.test.ts`、`mobile-intraday-projection.test.ts`、`mobile-kline-projection.test.ts`、`market-model.test.ts`，43/43 通过；Node test duration 121.78ms，命令 wall 0.29s。
- cwd 为 `apps/web`，Node v25.8.2；并行方式为外层两个独立进程，内部 concurrency=4。SSR 新增真实 caller 的 null 竞价量/阶段高度/算术均价，以及 candle slots/open-close 实体/影线断言。
- 未运行完整回归、build、E2E 或 tsc；未操作 Git 写入、提交或推送。上述短测不代替这些门禁。
- 独立复核由父 agent 安排未实施本次改动的 reviewer 执行，尚待发现处理与复核结论，不能将实现加短测直接宣称批次完成。
