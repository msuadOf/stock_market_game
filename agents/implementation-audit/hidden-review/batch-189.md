# 批次 189：engine pipeline 历史复核记录与当前实现

## 来源读取与指纹

基线 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad` 与当前 `HEAD` 一致。已阅读项目 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md` 及 ADR-0017 相关契约。三份指定历史记录均从头连续读取至 EOF；路径、SHA-256 和行数与计划一致。

| 来源 | 行数 | SHA-256 | EOF |
|---|---:|---|---|
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/engine-pipeline-11-final.md` | 34 | `40a725b9d574bedc27a705a561d0c5db3aeda8273ef8d45dfded4a10fa32b40b` | 已读 |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/engine-pipeline-area-final.md` | 44 | `d3131a9471f50c4f4ace0d440a8a6400b632519dff52563bbc398f67561c8bff` | 已读 |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/engine-pipeline-closure-final.md` | 55 | `bbef13a7bee86426edf732451c3b549b8bb7985453ac2752b57857a3464615a1` | 已读 |

## 当前 owner、调用者与消费者

- `packages/engine/src/session/pipeline/stock_stream.rs:271-455` 当前已实现私有 `StockStreamCoordinator<S>`；`drive_stock_stream` 在每 tick 建立并驱动它。它拥有 `available`、`pending`、`in_flight`、typed completion channel 及 notifications 等临时调度状态，不持有账户、结算或 session authority。`StockShard` 的具体实现仍由 `IncrementalContinuousStockCoordinator` 与 `IncrementalAuctionStockCoordinator` 提供。
- 当前调用者是 `apply_session_continuous_transaction`（`continuous_tick_transaction.rs:181-228`）、`apply_session_auction_transaction`（`auction_tick_transaction.rs:160-211`）和 `apply_session_pre_open_transaction`（`pre_open_transaction.rs:188-237`）。它们分别消费 `StockStreamProgress` 并在排空后调用对应的 `finish_*_shards` 与事务终结流程；stream 不取得交易优先级或最终账本所有权。
- 当前实现保留调用线程协调的 `rayon::in_place_scope`、按股票 key 路由、PlanRoot 与 Stock notification 分流、Stock 完成后取 typed payload、payload 发送失败时丢弃已放弃 tick 的结果，以及 drain 后由调用者继续完成事务。与 `engine-pipeline-11-final.md` 的 delta 契约一致。
- `day_end_release_receipt` 当前位于 `stock_auction.rs:557-566`，设置 `ReceiptSource::DayEnd(source_index)`、ordinal `0`、`ReceiptKind::Release` 并委托 `terminal_receipt`；调用点包括 `auction_day_end.rs:1451` 和 `incremental_continuous_stock_shadow.rs:467`，另有 `continuous_tick_finalizer.rs:461` 的对应调用。

## 历史结论与候选状态

1. `engine-pipeline-11-final.md` 明确是 unit024/045 的局部 delta 复核，继承旧的 11-file review；它没有声称本轮重新审查全部源码。其收据 helper 归属、四个 inline 测试分类及 payload/notification 关闭边界，与当前读取的实现相符。
2. `engine-pipeline-area-final.md` 核验的是 area/items/modules 与链接，不是 111 个源码文件；当时记录的 `execution-closure.json` 失效链接是历史发现。`engine-pipeline-closure-final.md` 后续记录 closure 文件与 area 指纹闭合，说明该文档追溯问题已在当时关闭。
3. 上述 area/closure 文档将 `11-A01 StockStreamCoordinator<S>` 描述为候选，closure 还称候选尚未实施。此状态仅是 2026-10-03 的历史记录：在本批基线中，该 owner 已存在于 `stock_stream.rs`，故不能将该旧状态当作当前待实现事项。当前代码实现范围与候选边界相符；本次未作完整 diff 审查，也未运行测试或构建。
4. 未发现新的 A 股制度主张或交易语义变化。现行代码在 tick 内调度股票工作并交付 typed 结果；优先级、撮合、收据及提交语义仍由各领域 coordinator/transaction owner 管理。没有依据因历史 OOP 候选新增产品 G/Q。

## 门禁与限制

- **大 A 语义：** 本次只核验历史材料和当前 owner/callers；未提出交易制度变更，不需新增官方规则断言。
- **必要性与范围：** coordinator 已按私有、每 tick 的生命周期 owner 实现；不应将其解释为新的持久 session authority 或正确性修复。
- **边界与跨层语义：** 核实了三条当前生产调用路径及 DayEnd helper 调用点；未重新审查整个 pipeline 或所有测试，也未运行验证命令。结论限于已列范围。

**结论：** 三份来源指纹有效且均已读至 EOF。历史 candidate 记录存在时间状态差异；基线代码已包含 StockStreamCoordinator，当前调用者与既有契约一致。未发现需新增的实现审计项。
