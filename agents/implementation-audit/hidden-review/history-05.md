# 历史来源补扫 12–14

基线：来源计划 `source_baseline=43b1aa5`；三篇均读取旧分支 `c139a69d2a09220e349912376d4be83d8859795f` 上的完整版本并到达 EOF。SHA-256、行数及 EOF 标记见同目录 `history-05.json`。历史记录只证明当时的实现状态、经验或计划审查，不自动成为现行需求。

## 来源 12：resolve-blockers-wayland/learnings.md

全文记录了 Worker/Server/Tauri parity、K7 provenance 与资源政策迭代、跨年计划生命周期修复、历史性能调查，以及 FIFO 写通道设计研究。早期记录中“没有安全的额外并行 seam”等结论后来被明确限定；2026-09-17 的独立 profile 又报告了特定短 fixture 下的观测值，但同时声明样本有限、子阶段计时嵌套、仅主线程和并未证明通用收益。随后关于按股票单独 FIFO、协调账户/计划资源的设计建议只是探索草案。其“用户批准的 governing semantic”不等于产品 ADR 或实现授权。

## 来源 13：resolve-blockers-wayland/problems.md

记录旧版 Worker reload、跨宿主矩阵、K7 长矩阵、Todo-8 发布契约等阶段性阻塞。内容本身随时间有“已解决”“继续执行”“证据不足”的状态变化；不能把旧 blocker 标题直接当成基线现状。末尾 Todo-7 V6 矩阵执行阻塞仍是当时的证据状态，不能从该历史笔记推出当前代码故障。

## 来源 14：escrow-parallel-engine-r24.md

这是计划审查回执：它批准当时 r24 计划的具体摘要和依赖顺序，并明确批准不代表产品实现、测试、性能或 K7 证据完成。后续实现与 ADR 修订需优先于该计划阶段状态。

## 按当前决策、总账与调用链复核

- **当前约束：** ADR-0018 §7 仍为 proposed，但其中后续明确接受的具体方向和实际生产接线已由总账作为现行基线核对。它将受理与业务先后限制在同股价时规则、同账户实际资源争用及计划依赖；无关账户/股票不因全局 FIFO 或输出身份排序串行。ADR-0017 的后续修订也明确不采用常驻跨实体 channel/actor 权威状态。历史 FIFO 草案因此属于未来方案探索，不能反向设为缺口。
- **真实消费者：** `43b1aa5` 的 `session/pipeline/authoritative_tick.rs::execute_authoritative_tick` 是连续/集合竞价/PreOpen 的生产分派入口；连续事务在 `continuous_tick_transaction.rs` 捕获入口、并行准备后由 `ReadyStockStream` 与 `stock_stream.rs` 按股票工作推进计划依赖及股票 shard。并发完成通知用于唤醒和传递私有结果，不决定业务优先级。该链路已经实现当前计划的核心阶段，不因旧笔记建议新增另一套通道或 coordinator。
- **A 股边界：** FIFO 笔记关于共享账户资金、可卖股份、T+1、价格时间优先及计划状态相互影响的风险仍是有效实现约束；但建议模型不能替换现行撮合规则。当前审计总账已覆盖资金/股份守恒、局部价时规则、受理事实与失败原子边界；本次未发现新的制度差异或新 caller 违背这些约束的证据。
- **性能记录：** 历史 microstructure profile 提议并行 immutable forward searches，并提出性能目标，是研究建议而非已接受产品要求。到 `43b1aa5`，`diagnostics/causal/aggregate.rs` 仍在报告构造中调用 `microstructure::analyze`，但实现已变为线性顺序 accumulator；原文所述多次未来扫描不再是当前实现。订单簿竞价候选并行也曾因样本计时极低而被原记录否决。ADR-0020 仍为 proposed，且历史短样本不能证明当前端到端瓶颈。故这些不是新增 G/Q；未来若开展性能优化，应先测量并按当时授权与测试契约评估。
- **去重与状态：** 总账 R02/R15 已记录 escrow pipeline 与生产并行入口的实现，G39 已记录自由并发运行不得以整局 stdout SHA 代替固定受理事实重放；resolution 也明确排除“100% 相同输出/恢复旧全序”的旧要求。旧问题簿的 K7 after/sensitivity 执行与验收证据边界已在总账列明，本次不复用旧 blocker 另编编号。旧 review receipt 只作历史批准证据，不新增承诺。

结论：三篇未产生可与总账 G/Q 合并的新缺口，也没有需要新增候选。旧性能测量只保留为受限历史观察；FIFO actor/channel 草案归为已取代的未来设计探索；阶段性运行 blocker 与 r24 计划状态按总账和现行 ADR 核销。本次只做静态审阅，没有改产品代码或运行测试。
