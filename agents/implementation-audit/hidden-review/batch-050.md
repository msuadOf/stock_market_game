# 独立审读：批次 050（owner 5）

## 基线与范围

- scan-plan 指定 source baseline `43b1aa5`，解析为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`；source root、当前仓库与 caller worktree 的 HEAD 均为该提交。绝对 output root 为 `/data1/baiyifan/workplace/stock_market_game/.worktree/implementation-reaudit/agents/implementation-audit/hidden-review`。
- 三份源材料均连续读至 EOF。核对 scan-plan、逐行数与 SHA-256：`web-05.md` 509 行，`f9861b343c7bf19ec1a4381a24a828769960fff2d363133a97805db0ad3c20a6`；`web-07.md` 436 行，`e4488f07ce41193d20c95e8af780eacb24bc36c6c48043c17d7e279335b1f50b`；`web-08.md` 120 行，`a4cd2a0694cce0bbb6f5b84d0c5c9a414b00e43fbfafa6d58adc2c7159f02072`。均与计划记录完全一致。
- 对照 AGENTS、工程原则、架构、开放问题、G/Q 总账及 ADR-0023–0028；抽查目标基线下实际调用者、状态 owner、消费者。未改产品代码，未运行测试、构建或回归。

## 逐批审读结论

1. **web-05 的对象边界大体与源码吻合。** `MinutePointCollector`、`AuctionPointCollector` 只在 `apps/web/src/mobile/market-model.test.ts` 构造，当前生产 `useMarketChartRuntime` 使用自己的 `NormalizedTickFrame` upsert 路径；两个 collector 不构成生产行情事实 owner。两者分别聚合连续竞价分钟增量与集合竞价累计量，不能因候选类名相似而合并。`DayEndPersistence` 在 `App.tsx` 由 ref 持有，继续拥有 generation/epoch/tail；`day-end-persistence.ts` 在候选解析后、写入后及 writer 传入的 `isCurrent` 边界协调失效，App 返回启动页时会 invalidate 并等待 idle。记录指出 writer 内 I/O 检查由 writer/target 负责，符合实际契约。
2. **web-07 对存档、UI 与 Redux 分层的判断成立。** `InitialSaveSource` 由 App 外层 ref 持有，`SessionReplacementGate` 由 AppShell 持有；选择器 generation、host 身份与 gate 分开维护。`parseSaveSlot`/`parseStrictSaveEnvelope` 是严格 DTO parser，由 Worker/WASM host、文件与存档 repository 等边界调用，不应转成有隐藏状态的领域对象。`store.ts` 的 slice 保持可序列化状态；`setSnapshot` 只改快照和序号，`installProtocolSnapshotBaseline` 还更新 generation 并清除工作委托投影，不能当成同义动作。记录强调保留双投影顺序，避免重构误改委托就绪状态，属于必要的调用契约提示。
3. **web-07 关于交易 helper 的领域归属准确。** `buildPlayerOrderIntent` 由 `useTradingCommands` 调用，构造 UI 意图；`trade-input.ts` 做输入与展示前置校验，不能取代引擎受理检查。Highest/Lowest 保持符号意图并按 ADR-0022 在受理时解释。金额分/元、股/手、类别价格限制与创业板 UI 数量上限不可在“对象化”名义下重算或统一化；本批没有提出改动这些契约。
4. **web-08 的可选原型 candidate 边界清楚。** 两个 HTML 入口只是模块脚本声明；mobile-trading-concept 的控制器只是可选的原型 DOM 交互封装，价格、盘口和资金流都是硬编码图稿数据，不是引擎行情或真实 A 股数据。若后续实现该候选，需维持原有事件顺序、导航/图表联动及 DOM 显示语义，不得借此产品化行情、加入交易规则或把示例当规则依据。静态 DOM 拼装不需要机械创建对象。

## 当前代码、G/Q 与 ADR 交叉核对

- 当前明确 caller/owner：`App.tsx` 创建并持有 `DayEndPersistence`、`InitialSaveSource`；AppShell 创建 `SessionReplacementGate`；`parseSaveSlot` 被存档、文件和 host 边界使用；`buildPlayerOrderIntent` 的产品调用来自 `useTradingCommands`。`applyPriceTickMarket` 只在 `market-depth-sync.test.ts` 导入调用，尚未确认生产消费者；材料未把测试导入误称生产接线。`syncSnapshotTick` 的已确认覆盖来自移动端时钟测试，不应外推到其他快照投影路径。
- G/Q 总账现有 G01–G68（其中 G27 标为已核销，其余记录缺口），总账说明其源码基线为 `08e4fc7`，不能冒称本批对 43b1aa5 的完整 G 项再审。web 材料未声明与未关闭任一 G 编号。最接近的现行 UI/图表项 G10–G14、G22–G25、G30–G34，以及证券规则消费项 G68，均不能由本批的 DTO、旧调查 candidate 或既有单测描述核销；本批也未形成新的实现缺口主张。Q11 的补充方向由 ADR-0026 固定为游戏假设，Q12 已按 ADR-0024 解决；本批对象化审查没有提出资金流或 NPC 策略变更。
- ADR-0023 约束虚拟前史与撮合行情边界；ADR-0024 约束投资者现金池可减少；ADR-0025 约束只在成功日结更新持久档、读档/新局与旧异步写入隔离；ADR-0026 是机构策略假设而非交易所制度；ADR-0027/0028 规定宿主/构建和标签发布边界，ADR-0028 明确不改交易规则、资金/股数单位、策略与日终存档语义。web-05/07/08 不与这些现行决定冲突，也没有新的交易规则主张。未以二手材料替代交易所或中国结算法源审查，因为本批没有更改交易制度。

## 发现与结论

- 未发现三份材料中尚未处置的实质错误。材料所述候选均标记为未实施或可选，没有将 OOP 调查建议伪装为当前源码行为；经 caller 抽查，涉及的生命周期 owner 与交易受理边界一致。
- 风险边界：材料中“未发现直接测试”是旧调查的静态覆盖陈述，不表示本批重新运行了测试；本审读同样没有运行测试。`market-depth-sync` 的生产 caller 未核实，应继续维持“未确认”，不能因同目录测试而断言已接入。
- 审读结论：**本批三份文档审读通过；不构成全部 Web 源码复审、G01–G68 全量核销、规则法源更新或产品验收。**
