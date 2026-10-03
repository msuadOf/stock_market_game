# 批次 015 独立复核

复核基线：`.worktree/implementation-reaudit` 的 `HEAD=43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。按 scan-plan 的 output_root 写入本文件与配套 JSON。三篇指定来源均连续读至 EOF：`final-review.md` 48 行、`frontend/report.md` 600 行、`frontend/review-final.md` 34 行。未改产品源码、未运行测试/构建或执行 Git 写操作。

## 源材料完整性

- `agents/oop-refactor-audit/completeness-2026-10-03/final-review.md`：48/48 行，SHA-256 `f29248c54bf68fbef9b4c0b6f6ed4dcfd1a813b557d2bc4866392deef1bfb074`。阅读全文覆盖审查范围、三门、修订复核、结构/版本核验与限制。
- `agents/oop-refactor-audit/completeness-2026-10-03/frontend/report.md`：600/600 行，SHA-256 `d7879d0f45106a71ed96c2a253f6f774f291d66de6f4deda99ab3a96eaf74673`。阅读全文覆盖总论、N01–N09、E01–E03、反向发现、证据纠正、验证及执行偏差。
- `agents/oop-refactor-audit/completeness-2026-10-03/frontend/review-final.md`：34/34 行，SHA-256 `2a58b3674fefb3ad024a2597c6840d727951cc19028febb3906a866625e7dbb4`。阅读全文覆盖增量纠正、三门结论、指纹及验证边界。

## 基线对照

截至基线 `43b1aa5`，frontend 报告中 9 个新增候选及 3 个扩展均已有对应生产 owner/caller。报告的领域语义边界与独立行为风险仍有参考价值，但“未实施候选”状态已过时，不能据此重复登记 OOP 实施工作。

- N01 `ReportQueryContext` 由 `remote-host.ts:47` 持有；分页登记、ID/company 归属和时间线校验由 `remote-host.ts:274-289` 调用。定义在 `report-query-context.ts:4-41`。
- N02 `PriceChartRuntime` 由 `PriceChart.tsx:47,60-75` 创建、更新、清理；`price-chart-runtime.ts:23-226` 管理图表资源。半初始化失败资源回滚仍是另一个行为边界。
- N03 `MarketGridRowSynchronizer` 由 `MarketGrid.tsx:47-58` 持有并通过 ready/effect 接线，Grid UI 入口见 `MarketGrid.tsx:157-165`；定义在 `market-grid-row-synchronizer.ts:7-50`。
- N04/N05 `WasmSessionSlot` 与 `WasmTickLoop` 在 `wasm-worker.ts:93,143-316` 被实际调用，定义分别在 `wasm-session-slot.ts:18`、`wasm-tick-loop.ts:24`。session 与 timer/publish 生命周期仍分开。
- N06 `WorkerRequestScope` 由 WorkerHost 持有并用于实际命令；请求序号、generation 过滤和逐请求资源清理见 `worker-request.ts:26-73`。postMessage 同步抛错仍依 timeout 清理，dispose/fatal 取消请求仍是独立行为缺口。
- N07 `RemoteCommandRegistry` 由 `remote-host.ts:48` 持有；提交、queued/gateway 响应和 fail 路径见 `remote-host.ts:55-62,209-216`。`CommandQueued` 仍只代表入队，不代表受理或成交。
- N08/N09 `MobileIntradayProjection`、`MobileKlineProjection` 定义于 `market-model.ts:625,699`，真实消费在 `MobileStockDetail.tsx:81-105`。投影存在不表示 G10–G14 已解决；集合竞价量/连续分钟增量、算术均价、周/月按游戏交易日聚合等边界仍须保持。
- E01/E02 hooks 已由 `App.tsx:310-317,324-326` 使用，定义见 `usePausePreferences.ts:55`、`useSpeedMetricsPolling.ts:50`。Redux 偏好 authority、共享 request gate 与 AppShell 显示状态等边界仍成立。
- E03 是静态 HTML 原型 controller 字段补充；本次未发现需另立生产 owner 的依据。

## 主账交叉核对

`agents/implementation-audit/implementation-audit-2026-10-02.md` 仍将 G10–G14、G18、G31、G64 等列为现行缺口。它们属于量能投影、消费端背压、引用隔离及键盘入口等独立契约，不因 N02/N03/N05/N08/N09 对象存在而自动核销。例如 G13 对应的当前 `market-model.ts:655` 仍为 `slice(-7).reverse()`；WasmTickLoop 存在也不等于有消费端背压。

**有效发现：** frontend report 的“候选设计，未实施”及“9 new + 3 extension 是待实施工作”结论与基线源码矛盾。应将 12 项重分类为已有实现对照，保留真实 caller/owner；尚未履行的行为契约按 G/Q 或独立行为修复登记，不重复计算 OOP 工作。review-final 的覆盖集合/指纹核对和“不重新查官方规则”限定仍有效，但未核对这些候选在指定基线的实现状态。

**大 A 语义：** 静态核对未发现候选 owner 改变撮合、委托、资金、股份、T+1 或证券类别语义的证据。沿用 `docs/trading-rules.md`、现行 ADR 和主账，不声称本次重新查验交易所/中国结算原文。金额分、数量股及图表量能口径仍须跨层一致。

**限制：** 本复核确认候选 owner 与生产调用链在指定基线存在，不证明行为由测试验证或缺陷已修复；未运行 JSON/清单解析、产品测试或构建，未变更 G/Q 状态。
