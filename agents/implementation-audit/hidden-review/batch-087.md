# 批次 087 文档复核

## 输入完整性

- 主工作区基线 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`；caller、owner 与 consumer 取自 `.worktree/implementation-reaudit` 的同一基线。
- 已按扫描计划连续读取三篇指定来源至 EOF；来源行数和 SHA-256 与计划一致，详见配套 `batch-087.json`。
- 已阅读 `AGENTS.md`、`docs/principles.md`、`docs/architecture.md`、`docs/open-questions.md`、`docs/decisions/` 及最新实现审计总账。相关决策重点为 ADR-0017 的并行边界、ADR-0018 中已被后续接受的具体条款，以及 ADR-0019 的容量范围；ADR-0018 整体仍为 proposed，不能将未接受章节作为现行契约。ADR-0020 仍为 proposed，不据此引入分配器决策。

## 复核结论

`final-review.md` 是 OOP 调查范围、证据与复核入口说明；`reader-instructions.md` 是当时逐文件扫描任务规约，不是新的生产需求。两者不改变当前产品契约。`engine-pipeline-11.md` 对 `StockStreamCoordinator<S>` 的有限抽取评价在其历史审查时间成立；其中提议的对象现已存在于审计基线，不能当作尚未实施的候选重复登记。

当前生产 caller 为 `continuous_tick_transaction.rs`、`auction_tick_transaction.rs` 与 `pre_open_transaction.rs`，均通过 `drive_stock_stream` 接入该协调器。`stock_stream.rs` 中私有 `StockStreamCoordinator<S>` 持有本 tick 的可用/待执行股票、进行中工作及 payload/notification 接收与调度状态；各 `StockShard` 仍持有单股工作。连续/竞价交易各自的事务负责在 stream 完成后统一 finish，并继续外层候选提交。协调器不持有账户、Settlement 或 session 权威状态。

历史 review 对调度职责、股票键路由、payload 与 notification 区分、排空后收尾、类型化致命错误传播的要求与当前 owner/caller 边界相容。此处只确认抽取和接线存在，不宣称其并行性能或验收已经通过，也不将容器顺序、通知到达顺序或完成顺序解释为交易优先级。A 股交易语义没有新增主张或变更；官方现行规则未重新联网核验。

## 总账与状态

最新总账仍将 G16、G39 等验证/性能边界单独记录；本来源没有提供它们的新反证，也不能凭协调器存在核销。Q 项和 ADR 状态均不需更新。没有新的 G/Q 建议，没有升级或修改既有条目。

静态复核未发现本批历史候选作为当前待办的依据。未运行测试、构建或性能验证；结论范围限于三份指定材料及基线 caller/owner/consumer 交叉核验。
