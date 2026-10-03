# Batch 169 独立复核

结论：未发现可确认的“已批准承诺遗漏”或“错误历史核销”。tooling-06、tooling-07 和 web-01 均属于 `before/exhaustive` 的候选设计/调查材料；明确记载未实施、未运行验证，不能把提议、问题线索或预期验证升格为批准承诺。web-01 曾提议的 `MarketChartProjection` 当前已在产品代码中实现，且当前 hook/Provider 使用投影；这属于后续状态变化，不使历史“未实施”声明失实。当前 `TradeMarketControls` 快捷涨跌停仍按 `DEFAULT_SETUP` 类别计算，而下单预检按恢复后的 `activeSetup` 类别计算；这是来源已明确登记、当前仍存在的 A 股语义一致性候选，不是新发现或已被历史核销的缺陷。

复核方法：按计划对三份来源全文读至 EOF，并独立核对路径、SHA-256 和行数。检查当前 `TradeMarketControls`、`useTradingCommands.buildIntent`、存档 `parseSetup`、图表投影实现/调用，以及 Pages smoke 的文件路径处理。批次不证明任何候选对象设计的完整性，也未运行测试或构建。

## 来源核验

- `agents/oop-refactor-audit/chinese-localization/before/exhaustive/modules/tooling-06.md` — SHA-256 `871a7f8f9d0c9486c22b9f65d8a56cf26680fd7a11855584212a90a4a0e48e95`，47 行，读至 EOF。
- `agents/oop-refactor-audit/chinese-localization/before/exhaustive/modules/tooling-07.md` — SHA-256 `a82124ec095e32e61f82c58609ba7771456f4ce8d7d0cbc131b5977a78bab97b`，31 行，读至 EOF。
- `agents/oop-refactor-audit/chinese-localization/before/exhaustive/modules/web-01.md` — SHA-256 `0d77da52954538f64ea09af40c00a37e5cb32a92529b869e67ba34312bb9e338`，314 行，分段读至 EOF。

## 当前交叉核对

- `TradeMarketControls` 由 `apps/web/src/App.tsx` 渲染，仍在 `apps/web/src/app/LocalRefreshViews.tsx` 用 `DEFAULT_SETUP.stocks` 查类别并计算价格限制；`apps/web/src/app/useTradingCommands.ts` 的 `buildIntent` 则从 `activeSetup.stocks` 查类别。存档 schema `apps/web/src/save/schema/market.ts` 接受 `MainBoard`、`StMainBoard`、`ChiNext`，因此恢复配置存在类别与默认 setup 不同的可能。后续修复应让快捷价格计算使用活动 setup，并测试不同类别恢复场景；本批不改代码。
- 图表候选已有现行实现：`useMarketChartRuntime` 创建并使用 `MarketChartProjection`，测试覆盖缓存行为；当前产品消费边界通过 `MarketRuntimeProvider`/runtime hook，历史报告提到的 `priceHistoryByCodeRef`、`activeDailyCandlesRef` 旧 API 已不再出现在当前调用点。这是历史设计提案后续落地的状态，不是“已批准承诺遗漏”。
- tooling-06 指出的 Pages 静态 smoke symlink 边界在当前 `scripts/smoke-pages.mjs` 仍可见：只以 `path.resolve` 做词法前缀检查，随后 `readFile` 跟随 symlink。若构建输入目录可含外指 symlink，脚本可能读取 root 外文件。该线索在历史来源已作为未修候选明确记录；本轮将其作为仍待处置风险通知协调者，不声称验证了可利用条件。
- tooling-07 的 `task-30-worker-harness.mjs` 是历史手写验证脚本；来源说明其非正式测试入口，不能据历史成功声称当前 WASM API 兼容。本轮未运行该脚本。WASM 构建的 `.sh`/`.bat` 入口当前仍分别转交 `frontend-build.mjs`，README/构建文档存在调用说明。

未运行测试、构建或 Git 操作。未修改产品文件。
