# 批次 015 独立复核

复核基线：`.worktree/implementation-reaudit` 的 `HEAD=43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。按主任务要求读取项目准则、原则、相关 ADR/open-questions，并连续阅读全文至 EOF：`final-review.md` 48 行、`frontend/report.md` 600 行、`frontend/review-final.md` 34 行。未写产品源码、未运行测试/构建或执行 Git 写操作；只新增本复核的 Markdown 与 JSON。

## 源材料完整性

- `agents/oop-refactor-audit/completeness-2026-10-03/final-review.md`：48/48 行，SHA-256 `f29248c54bf68fbef9b4c0b6f6ed4dcfd1a813b557d2bc4866392deef1bfb074`。全文覆盖审查范围、三门、修订复核、结构/版本核验与限制。
- `agents/oop-refactor-audit/completeness-2026-10-03/frontend/report.md`：600/600 行，SHA-256 `d7879d0f45106a71ed96c2a253f6f774f291d66de6f4deda99ab3a96eaf74673`。全文覆盖总论、N01–N09、E01–E03、反向发现、证据纠正、验证及执行偏差。
- `agents/oop-refactor-audit/completeness-2026-10-03/frontend/review-final.md`：34/34 行，SHA-256 `2a58b3674fefb3ad024a2597c6840d727951cc19028febb3906a866625e7dbb4`。全文覆盖增量纠正、三门结论、指纹及验证边界。

## 主账与当前实现

截至基线 `43b1aa5`，前端报告中 9 个新增候选及 3 个扩展都已有对应生产 owner/caller。报告的领域语义边界与独立行为风险仍有参考价值，但“未实施候选”这一状态已过时，不能再据此重复登记 OOP 实施工作。

- N01 `ReportQueryContext` 已由 `remote-host.ts:47` 唯一持有；分页登记、ID/company 归属和 generation/时间线校验由 `remote-host.ts:274-289` 真实调用。`report-query-context.ts:4-41` 实现对应方法。
- N02 `PriceChartRuntime` 由 `PriceChart.tsx:47,60-75` 创建、更新、清理；`price-chart-runtime.ts:23-226` 管理图表资源。候选明确排除的半初始化失败资源回滚仍是另一个行为边界，不能因为 owner 存在而标记修复。
- N03 `MarketGridRowSynchronizer` 由 `MarketGrid.tsx:47-58` 持有，`onGridReady`/effect 分别 attach/update。`market-grid-row-synchronizer.ts:7-50` 接管行同步；Grid 的真实 UI 入口仍是 `MarketGrid.tsx:157-165`。
- N04/N05 `WasmSessionSlot` 与 `WasmTickLoop` 分别由 `wasm-worker.ts` 组合，实际调用见 `wasm-worker.ts:93` 及 `:143-316`；定义分别在 `wasm-session-slot.ts:18`、`wasm-tick-loop.ts:24`。session 与 timer/publish 生命周期仍分开。
- N06 `WorkerRequestScope` 在 `worker-host.ts` 由单一 host 创建并用于真实命令；定义及 request ID/generation 过滤、listener/timer 清理见 `worker-request.ts:26-73`。postMessage 同步抛错仍依 timeout 清理，host dispose/fatal 取消请求也仍是单独行为缺口。
- N07 `RemoteCommandRegistry` 由 `remote-host.ts:48` 持有；提交、queued/gateway 响应与 fail 路径接线见 `remote-host.ts:55-62,209-216`。`CommandQueued` 仍只代表入队，不是受理或成交。
- N08/N09 `MobileIntradayProjection` 与 `MobileKlineProjection` 定义于 `market-model.ts:625`、`:699`，实际组件调用在 `MobileStockDetail.tsx:81-105`。投影类已存在不表示 G10–G14 已解决；主账仍把连续量累计、最近成交方向和逐笔时间等列作现行缺口。集合竞价累计量与连续分钟增量、算术均价和周/月按游戏交易日聚合的简化不可在抽取中改变。
- E01/E02 hooks 已在 `App.tsx:310-317,324-326` 使用，定义分别位于 `usePausePreferences.ts:55`、`useSpeedMetricsPolling.ts:50`。Redux 偏好 authority、共用 speed request gate 和 AppShell 显示 state 等候选边界在现行接线上仍成立。
- E03 是静态 HTML 原型 controller 字段补充；本轮没有据现行 A 股或主账发现其需要另立生产 owner。

## 主账交叉核对与结论

当前 `agents/implementation-audit/implementation-audit-2026-10-02.md` 将 G10–G14、G18、G31、G64 等继续列为现行缺口。它们属于量能投影、交易帧背压、引用隔离、可访问输入等独立契约，不因 N02/N03/N05/N08/N09 的对象存在而自动核销。例如 G13 当前 `market-model.ts:655` 仍从最新优先数组 `slice(-7).reverse()`；G18 的 tick loop 存在也不等于有消费端背压。主账和候选应同时表达“owner 已实现”与“其外部行为仍有缺口”，避免把不同问题归给一个 OOP 提取。

**有效发现：** frontend report 的“候选设计，未实施”及“9 new + 3 extension 是待实施工作”结论与基线源码矛盾。建议将 12 项重分类为已有实现对照，逐项保留真实 caller/owner；只把尚未履行的行为契约按 G/Q 或独立行为修复登记，不重复计算 OOP 工作。review-final 对覆盖集合、指纹和本轮未重新查官方规则的限定仍有效；它没有核对这 12 项在 43b1aa5 的实现状态。

**大 A 语义：** 静态核对未发现候选 owner 改变撮合、委托、资金、股份、T+1 或证券类别语义的证据。沿用 `docs/trading-rules.md`、现行 ADR 和主账，不声称本次重新查验交易所/中国结算原文。金额分、数量股以及图表量能口径仍须跨层一致。

**范围与限制：** 本复核只确认候选 owner 与真实生产调用链在指定基线存在，并指出已知主账缺口的不同归属；不证明所有实现细节通过测试、不运行行为验证，也不修改 G/Q 状态。无 JSON/清单解析或产品测试执行。
