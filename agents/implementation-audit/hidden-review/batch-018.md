# 隐藏扫描 batch 18

- 基线：`43b1aa5`；owner：3；caller：`/data1/baiyifan/workplace/stock_market_game/.worktree/implementation-reaudit`。
- 三个源均连续读取到 EOF，实测行数、SHA-256 与 `scan-plan.json` 相符；别名数均为 1。完整元数据见同目录 `batch-018.json`。
- 依据：caller 的 `AGENTS.md`、`docs/principles.md`、ADR-0017/0019；ADR-0018 仍为 proposed，报告引用的局部交易顺序只按 ADR-0017 修订和 ADR-0018 §7 的明确决定理解。N01/N02 是已记录候选；当前实现路径已接入，不把之后的实现当作历史报告内容。

## 源文件逐章核对

- `pipeline/rejected-reader.md`（5 行）：全文到 EOF。记录 p39 与 p01 结果因缺文件误判/哈希抄录错误被拒绝并重读；与 report 的最终替代 reader 说明一致。无独立业务或交易语义承诺。
- `pipeline/report.md`（245 行）：全文到 EOF。开头领域边界、N01/N02、原六动作覆盖、计数更正、111 文件逐项核销及结论均已核对。N01 描述一次调用内的诊断事件依赖聚合；N02 描述 checked auction tick 边界值；未声称修复交易行为、性能或变更 A 股规则。旧表中的 `auction_tail_boundaries`“保持自由函数”结论已被 N02 新候选限定取代，不能视为当前遗漏。report 将候选写为“未实施”只描述当时调查状态。
- `pipeline/review-n01.md`（23 行）：全文到 EOF。复核通过，明确 N01/N02 是后续候选而非已实施，并列出需保持的事件排序、错误位置、边界捕获与预算/交易语义。没有与现行 caller 源码相矛盾的遗留核销错误。

## 当前代码与调用链

- N01：`packages/engine/src/session/pipeline/continuous_lifecycle_projection.rs:125` 仅在 `order_lifecycle_events(...)?` 成功后一次性 append；`:137` 起 `ContinuousLifecycleEventBatch` 拥有六项本地依赖状态；`:152`–`:206` 保留 fill 数量链、Submitted 防重和事件处理；`:238` 起 fill 递减并在最后一笔后释放 terminal；`:287` 检查未解决依赖。失败仍在局部 buffer 内返回，没有逐事件写入 session。
- N02：`packages/engine/src/session/pipeline/auction_day_end.rs:940` 起定义私有 `AuctionTickBoundary`，`:947` checked capture 阶段/tick/day 事实；`:1017` 的 finish context 使用该值。生产 caller 在 `auction_tick_transaction.rs:230` capture 并于 `:231` 传给 `finish_auction_shards`；`stock_stream.rs:180` 按原边界字段调用每股 `finish`。`:897` 的 prepared finish 再 capture 一次，保留报告要求的原校验位置。没有发现漏接线或把不同股票合并排序的证据。
- 大 A 语义：变更只涉及连续订单诊断排序依赖与竞价收尾派生边界，不改变 Money/share、T+1、沪深差异、股票内价格时间优先、订单生命周期收据或 P0/P1/P3 预算规则。报告中的官方规则日期是历史登记，本文未重新联网核验，也没有声称重核规则。
- G/跨组关联：报告将 `auction_day_end.rs::apply_auction_lifecycle_projection` 的 ParentOrderPlan 子单状态写入归 `session-N01`，而 `pipeline-N02` 仅持有边界事实；当前 `AuctionTickBoundary` 不持有 PlanBook/权威状态。未发现重复归属或跨组所有权冲突。

## 结论

未发现已批准承诺遗漏、旧核销错误或新增可行动候选。两个历史候选在 caller 当前源码中均已实现并接线；这是代码路径核对，不代表本批运行测试或重新验证产品行为。未运行测试/回归，未改产品代码及 Git 状态。
