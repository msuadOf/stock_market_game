# Web UI checkpoint 独立复核

日期：2026-10-06。复核者未参与本批实现。本记录只覆盖 `HEAD` 到当前 worktree 中 `apps/web/src` 的范围；排除 `host/`、`save/`、`types/`，不复核另行分配的 `host/save/types` 接缝。审阅了该范围完整 staged 与 unstaged diff、变更文件当前源码及有关 UI／host 连接；未修改代码、未运行测试、未运行 Cargo／index 操作。由于仓库含并行作者工作，本记录是明确边界的 checkpoint，不代表整批改动或产品功能已完成。

## 独立结论

当前未保留有效 finding。先前针对 `timelineGeneration` 状态隔离的 finding 已由 `CompanyPanel` 更新及相应迟到成功／失败测试覆盖；先前关于股票名称 `title` 属性的 finding 不适用，需求指可见标题而非 HTML `title` 属性。

## 已确认的范围点

- Simple seed 初始化 helper 仅以 `StockSpec.initial_price`、`total_shares`、seed 与 settlement cycle 初始化虚拟账面基准；后续无读取运行期成交价的反锚定路径。启动预览 draft 的 seed 被新局生命周期消费；远程 context 会用实际 setup／seed 替换预览值。既有 `seed-preview-runtime-review.md` 提供了这部分另一名 reviewer 的局部记录，但不能替代本次全 UI checkpoint。
- 公司财务材料标出 `SimpleGenerated` 来源；新 UI 没有为 Simple 隐藏共同报表／披露入口。已知无法查得的报告提供明确 unavailable reason。按期间查询仍有上述 timeline generation 状态缺口。
- 私有 `PersonalTradeHistory` 在无 account 时由调用层不渲染查询面板，并由 `queryPrivateHistory` 在发出请求前拒绝 null account；响应还检查宿主、generation、account。交割单组件调用前也先检查 account，账户不存在不会触发查询。账户 ID 通过 membership selector 获取，不再假定固定账户 `0`。登录 credential store 是单独的 auth 模块，不与市场账户 selector 合并。
- K 线周期选择覆盖自然周／月／季／年和 1／5／15／30／60／120 分钟；自然 candle 聚合读取日线，minute K 线走 retained/current-minute history。均线默认 MA5/10/20/30/60，可编辑并本机持久化。指标源路由显式按宿主 capability 选择，能力缺失时显示 unsupported，不回退到另一来源。
- A 股涨红跌绿样式变量明确为红色 `--msd-rise`、绿色 `--msd-fall`，并被 K 线、量柱及报价样式复用。可见股票名称与代码在桌面／移动详情均显示；“股票名称 title”按可见标题需求理解。
- 领域改动为初始化展示与图表查询，不更改撮合、交易日历或证券交易制度。金额单位继续使用分／元转换、成交量标手／股；未发现把展示字段当作 A 股规则的改动。

## 变更清单

以下列出本次审阅的 `apps/web/src` 全部 HEAD→worktree tracked 变更及 untracked 文件。路径只表示审阅清单，不表示所有文件均属于同一功能或已整体验收。

```text
apps/web/src/App.css
apps/web/src/App.tsx
apps/web/src/app/LocalRefreshViews.tsx
apps/web/src/app/MarketRuntimeProvider.tsx
apps/web/src/app/QuickTradingPanel.tsx
apps/web/src/app/RemoteLoginScreen.tsx
apps/web/src/app/StartupScreen.tsx
apps/web/src/app/app-startup-wiring.test.ts
apps/web/src/app/command-host-test-fixture.ts
apps/web/src/app/company-config-commands.test.ts
apps/web/src/app/local-amount-render.test.ts
apps/web/src/app/market-chart-projection.test.ts
apps/web/src/app/market-chart-projection.ts
apps/web/src/app/money-wire-ui.test.ts
apps/web/src/app/portfolio-selector.test.ts
apps/web/src/app/portfolio-selector.ts
apps/web/src/app/private-history-query.test.ts
apps/web/src/app/private-history-query.ts
apps/web/src/app/quick-trading.test.ts
apps/web/src/app/quick-trading.ts
apps/web/src/app/remote-login-behavior.test.ts
apps/web/src/app/remote-login-screen.test.ts
apps/web/src/app/remote-logout.test.ts
apps/web/src/app/remote-logout.ts
apps/web/src/app/save-commands.test.ts
apps/web/src/app/session-control-commands.test.ts
apps/web/src/app/session-control-commands.ts
apps/web/src/app/session-host-lifecycle.test.ts
apps/web/src/app/startup-recovery.test.ts
apps/web/src/app/trading-commands.test.ts
apps/web/src/app/usePausePreferences.ts
apps/web/src/app/useSaveCommands.ts
apps/web/src/app/useSessionHostLifecycle.ts
apps/web/src/app/useTradingCommands.ts
apps/web/src/auth/credential-store.test.ts
apps/web/src/auth/credential-store.ts
apps/web/src/components/ArchiveManager.tsx
apps/web/src/components/ChartDisplayMenu.tsx
apps/web/src/components/ChartPeriodTabs.tsx
apps/web/src/components/DesktopIntradayChart.tsx
apps/web/src/components/KlinePeriodSelector.test.ts
apps/web/src/components/KlinePeriodSelector.tsx
apps/web/src/components/MarketKlinePanel.test.ts
apps/web/src/components/MarketKlinePanel.tsx
apps/web/src/components/MinuteKlinePanel.css
apps/web/src/components/MinuteKlinePanel.tsx
apps/web/src/components/MovingAverageSettings.tsx
apps/web/src/components/PersonalTradeHistoryPanel.tsx
apps/web/src/components/PriceChart.tsx
apps/web/src/components/ReportFrequencyInput.tsx
apps/web/src/components/RetainedHistoryPanel.css
apps/web/src/components/RetainedHistoryPanel.tsx
apps/web/src/components/TradeConfirmationTable.tsx
apps/web/src/components/company/CompanyPanel.tsx
apps/web/src/components/company/CompanySystemInput.tsx
apps/web/src/components/company/ReportCorrectionPanel.tsx
apps/web/src/components/company/ReportNotes.tsx
apps/web/src/components/company/company-report-selection.test.ts
apps/web/src/components/company/company-system-config.test.ts
apps/web/src/components/company/company.css
apps/web/src/components/company/public-financials-render.test.ts
apps/web/src/components/company/public-report-fixture.ts
apps/web/src/components/company/report-availability.test.ts
apps/web/src/components/company/report-availability.ts
apps/web/src/components/company/report-correction-panel.test.ts
apps/web/src/components/desktop-intraday.test.ts
apps/web/src/components/indicator-source-policy.test.ts
apps/web/src/components/indicator-source-policy.ts
apps/web/src/components/kline-moving-averages.ts
apps/web/src/components/kline-periods.ts
apps/web/src/components/minute-kline-model.test.ts
apps/web/src/components/minute-kline-panel.test.ts
apps/web/src/components/minute-kline.ts
apps/web/src/components/moving-average-settings-ui.test.ts
apps/web/src/components/moving-average.test.ts
apps/web/src/components/moving-average.ts
apps/web/src/components/personal-trade-history-panel.test.ts
apps/web/src/components/player-orders.test.ts
apps/web/src/components/player-orders.ts
apps/web/src/components/price-chart-runtime.test.ts
apps/web/src/components/price-chart-runtime.ts
apps/web/src/components/report-frequency-input.test.ts
apps/web/src/components/retained-history-model.test.ts
apps/web/src/components/retained-history-model.ts
apps/web/src/components/retained-history-panel.test.ts
apps/web/src/components/trade-confirmation-table.test.ts
apps/web/src/components/useMovingAverageSettings.ts
apps/web/src/config/candle-aggregation.test.ts
apps/web/src/config/candle-aggregation.ts
apps/web/src/config/company-initial-preset.test.ts
apps/web/src/config/company-initial-preset.ts
apps/web/src/config/defaults.ts
apps/web/src/config/moving-average-settings.test.ts
apps/web/src/config/moving-average-settings.ts
apps/web/src/config/seed-draft.test.ts
apps/web/src/config/seed-draft.ts
apps/web/src/dev/NpcDecisionInspector.tsx
apps/web/src/dev/npc-decision-inspector.test.ts
apps/web/src/mobile/MobileRunToggle.tsx
apps/web/src/mobile/MobileSpeedSelect.tsx
apps/web/src/mobile/MobileStockDetail.tsx
apps/web/src/mobile/calendar-candles.test.ts
apps/web/src/mobile/calendar-candles.ts
apps/web/src/mobile/market-model.test.ts
apps/web/src/mobile/market-model.ts
apps/web/src/mobile/mobile-color-consumer-ssr.test.ts
apps/web/src/mobile/mobile-component-render.test.ts
apps/web/src/mobile/mobile-intraday-projection.test.ts
apps/web/src/mobile/mobile-kline-projection.test.ts
apps/web/src/mobile/mobile-ui-state.ts
apps/web/src/store/chart-settings-slice.ts
apps/web/src/store/chart-settings.test.ts
apps/web/src/store/indicator-source.test.ts
apps/web/src/store/portfolio-selector.test.ts
apps/web/src/store/remote-membership.test.ts
apps/web/src/store/remote-membership.ts
apps/web/src/store/store.ts
apps/web/src/utils/format.test.ts
apps/web/src/utils/format.ts
apps/web/src/utils/turnover.ts
```

## 审查依据与边界

已阅读 `docs/principles.md`、`docs/architecture.md`、`docs/open-questions.md`、ADR-0035、ADR-0036，并对照仓库既有 UI 设计风格。图表时间区间是展示取样聚合，不改变证券交易规则；公司初始化明确为虚拟账面模型，不声称真实财务统计。未复核被排除目录的 host／save／types 完整 diff，也未确认全项目构建、TypeScript 检查、全部测试或完整功能矩阵通过。此记录只作为限定范围 checkpoint，不表示整体功能完成。
