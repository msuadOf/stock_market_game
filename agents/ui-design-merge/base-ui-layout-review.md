# base UI 合并复核

复核范围：`apps/web/src/App.css`、`apps/web/src/App.tsx`、`apps/web/src/app/LocalRefreshViews.tsx`、`apps/web/src/mobile/MobileStockDetail.css`、`apps/web/src/mobile/MobileStockDetail.tsx`、`apps/web/src/mobile/mobile-color-consumer-ssr.test.ts`。比较工作树合并结果与 `HEAD`（main）及 `origin/feat/ui-design`；遵循 `AGENTS.md` 与 `docs/principles.md`。

## 结论

- 指定范围没有发现 A 股概念、单位或交易规则语义漂移。变化均在 web 展示和历史行情读取层；涨跌颜色不改变大 A 涨红跌绿表达。
- main 的 `hostRef` 通过 `MarketRuntimeProvider` 共享、`FloatAllocationInput`、`InitialAllocationSummary` 和新游戏设置仍保留；`DeliveryModeControl`、暂停偏好与存档入口转移到“我的”面板后仍有对应入口。错误状态仍以 `setNotice` 显式展示，`queryChartHistory` 内部保留上下文错误提示。
- 旧 `mobile-color-contrast.test.ts` 中原有断言未被删减或放宽。SSR consumer 测试新增 `ChartSettingsFixture` 包装用于满足真实 `MarketKlinePanel` 的 chart settings context，且给 `MarketGrid` 提供真实 browser 依赖形状；改动范围适当。

## 发现

- 低优先级：`ConnectedChartPanel` 与 `ConnectedMobileDetail` 的 history-query effect 将 `klineDays` 纳入依赖，但请求只传 `code`，`queryChartHistory` 拉取完整日线历史且不读取天数。因此切换显示窗口也会重复查询同一只股票历史，并可能让在途请求被新的窗口选择请求取代。建议从两个 effect 依赖中移除 `klineDays`；如果该重查是为特定行为所需，应补充说明及边界测试。位置：`apps/web/src/app/LocalRefreshViews.tsx` 的两个 `queryChartHistory` effect。

除上述非阻断的重复读取外，未发现需阻止合并的缺陷。未运行额外测试；实现者反馈 SSR 短测在 10 秒 supervisor 下 1/1 通过，耗时 1.75 秒，本复核未独立重跑。
