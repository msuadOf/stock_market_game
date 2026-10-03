# 批次 205 独立复核

## 范围与结论

复核计划 `scan-plan.json` 中 Batch 205 的三份历史复核文档，确认其审计结论、A 股语义限定与现行架构边界相符；并按任务要求核对 baseline `43b1aa5` 和当前的调用者、所有者、消费者，以及相关 G/Q 与现行 ADR。未运行测试、构建或修改产品代码。

**结论：通过（审计材料范围）。** 三个源文件的 SHA-256 和行数均与计划相同，均读至 EOF。未发现有效阻断项。engine 区域文件清楚区分测试契约、synthetic fixture 与正式 A 股规则；frontend 区域文件把对象候选作为边界分析而非已实施声明，也保留产品层、Redux 与 engine 的现有职责。

## 证据核对

- `engine-tests-10.md` 记录修订后独立复核通过，同时保留“初审未通过”的历史；其结论指出 strategy 混合板块选股的统一 synthetic 10% bounds 只验证选股覆盖，urgency/patience 是游戏模型参数。没有把零股、100 股、T+1、价格带等单项 fixture 夸大为全市场规则验证。该文明确未审生产实现、未运行测试，也未独立核对交易所来源时效性。
- `engine-tests-area-final.md` 的 176 条、160 条 `support`、16 条 `retain`、所有 `actions` 为空及第 09 批 118 个测试等摘要，均将结论限定在迁移导航和既有源码复核材料；文档清楚声明未运行测试/构建，不声称生产代码无缺陷。
- `frontend-area-final.md` 限定为文档间一致性复核，明确未重读业务源码。四项候选边界分别保留生命周期借用引用、存档权威、交易 host/engine 受理及 React/Redux 编排；硬编码行情和原型交互限定为设计样例，没有作为真实规则依据。
- `docs/trading-rules.md` 现行规则明确普通主板与创业板涨跌幅差异并要求逐股显式配置；engine 文档中的合成 10% 多板块 fixture 限定准确，没有与该规则冲突。
- 相关已决问题/ADR 与复核边界一致：Q6 与 ADR-0007 规定界面语言/前端栈；Q9 与 ADR-0005 确立宿主无关 engine、统一账户、撮合及对外 T+1；策略参数另由 Q11/ADR-0006 等游戏模型决策承载。没有从这些决策推导出新交易制度。

## 调用者、所有者与消费者

在 baseline `43b1aa5` 与当前工作树，`App.tsx` 均调用 `useSessionHostLifecycle`、`useSaveCommands`、`useTradingCommands`，并挂载 `MarketRuntimeProvider`。`MarketRuntimeProvider` 调用 `useMarketChartRuntime`，经 actions/selection/data Context 提供给本地视图；`MarketChartProjection` 仍由 runtime hook 持有并用于行情展示投影。六个相关实现文件相对该 baseline 无差异，因此本次没有发现历史复核叙述与当前调用链漂移。这个核对只验证接线与对象归属，不重新审查完整实现行为。

## 三门结论

1. **大 A 语义：通过。** 合成 fixture、简化范围及游戏策略参数均有边界说明；无规则外推或板块语义混同。
2. **必要性与最小范围：通过。** 三份材料服务既定测试迁移/职责审计，没有将测试支撑升级为生产领域对象，也没有把候选抽取写成已实施变更。
3. **遗漏边界与跨层一致性：通过。** 审计报告明确自身未覆盖的生产实现、交易所来源时效和运行验证；当前 caller/owner/consumer 与 baseline 一致。未发现需要修订的有效发现。

## 来源完整性

来源指纹及 EOF 核对结果见同目录 `batch-205.json`。指纹来自工作树全文读取前后核对；三个文件实读行数与计划完全一致。
