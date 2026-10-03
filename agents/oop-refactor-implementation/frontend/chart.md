# frontend 图表与行情 Grid owner 实施记录

## 范围与依据

本簇实施 `web-01-A27`、`frontend-N02`、`frontend-N03`。已读取 `AGENTS.md`、
`docs/principles.md`、`docs/testing.md`、`docs/architecture.md`、`docs/open-questions.md`，
以及 ADR-0004、ADR-0007、ADR-0009、ADR-0010；待改源码均全文读到 EOF。

本次只迁移 Web 展示投影、第三方资源与异步提交记账，不修改交易制度。原分时/竞价坐标、
`Cents` 到元的 `/ 100`、成交量股数、量能显示手数、A 股红涨绿跌、权威日 K 来源和
收盘竞价排除行为保持原实现。未新增依赖、协议字段、存档字段或 engine authority。

## web-01-A27：MarketChartProjection

- 唯一拥有五组私有字段：`pricesByCode`、`auctionsByCode`、`continuousVolumes`、
  `completedDailyByCode`、`activeDailyByCode`。
- `upsertFrames` 集中跨批次分钟点、竞价点与累计量基线写入；`rebuildHistory` 清除旧代码后按输入顺序重建。
- `replaceSnapshot` / `replaceActiveCandles` / `installBaseline` / `reset` 集中完成与活动日 K 的转换、替换和读取。
- `pricePointsFor` / `auctionPointsFor` / `candlesFor` 返回新的呈现数组；分时、竞价点以及日 K 对象冻结，
  `tradeStats` 也冻结，selected 数组写入不能反向修改 projection。
- `history` / `activeCandles` 只浅拷贝并冻结 code 索引，不深复制全量历史。未变化证券数组和点复用；
  普通无 snapshot delta 不重建完成日 K，完成 candle 对象引用稳定。baseline、civil 与 runtimeSnapshot
  仍按原规则重建权威日 K；reset 只清分时/竞价/volume 历史并安装所给 snapshot 日 K。
- `useMarketChartRuntime` 每次挂载用惰性 `useState` 唯一构造 projection，保留 chartCode、三个 React 输出数组、
  Redux dispatch 与 `applyEffects` 次序。projection 没有订阅、计时器、I/O 或 dispose。
- Provider Actions Context 将可写 refs 改为稳定 `useCallback` getters：`getPriceHistory`、`getActiveDailyCandles`。
  getter 调用次数只影响只读浅索引副本，不增设缓存 authority；每次调用读取当前 projection。
- 真实 caller 同步迁移：`MarketRuntimeProvider`、`ConnectedMarketPanel`、`ConnectedMobileDetail`、真实
  `ProtocolCoordinator` 到 hook 的 SSR probe。App 的 reduction bridge 不变，没有新增 effect 消费 reduction。

定向测试覆盖跨批次 volume、同分钟替换、下降累计量差值非负、输入及嵌套对象值不变、独立实例确定性、
重建清旧代码、空历史、竞价 null 指示价与 ClosingAuction 过滤、baseline/reset、缺失 active 代码删除、
selected 与只读 getter 的反写拒绝。生产 hook probe 覆盖普通连续 delta、runtimeSnapshot、civil、select/reset。

## frontend-N02：PriceChartRuntime

- 唯一持有主图/副图、price/candle/volume/MACD/KDJ series 及 ResizeObserver disconnect 和 window resize listener。
- `create`、`updatePrice`、`updateIndicator`、`resize`、`dispose` 接管完整第三方资源操作。
- `PriceChart` 保留 DOM refs、indicator React state、`useIndicatorResults`、effect 调度和 JSX；每个挂载 effect
  的 cleanup 只 dispose 本次局部创建的 runtime，再清除相同实例的 ref。
- 注入窄 `PriceChartRuntimePorts` 用真实命令 mock 检验行为；生产默认使用原 createChart、observeChartContainers
  与 window。resizeEvents 契约仅包含实际使用的 resize 注册/移除和无参 callback。
- 保留主容器缺失时不创建、副容器缺失时只创建主图、创建错误抛出、空分时清空、日 K 权威输入窗口裁剪、
  切换模式清前模式、指标 pending/error 清旧数据、series 删除/创建与 fitContent 顺序。
- 不增设半初始化创建 rollback；原有创建中途失败资源边界留待独立行为修复。
- dispose 按原 observer disconnect → window unlisten → main remove → secondary remove 次序释放并清内部 series 字段。

定向测试覆盖创建与重新创建、两个容器独立 resize、cleanup 顺序、主/副容器缺失、createChart 抛错、
分时/日 K 切换与空值、量能/MACD/KDJ/none 切换、pending/error 清值、none 后重建 volume。

独立复核发现 FR01：最初提取的 MACD/KDJ 循环全删除后批量置 null 改变 removeSeries 第 2 项抛错后的部分更新。
已恢复原顺序逐项 remove 成功后立即清字段，补 mock 第二项失败后重试测试。修复前断言已释放句柄被删除 2 次
而非 1 次，修复后 MACD 与 KDJ 均保持第一项 1 次、失败第二项 2 次、第三项 1 次；未扩大创建 rollback。

## frontend-N03：MarketGridRowSynchronizer

- 唯一持有借用 API、latest rows 和 submitted rows；AG Grid 继续拥有实际 grid 与异步事务执行。
- `recordLatest` 只登记 render 时最新目标，使 ready 先于 effect 时仍读取最新行；`updateLatest` 由 effect 登记并提交。
- `attach` 使用新 grid 的 initial rowData 建立基准后提交最新目标；`dispose` 仅解除 API，不声称取消已排队事务。
- 真实 caller 为 `MarketGrid`：移除 `latestRowsRef`、`appliedRowsRef`、`gridApiRef` 与原 applyLatestRows；保留纯
  buildMarketRows identity cache、初始 rowData ref、React 移动 viewport/排序/JSX。
- cleanup 接入依赖实际支持的 `onGridPreDestroyed`；已核实当前 AG Grid 类型中的该事件与 applyTransactionAsync 签名。
- 空事务不提交；applyTransactionAsync 返回后前移 submitted target，throw 时不前移，不冒充异步完成 callback。
- `market-grid-rows.ts` 的 build/diff 纯 helper 与 `chart-resize.ts` 的 observer helper 保持原实现。

定向测试覆盖 attach 前多次目标只提交最新目标、render 登记不提交、连续异步目标差异、未变化证券引用、
空事务、集合缩小/清空、首次 ready/effect 不重复添加、dispose/new API 初始基准、API throw 后重试不丢差异。
原 local-refresh 源码接线守卫改为检查真实 owner attach/updateLatest 及 owner 内 applyTransactionAsync，仍检查稳定 row ID；
行为由 synchronizer 定向测试验证。

## 验证记录

- Node `v25.8.2`；先全文读取 `scripts/run-with-deadline.mjs`。
- API 无行为 stub 的红灯：projection 缺少分钟点/日 K、synchronizer attach 未提交、runtime 未创建 chart 均产生断言失败；
  移入原行为后绿。只读索引反写和 FR01 部分删除重试均额外经历断言红 → 绿。
- 一次从仓库根目录运行混合 Vite probe 的命令因 Vite root 错误失败，随后在 `apps/web` cwd 正确执行；
  该失败是运行环境路径错误，不报告为业务红灯。
- 精确八 suite 使用真实 Node 文件隔离与 `--test-concurrency=4`，case `--test-timeout=10000`，
  整进程树 `run-with-deadline.mjs 10000`。最后完整所属运行 8/8 文件通过，wall 1.10 秒；
  FR01 后再修 fixture listener 类型，两个所属 suite 使用 concurrency=2 通过，runner duration 0.25 秒。
- 精确 oxlint 使用 `--threads=4`，所有所属源码/测试首次通过；FR01 与 fixture 类型修改后所属 runtime 两文件再 lint 通过。
- 未运行全量回归、build、E2E 或本地 tsc；父 agent 统一 tsc 发现的 listener mock 4 处 implicit-any 已修复，等待父 agent 重验。
- 最终八 suite：market-chart-projection、market-chart-runtime、price-chart-runtime、chart-resize、
  price-chart-indicators、market-grid-row-synchronizer、market-grid-rows、local-refresh-boundaries。
- 完整独立复核及 FR01 修复复核由父 agent 统一安排；此记录不替代独立门禁结论。

## 完整修改文件清单

新增：

- `apps/web/src/app/market-chart-projection.ts`
- `apps/web/src/app/market-chart-projection.test.ts`
- `apps/web/src/components/price-chart-runtime.ts`
- `apps/web/src/components/price-chart-runtime.test.ts`
- `apps/web/src/components/market-grid-row-synchronizer.ts`
- `apps/web/src/components/market-grid-row-synchronizer.test.ts`

修改：

- `apps/web/src/app/useMarketChartRuntime.ts`
- `apps/web/src/app/MarketRuntimeProvider.tsx`
- `apps/web/src/app/LocalRefreshViews.tsx`
- `apps/web/src/app/market-chart-runtime.test-support.tsx`
- `apps/web/src/app/market-chart-runtime.test.ts`
- `apps/web/src/components/PriceChart.tsx`
- `apps/web/src/components/MarketGrid.tsx`
- `apps/web/src/components/local-refresh-boundaries.test.ts`

工作记录：本文件。无 Git 写操作、commit、push；未改 MobileStockDetail、useIndicatorResults、indicator-results 或其他 owner 文件。
