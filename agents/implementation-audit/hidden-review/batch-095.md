# 批次095：引擎流水线与会话历史候选复核

## 范围与依据

复核基线为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。连续全文读取三份指定材料至 EOF，并核对行数与 SHA-256：`engine-pipeline-11.md` 22 行，`5405ca08fd3f77b5767bcc199535a5fdbab9244922a42a613bf53c428ee50159`；`engine-pipeline-12.md` 22 行，`47afe645525cea41d1db1c931709ab19f5fdc365c2eb7ed2e84e21a09ddbee87`；`engine-session-01.md` 22 行，`c61f13f12a84b50a693a809ed9478dbbeee71561b6c545653b95764d6a1f6812`。

遵循根 `AGENTS.md` 与 `docs/principles.md`；对照现行 ADR-0009、0014、0015、0017、0018、0019、0022、0024、0025、0028 及 `docs/open-questions.md`。其中 ADR-0017、0025 为 accepted；ADR-0018 仍为 proposed，只能将其已明确接受的局部约束按文内说明使用，不能把整套长期历史/WAL/COW设想当成当前契约。open-questions 开头明确 escrow 阶段按 ADR-0017 已决；本批未发现需要借这三项 OOP 材料重新裁决的开放问题。A股判断限于保持现行沪深差异、成交/费用及资产生命周期边界；未重新查询官方规则，也未提出制度变化。

## 基线核对

- **engine-pipeline-11：** 历史正文提出的唯一生产候选 `StockStreamCoordinator<S>` 已存在于基线 `packages/engine/src/session/pipeline/stock_stream.rs:271-302`。`drive_stock_stream` 构造 coordinator 并委托其 drive；coordinator 持有本 tick 的 available/pending/in_flight、完成 channel 和 notifications，不取得账户、Settlement 或 session authority。调度、completion接收路径见 `:323-404`；调用方仍以各股票 shard 运行并在适用阶段收尾。故历史“候选未实施”状态已过时，不能重复登记 A01。此确认只说明抽取存在，不证明全部并行性能或运行验收已通过。
- **engine-pipeline-12：** 历史结论是保留纯 `FillTransition` 输入/输出边界，没有生产迁移动作。基线仍由 `transition.rs` 的 `FillTransition::buy/sell` 基于显式输入计算成交腿结果；连续撮合和竞价分别调用（`continuous_matching.rs:1147-1160`、`stock_auction.rs:592-609`）。费用与资源变化仍从调用事实派生，不将订单簿、账户或长期生命周期重复持有。未发现历史结论与当前实现相悖的理由。
- **engine-session-01：** 原 A01 的私有 `CommittableSessionState` 已实现：`GameSession.state` 唯一持有该组合（`session.rs:1131-1135`），`clone_for_tick_shadow` 委托 `state.clone_for_shadow`，`commit_tick_shadow` 委托 `state.commit_from`（`:1328-1340`、`:4075-4170`）。生产候选建立、TickShadow/candidate commit 和自然日日终 checkpoint 都仍经过这些入口（`:1416`、`:2088`、`pipeline/shadow.rs`、`pipeline/candidate_commit.rs`、`pipeline/auction_day_end.rs`）。这并不表示所有状态只在 P9 写入；历史材料也明确指出 `pending_player` 入队、民用时钟入口及独立日终事务边界，当前候选不得把这些边界误述为新缺陷。

## 主账与结论

三份历史材料没有提出新的交易制度或仍待实施功能。相关现行差距保持独立：G16 的长历史复制成本不能由私有 state 聚合或 stock stream coordinator 抽取核销；G39 的 K7 跨 worker 产物验收不能由候选对象存在核销。其余流水线/撮合缺口也不能由 `FillTransition`、coordinator 或 session state 的类型存在推导关闭。没有发现可将这些历史候选直接登记为新 G/Q 的依据；不改写 G/Q 状态。

结论：pipeline-11 与 session-01 的候选已在基线实现；pipeline-12 继续保留现有窄纯计算边界。三份材料中有效的沪深撮合、收据、Settlement、T+1、P9原子提交和失败隔离约束仍应维持，但此复核不是这些运行时行为的重新验收。改动必要性结论是无需再实现同一候选；没有发现超出历史范围的新复杂度或应新增行为测试的独立候选。

本批只读检查代码和文档；未修改产品代码、Git 状态或 G/Q 台账，未运行测试、构建、性能验证或回归，未核验官方规则来源。源码静态对应不能代替运行时证据。
