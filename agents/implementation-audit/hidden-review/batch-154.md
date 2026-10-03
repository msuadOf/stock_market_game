# Batch 154 独立复核

## 结论

未发现来源证明的已批准承诺遗漏或错误历史核销。三份材料均是候选设计/对象归属调查，并明确指出未实施，不构成生产交付承诺。`TradingPlan` 候选的主要封装边界在 baseline 已落实为 `pub(in crate::plans)` 字段、只读 getter 和 inherent 状态转移方法；外部消费者通过 `PlanBook`、执行器及只读 API 使用它。没有必要把候选的初始状态误报为当前遗漏。

交叉核对发现来源中若干行为风险在 baseline 仍可见，应作为独立实现审查候选，不应归类为 OOP 承诺：`InventoryLedger::receipt` 在成本累加失败前已增加数量，且现有单测明确固定该部分变更；工业采购先提交 `Books` 再收货。`TradeOpenLedger::write_off` 先清空开项再 checked-add 核销总额，工业核销调用方也先提交 `Books`。合并抵销未拒绝零/负 `IntercompanyBalance.amount`，配对相同的非正金额会进入工作底稿。来源自己已将它们与 OOP 候选分开；本轮未改产品代码、未运行测试，也未从官方会计依据重新审定这些假设/规则。

`ClosingEngine::close_year` 与 `correct` 的多步失败窗口仍只是源码顺序上的可能性；材料没有证明后续步骤存在确定可达失败条件，本轮亦未发现新增可触发证据。双生成目录差异和 `TradingPlan` restore 验证线索也需各自针对生成/存档契约进一步核查，不宜由本批报告直接判为缺陷。

## 来源校验

三份来源均已从首行读取至 EOF；路径、SHA-256 与行数均匹配 scan-plan。对 `TradingPlan` 的当前 consumers 做了搜索，范围包括 `packages/engine/src/plans`、执行器、决策链、`packages/engine/tests/plans.rs` 及 Web 存档类型/解析器；未穷举全仓全部字段消费者。对会计行为风险交叉阅读了对应子账与工业 caller 实现。未运行测试、构建或 Git 操作。
