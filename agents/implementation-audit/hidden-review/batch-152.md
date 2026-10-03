# 隐藏复核批次 152（owner=2）

## 范围与来源校验

- 按唯一 `scan-plan.json` 的 batch 152，连续读取主仓三份来源至 EOF；行数、SHA-256 与计划一致。来源及 `read_to_eof` 见配套 JSON。
- Caller worktree HEAD 为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`，与计划基线一致。已核对其 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`、`docs/architecture.md`、implementation audit 总账及相关 ADR。
- 本批材料是历史 Web 管理者审计/交接记录，不是产品实现变更。未改产品代码或 G/Q，未运行测试/构建，未执行 Git 写操作。

## 复核结论

来源中 `reviews-web-07.md` 自身存在需要保留的内部矛盾：开头 delta 明确指出 `apps/web/src/host/market-depth-sync.test.ts` 直接导入并调用 `applyPriceTickMarket`，而逐文件第 17 项、跨文件关系及最终修订清单仍错误地称没有直接测试。Caller 基线确认该测试确实存在并覆盖盘口及 best bid/ask 的成功同步；`market-depth-sync.ts` 在 caller 中只有定义及该测试引用，未发现生产 caller。因此能确认的是直接测试存在，不能由测试导入推断生产接线，也不能把历史 Markdown 的错误当成真实代码缺陷。

该来源还记录六个直接测试表述应修正；caller 基线中对应 `institution-policy.test.ts`、`urgency-policy.test.ts`、`portfolio-selector.test.ts`、`format.test.ts`、`symbolic-limit-order.test.ts` 和 `trade-input.test.ts` 均存在。其余状态区分与当前实现吻合：`InitialSaveSource`、`SessionReplacementGate`、`saveSelectionGenerationRef`、host/coordinator generation 分属不同状态所有者；Redux Toolkit slices 与 App 的 React 局部状态也不可混称。`frontend-review-notes.md` 的 contracts 序号校注只修正旁证文字，不涉及产品契约。

`frontend.md` 将 `SessionHostLifecycle`、`SaveCommands`、`TradingCommands`、`MarketChartProjection` 明确标为候选设计、未实施，并称本轮仅修改审计材料。Caller 基线已有相应生命周期 hook、`useSaveCommands`、图表投影类及 App 接线；这只说明后续状态已有实现，不证明所有历史候选设计均已照搬，也不构成新的实现义务。

与 ADR-0010 的宿主统一消息和局部刷新边界、ADR-0022 的符号限价语义相符；`trade-input.ts` 作为 UI 前置校验不能代替 engine 受理规则。来源没有提出新的交易制度主张。当前总账继续保留 G10–G14、G22–G25、G30–G34 等既有界面缺口及其验证边界；没有依据由本批增加、关闭或升级 G/Q。整体结论：来源主要结论应按历史审查记录理解，记录内部的 `market-depth-sync` 错误已由 caller 直接测试证据反证；没有形成新的已批准产品缺口。

## Caller / owner / consumer 核对

- 存档 schema parser 由 `parseSaveSlot` / strict envelope 路径组合；`InitialSaveSource` 与 `SessionReplacementGate` 位于 save 层，由 App 的启动及存档命令路径持有。`saveSelectionGenerationRef` 在 App/`useSaveCommands` 路径过滤过期文件选择结果。
- `MarketChartProjection` 由 `useMarketChartRuntime` 持有并服务图表投影；当前总账仍把图表引用更新问题列为 G31。对象存在不核销该消费行为缺口。
- `market-depth-sync.ts::applyPriceTickMarket` 有 host 测试 consumer；本轮搜索未发现生产 consumer。host 测试覆盖成功同步，不覆盖来源指出的缺盘口抛错路径。
- 大 A 语义未被历史 OOP/管理材料改变。价格选项仍按 ADR-0022 是普通限价单的选价方式；UI 输入辅助工具的单位/边界不构成 engine 规则证明。
