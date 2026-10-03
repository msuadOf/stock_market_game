# 隐藏历史复核批次 081

基线：`43b1aa5`。计划指定三份历史 review，均从主工作区绝对路径连续阅读全文至 EOF；未用搜索片段代替原文。实际行数依次为 35、36、25，合计 96 行，SHA-256 与计划一致。已读取 worktree 根 `AGENTS.md`、`docs/principles.md`，并核对现行 G01–G68/Q 总账、后续 ADR 和当前生产调用者。历史 review 是候选/设计审查，不因其“通过”而表示当时尚未实现的设计已经通过运行验证；当前实现状态另按 `43b1aa5` 源码判断。本任务没有改产品代码、执行 Git 写操作或运行回归。

## 逐篇结论

1. `web-01-app-final.md`（35 行，EOF）：原设计审查关于资源归属与 handler 写集的判断得到当前代码支持。`App.tsx:287-300` 将生命周期 hook 接入 composition root，`useSessionHostLifecycle.ts:55-85` 将取消标记、owned host 和 indicator registration 放在单 effect owner；App 的 `hostRef` 仍是借用别名。`App.tsx:352-360` 调用 save commands 并保留 handler 对外别名，`useSaveCommands.ts:51-61` 注明不复制已有 owner，当前交易调用在 `useTradingCommands.ts:22-35,58-83`。七个保留 effect 仍在 App（speed、pause preferences、running metrics、polling、visibility、daily chart），如 `App.tsx:302-350`。设计通过可延续为当前所有权边界证据，但不表示整个交易链无缺陷：总账 G22 仍记录表单只有全局 notice；旧 review 指出的快捷价 `DEFAULT_SETUP` 与下单 `activeSetup` 类别差异是独立交易线索，未被 OOP 提取修复。现行交易校验仍依活动 setup 的股票类别（`useTradingCommands.ts:64-67,113-118`）。

2. `web-01-chart-final.md`（36 行，EOF）：原设计审查关于 `MarketChartProjection` 只拥有派生行情缓存、hook/Provider 保持 React 与协议编排、消费者只读的边界，在当前代码中成立。生产 hook/Provider 通过 `MarketRuntimeProvider.tsx:17,49-50` 组合；投影缓存字段与写入口在 `market-chart-projection.ts:55-92`，消费者 accessor 在 `:94-105`。但旧 review 明确保留、且当前代码仍可直接确认的 G10/G12 现行缺口不能被对象提取核销：`upsertFrames` 对每帧累计成交量做差后按分钟 `mergeMinutePoints` 覆盖（`:34-43`），`continuousPoint` 仍固定 `buy: true`（`:8-16`）。本轮未提出新缺口；G31（accessor 导致图表数组引用更新）等总账项应以总账当前证据为准，不由旧设计通过结论核销。Review 对稳定 getter 的建议已由当前 API getter 实现；只读数组/派生对象的冻结和消费者刷新边界不等于实际浏览器验收。

3. `web-02-final.md`（25 行，EOF）：MarketGrid 对 `appliedRowsRef` 仅记账已提交目标、模拟指数文案不冒充真实指数、组件保留 React/Grid 资源的设计审查，与当前结构相容。当前 `MarketGrid.tsx:36-59` 仍由组件持有行数据和 `MarketGridRowSynchronizer`，指数计算位于 `:66-72`。此旧 delta 审查不是整个 MarketGrid 的重新审查或运行时证明。旧报告的 `retain` 只表示该候选迁移条目不需抽取，不会核销 G48/G64/G65 等后来总账识别的语言与辅助功能问题。

## 总账与决定对照

- 相关现行缺口仍是独立行为：G10 分钟量差替换/竞价量基线、G12 分时方向展示、G22 字段级错误，以及总账中与图表引用刷新相关的 G31；对象化并未修复这些行为。没有由这三篇材料产生的新候选。
- App 存档 owner 边界应服从后续接受的 ADR-0025（日终持久化与读档政策），不能把早期候选审查解读为允许日内写档。当前 `useSaveCommands` 的策略提示和异步 handler 是具体消费路径。
- 图表/协议 owner 关系延续 ADR-0004/0010 的 Redux authority、统一协议和局部刷新边界；交易命令最终仍由 EngineHost/engine 接受。未发现来源提出或当前代码引入新的 A 股撮合语义。
- 对照 2026-10-02 实现审计的 G01–G68/Q 总账及其“后续决定优先”说明，这三篇 OOP review 不改变任何既定 G/Q 状态，也不能替代独立完整 diff 复核或测试证据。材料都在 EOF 正常结束。
