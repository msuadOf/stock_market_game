# 批次 146：engine pipeline 历史复核记录审读

- 基线：`.worktree/implementation-reaudit`，HEAD `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。
- 依据 `.worktree/implementation-reaudit/agents/implementation-audit/hidden-review/scan-plan.json` 的 id=146、owner=1；三份主工作区来源均逐篇连续读取至 EOF，行数和 SHA-256 与计划一致，见配套 JSON。
- 已阅读 worktree 根 `AGENTS.md`、`docs/principles.md`。没有修改产品代码、Git 或测试，也未运行测试/构建、未重新查询交易所规则。

## 历史记录章节族及状态

1. `engine-pipeline-03.md`：章节族包括竞价 coordinator/shadow 所有权、lifecycle projector、保留的纯函数边界、交易与收尾约束及行为测试支撑。旧审查明确不创建无状态 `AuctionDayEndTransaction`，建议将单股 round/finish 与共同推进账本的 lifecycle 投影放进有状态 owner。当前实现已包含 `AuctionStockShadow`、`AuctionLifecycleProjector`；保留独立纯函数的判断也与代码形态相符。历史记录描述的是设计审查，不是产品规则变更。
2. `engine-pipeline-04.md`：章节族包括 11 个源/测试文件的候选与 retain 判断、提交/P9、收据/守恒/生命周期投影边界、覆盖和文件间组合。主要结论是保持真实状态 owner，避免为纯校验、事件整理或测试 fixture 新建对象。当前候选代码仍以准备/提交 token、纯函数和阶段 DTO 表达这些边界；其“不增加常驻对象”的判断再证成立。模块材料标为“候选设计，未实施”是当时状态，不能覆盖基线源码事实。
3. `engine-pipeline-05.md`：章节族包括连续撮合 round processor、adapter、事实与 receipt、lifecycle batch、finalizer、测试支撑和跨文件生命周期。旧 review 发现模块说明对“轮结束最终 market/ledger 对账”措辞不够明确，要求区分单轮/独立完成校验与所有 continuation 排空后的 coordinator 最终校验。当前实现记录与代码保留了这两处生命周期边界；该旧文档问题是历史材料表述发现，不代表交易账本实现缺陷。

## 基线调用与候选复核

- Auction 单股状态现在由 `AuctionStockShadow` 的 `apply_round`/`finish` 承担，coordinator 仍有跨股分区、稳定错误选择和统一 finish 责任；`auction_tick_transaction` 调用 `finish_auction_shards`，然后进入显式候选聚合。见 `packages/engine/src/session/pipeline/auction_day_end.rs:301,444,479,503,1023` 和 `packages/engine/src/session/pipeline/auction_tick_transaction.rs:231`。
- Auction 生命周期候选由 `AuctionLifecycleProjector::apply` 处理；无共享状态的 `validate_worker_finalizers`、`apply_finished_candidate` 等仍独立。旧 03 结论“不抽空壳事务类、保留单股 owner 与 projector”的候选状态为**已在基线实现**，而不是尚待执行的实现指令。见 `auction_day_end.rs:1731,1779,1785,2140`。
- Continuous round 的 `ContinuousStockRoundProcessor` 在 `process_continuous_stock_step_inner` 初始化并运行；多轮 `IncrementalContinuousStockShadow::apply_round` 续接 shadow，coordinator 的 `finish_for_tick` 在所有轮次完成后消费每股 finish。故旧 05 对单轮处理器与跨轮最终验证 owner 的划分得到当前调用链支持。见 `packages/engine/src/session/pipeline/continuous_matching.rs:250,254,272`、`incremental_continuous_stock_shadow.rs:176,342,349,385,443`。
- 候选实施后的工程记录列明生产 callers 和迁移范围，但其中测试建议/测试清单不构成运行证据。本批未运行测试，不能据此宣称行为测试通过。

## 领域、总账与限制

ADR-0017 为 accepted，要求 shadow 与 P9 单点提交、同股实际受理次序、收据和资源守恒；ADR-0018 仍为 proposed，不能拿其未决建议覆盖现行交易顺序。ADR-0015 保留母单权威状态边界；ADR-0023–0028 后续分别明确虚拟历史/撮合行情、投资者现金池、日终存档、机构个人经历及构建发布等范围，没有取代本批撮合 round、ledger 或竞价语义。开放问题中 Escrow 阶段契约已指向 ADR-0017；未发现三篇材料可直接映射或关闭 `G01–G68`、审计 `Q` 项或正式开放问题的证据。它们是 OOP 归属复核，类型提取不等同于修复独立行为缺口。

没有发现本批候选改变 A 股交易制度、费用、Money/股单位、竞价阶段、T+1 或资金/股份结算语义的证据；该判断是范围核对，不是对交易所现行规则的重新背书。建议实现范围与这些模块的真实共享状态相称，未见额外抽象需求。旧 review 提到的 05 模块说明文字歧义保留为历史记录发现；实现记录已明确单轮与 coordinator 最终校验边界，没有发现其升级为当前行为反证的证据。

结论：三篇历史 review 的主要对象归属与失败/提交边界在 `43b1aa5` 当前实现中再证；03、05 核心 owner/projector/processor 候选已落地，04 的 retain/不增无状态对象建议仍成立。无由这三篇材料新增的产品修复动作。本结论限于本批来源及列明的直接 callers，不等于全量 pipeline 源码审计、全量 G/Q 复核或产品验收。
