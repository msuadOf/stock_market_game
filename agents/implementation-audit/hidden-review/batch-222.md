# 批次 222 历史材料独立复核

## 范围与核验

- 基线为 `.worktree/implementation-reaudit` 的 `43b1aa5`。按唯一计划读取三份指定来源至 EOF，并核对 SHA-256 与行数；没有读取计划外的历史审查材料。
- 对照现行审计总账、`exhaustive-review/resolution.md`、ADR-0007 §2 与 Q6，并静态核对 `market-model.ts` 的 collector 声明/调用、`useMarketChartRuntime` 的投影接线，以及 `loadViaUpload` 到 `loadFromFile`/读档命令的消费路径。未运行测试、构建、Git 命令或官方规则查询；未改产品代码和 G/Q 总账。

## 结论

未发现可证实的已批准承诺遗漏或错误历史核销。`web-05` 明确说候选设计尚未实施，其“通过”只评价调查材料的静态归属和事实准确性；`web-05-model-final` 只复核一个 market-model delta，不重新认证该批其他条目。当前生产图表路径由 `useMarketChartRuntime` 持有 `MarketChartProjection` 并消费 `NormalizedEngineUpdate`/frame；`MinutePointCollector` 和 `AuctionPointCollector` 在该模块中仅有定义，仓内实际实例化出现在测试。因此，历史材料对 collector 默认值和生产消费边界的区分与当前接线一致，没有把测试构造参数冒充生产交易时钟。

`web-06-final` 也把“通过”限定为 `loadViaUpload` 对应单一存档条目、items 与模块表格行的最终 delta 复核，并明确继承而不重审旧全批结论。当前 `loadViaUpload` 是 `loadFromFile` 的浏览器降级实现；`loadFromFile` 再由 `useSaveCommands`/应用入口消费。历史记录指出成功或读取失败路径不更新 `settled`，Promise 一次性完成不会因此覆盖结果；同时明确记录 focus listener 在 change/读失败后尚未触发时没有显式清理。源码与该有限描述相符。listener 生命周期仍是局部风险线索，但材料未证明违反已批准的清理契约或导致存档结果覆盖，本批不将其升级为 G/Q。

领域方面，这些材料审查的是前端模块归属、存档文件读取和调查证据，不改变价格、股数/手、交易阶段或交易制度，不提出新的 A 股规则主张。ADR-0007 §2 和现行 Q6 仍要求首发全中文界面；最新总账 G48 仍记录 HTML `lang=en` 与 AG Grid 辅助文本未中文化。`web-05` 的候选审查“通过”不表示该候选已实施，也不核销 G48；来源未作相反声称。

## 限制

本批只审阅计划所列三份材料及上述有限当前接线/规范依据；未复核它们提及的 items/modules、其他 receipts 或完整 web 审查，也未验证浏览器时序。结论不是完整前端验收，也不是 A 股规则复核。

## 来源

- `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/web-05-model-final.md` — 20 行，读至 EOF；SHA-256 与计划一致。
- `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/web-05.md` — 12 行，读至 EOF；SHA-256 与计划一致。
- `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/web-06-final.md` — 28 行，读至 EOF；SHA-256 与计划一致。
