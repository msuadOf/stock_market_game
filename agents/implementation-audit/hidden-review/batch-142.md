# 批次 142：engine pipeline 历史模块记录审读

- 基线：`.worktree/implementation-reaudit`，HEAD `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。
- 按 scan plan 中 `id=142, owner=2` 指定三份来源逐篇从首行连续阅读至 EOF；SHA-256 与行数均与计划一致，见配套 JSON。
- 已阅读仓库 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`、`docs/architecture.md`、相关 ADR-0009/0014/0017/0018/0019、`docs/trading-rules.md` 和现行实现审计总账；caller 源码取自指定 worktree 基线。

## 来源要点

1. `engine-pipeline-03.md` 是竞价与日界管线的候选设计记录：建议由 `AuctionStockShadow` 拥有单股 round/finish，由 `AuctionLifecycleProjector` 承担一次性 parent/plan/retail 投影；不把无状态边界包装成 `AuctionDayEndTransaction`。记录强调候选失败丢弃、显式 receipt/fact identity、P6 统一结算、开盘余单按原到达顺序转入连续簿，并列出竞价与日界测试边界。不是额外批准产品迁移或交易规则。
2. `engine-pipeline-04.md` 是提交边界与投影的 retain 记录：保留 `PreparedCandidateCommit`/`PreparedTickPlanCommit` 的 P9 typestate、显式证据快照、现金/股份二维守恒、单 tick 生命周期投影、无状态校验函数和测试 fixture；不增加常驻状态 owner。ADR-0018 的 proposed 内容不能作为重构改变交易先后的授权。
3. `engine-pipeline-05.md` 是连续撮合与收尾候选：建议一次性 `ContinuousStockRoundProcessor` 集中推进单轮 market shadow、ledger、receipt、facts 和诊断；跨轮协调仍归 coordinator，adapter 与 projection adapter 保持窄边界，finalizer 只处理调用方候选。明确不得改变 P3/P4 身份与资源语义、同股受理顺序、费用、T+1 或失败原子性。

## 基线 caller / owner / consumer 核对

- Auction 当前实现已有 `AuctionStockShadow` 以及 `AuctionLifecycleProjector`；`IncrementalAuctionStockCoordinator` 管跨股分派和 round，`auction_tick_transaction` 调 `finish_auction_shards` 后进入候选 finish。基线位置包括 `auction_day_end.rs:301,444,479,503,1779,2140` 与 `auction_tick_transaction.rs:231`。`apply_finished_candidate` 和 `validate_worker_finalizers` 仍为独立函数，符合其无共享状态的边界。
- Continuous 当前 `process_continuous_stock_step_inner` 构造并运行 `ContinuousStockRoundProcessor`；`IncrementalContinuousStockShadow::apply_round` 续接多轮状态，`finish_for_tick` 在轮次完成后消费 stock finish。基线位置包括 `continuous_matching.rs:250,254,272` 与 `incremental_continuous_stock_shadow.rs:176,342,349,385,443`。
- P9、receipt/conservation/projection 分层与历史 retain 判断相符。最新总账仍保留 G17 的 Rayon 批量生产接线缺口、G39 的 K7 实际受理轨迹验证缺口；本批 OOP 记录既不能核销这些项，也不产生新 G/Q。总账 R02 记载真实权威入口已接线，同时长期吞吐/全门禁仍属独立验收。

## 领域与判定

ADR-0017 accepted 是当前阶段、shadow/P9、局部冲突和守恒约束依据；ADR-0009/0014 与 `trading-rules.md` 保留开盘/收盘集合竞价阶段、沪深竞价差异和游戏简化；ADR-0018 仍为 proposed，其中已决局部受理规则按其修订和 ADR-0017 前言理解，未决部分不覆盖现行实现。范围属于 owner / 生命周期审读，没有发现历史材料指示改变沪深交易阶段、费用口径、Money/股单位、T+1 或结算语义。本批不重新查询官方规则。

历史候选状态：03 的 stock owner/projector 与 05 的 round processor 已在基线实现；04 的保留真实 owner、纯函数和 fixture 判断继续成立。旧记录中的测试建议不是执行结果，本批没有运行测试或构建。候选及反证均仅作审计记录，不升级或修改 G/Q。

结论限于三份来源及上列直接 caller/owner/consumer，不代表全量 pipeline 审计或产品验收；未修改产品代码、正式文档、测试或 Git。
