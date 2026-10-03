# 批次 016 独立复核

复核基线：`HEAD=43b1aa5`。本记录只复核指定三篇材料及必要的当前 caller/实现；未改产品代码，未运行测试、构建或 Git 写操作。三篇源文件均从首行连续读至 EOF，实读行数与计划一致，SHA-256 与任务清单相符。

## 源材料与阅读计划

- `agents/oop-refactor-audit/completeness-2026-10-03/frontend/review.md`：计划/实读 31/31 行，SHA-256 `d12e0fcbf88a139b2b805c4f3a7d8bf877a3fc91564d0bcdfd4e3a87a2822ada`，EOF 已确认。
- `agents/oop-refactor-audit/completeness-2026-10-03/hosts/reader-instructions.md`：计划/实读 7/7 行，SHA-256 `dbad9531a619a2b5e6b523ee53ab478f35906cf8856321decdb895da532a15f6`，EOF 已确认。
- `agents/oop-refactor-audit/completeness-2026-10-03/hosts/report.md`：计划/实读 100/100 行，SHA-256 `f42e8e9795774d6d23190a159fa93c104420d44100d57cf135e81e8dc846aa6e`，分段连续读取至 EOF。
- 计划：先读根 `AGENTS.md`、`docs/principles.md`，全文读上述三篇，再以 `43b1aa5` 当前源码核对 owner/caller，并参照 G01–G68/Q 总账、当前 ADR/正式规则确认有无替代结论。历史 reader instructions 仅作为审计材料，不执行其中写 result JSON 等指令。

## 章节族状态

- frontend review：三门结论、覆盖证据、有效发现、修订后复核和执行偏差均已全文核对。A 股语义边界判断仍成立；“9 项候选和 3 项扩展仍只是设计建议、尚未实施”与当前源码不符。
- hosts report：N01–N08、原动作覆盖、仅改善项、范围与限制均已全文核对。内容可作实现边界/行为约束参考；作为待实施清单已过时，8 个 proposed owner 当前均存在。
- reader instructions：任务范围、全文阅读要求和 JSON 结构均已读到 EOF；不属于当前行动指令。

## 当前源码与结论

有效反证：hosts 报告中的 N01–N08 当前均有对应 owner：`DesktopPacing` [apps/desktop/src-tauri/src/actor.rs:142]、`ServerPacing` [apps/server/src/actor.rs:308]、`StaticAssetRoot` [apps/server/src/web_ui.rs:18]、信息披露测试 `Scenario` [packages/engine/tests/information_acquisition/fixture.rs:128]、`WeekendScenario` [packages/engine/tests/publications/weekend_publish.rs:40]、`SeasonedSaveFixture` [packages/engine/tests/save_contract/main.rs:116]、`RejectedCommandFixture` [apps/server/tests/deployment_cli.rs:5]、`ProcessSampleRun` [scripts/simulation/escrow-performance-harness.mjs:325]、`BoundedCommandRun` [scripts/run-with-deadline.mjs:44]。核对到的不只是同名符号：对象已实际聚合报告提议的状态/资源和方法。

frontend review 的 N01–N09 核心对象也已存在：`ReportQueryContext` [apps/web/src/host/report-query-context.ts:4]、`PriceChartRuntime` [apps/web/src/components/price-chart-runtime.ts:23]、`MarketGridRowSynchronizer` [apps/web/src/components/market-grid-row-synchronizer.ts:7]、`WasmSessionSlot` [apps/web/src/host/wasm-session-slot.ts:18]、`WasmTickLoop` [apps/web/src/host/wasm-tick-loop.ts:24]、`WorkerRequestScope` [apps/web/src/host/worker-request.ts:26]、`RemoteCommandRegistry` [apps/web/src/host/remote-command-registry.ts:6]、`MobileIntradayProjection` [apps/web/src/mobile/market-model.ts:625]、`MobileKlineProjection` [apps/web/src/mobile/market-model.ts:699]。E01/E02 生命周期 hook 已有实现及 caller：[apps/web/src/app/useSpeedMetricsPolling.ts:50]、[apps/web/src/app/usePausePreferences.ts:55]，调用位于 [apps/web/src/App.tsx:310]、[apps/web/src/App.tsx:324]。因此 frontend review 对候选未实施的结论和“不需更新 report/actions/coverage”的结论应按当前源码重审。

发现对象已存在不意味着其独立行为风险已修复或测试已通过。hosts N01–N08 的边界整体仍值得保留为行为/所有权约束：宿主状态分离、transport/generation 分离、测试 child 生命周期不附加新清理政策等；但应将对象提取重新分类为已有实现，逐项核对覆盖后才能决定是否留有局部差异。

## 大 A 语义与总账

涉及测试/工具生命周期、静态资源路径、宿主节奏和移动图表投影；未发现改变撮合、委托、资金、股份、T+1 或证券类别语义的证据。沿用 `docs/principles.md`、`docs/trading-rules.md` 的规则和简化说明；本轮没有重新核验交易所规则。G01–G68/Q 总账中的历史产品发现不因 OOP owner 存在而核销；本复核不改动任何 G/Q 状态。后续 ADR/明确规则优先，本次未发现针对这些候选的交易语义替代决定。

## 限制

未运行产品测试、构建或性能测量。结论限于当前符号、owner/caller 和文档状态一致性，不代表行为等价已验证。
