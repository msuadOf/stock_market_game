# 批次 061：engine pipeline 历史复核记录审读

- 基线：`.worktree/implementation-reaudit`，HEAD `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。
- 计划：`agents/implementation-audit/hidden-review/scan-plan.json` 的 batch 61，owner 1；三篇来源的路径、SHA-256 和行数均与计划一致。
- 阅读证据：三篇来源各自从首行连续读至 EOF；行数依次为 29、40、34。另读 worktree 根 `AGENTS.md` 与 `docs/principles.md`（92 行，至 EOF）。未运行测试/构建，未修改产品文件，未执行 Git 写操作。

## 来源章节族与旧结论

1. `agents/oop-refactor-audit/exhaustive/reviews/engine-pipeline-10-tests-full.md`（29 行）：A01 settlement 测试全文复核；章节族为绑定材料与阅读证据、A01 覆盖审计、三项复核门禁。旧结论准确限定了测试行为、费用历史与原子性覆盖，且明确不主张覆盖全部 A 股费用规则，也未虚构私有 enum 非法状态的测试。
2. `agents/oop-refactor-audit/exhaustive/reviews/engine-pipeline-11-auction-full.md`（40 行）：`stock_auction.rs` 全文独立复核；章节族为全文与归属核对、交易语义、三项门禁、SHA 清单。此前对 `day_end_release_receipt` 归属的遗漏已在该文所述当前条目中修订；不新增对象的结论与代码仍一致。沪深竞价差异及深市简化没有被写成官方规则重新核验。
3. `agents/oop-refactor-audit/exhaustive/reviews/engine-pipeline-11-final.md`（34 行）：unit024/045 短 delta；章节族为范围/继承依据、Delta 核对、三项门禁、审计记录 SHA。它明确继承旧完整复核而不冒称重审全部 11 个文件；payload/notification 关闭语义与测试计数描述相互一致。

## 43b1aa5 产品路径与候选状态

- **10-A01 `ExperienceUpdateMode`：已落地，不是待修复缺口。** `packages/engine/src/session/pipeline/retail_projection.rs:322-344` 的两个入口分别构造 `Retail` 与 `InstitutionalFacts(ExperienceMoment)`，模式内携带机构分支必需时间。机构路径由 `account_settlement.rs:84-100,196-205` 和 `institutional_experience_projection.rs:9-54` 传入；机构成交分支在 `retail_projection.rs:529-535` 消费该 moment 并汇总收据实际费用。测试来源只证明其记载范围，不证明所有 A 股收费政策。
- **不要把 A01 的类型整理等同于核销 G08。** 现行总账 `agents/implementation-audit/implementation-audit-2026-10-02.md:51` 仍记录 G08 的散户 dated writer/衰减调用链缺口。这里的 institutional projection 不补齐散户链，不能映射或核销该项。
- **11-A01 `StockStreamCoordinator<S>`：在基线产品中已存在。** `stock_stream.rs:271-287,292-312` 显示 `drive_stock_stream` 构造私有 per-tick coordinator；实现生命周期方法位于该文件后续 impl。生产 caller 包括 `continuous_tick_transaction.rs:227-233`、`auction_tick_transaction.rs:210-216`、`pre_open_transaction.rs:236-242`。旧结论关于调度 owner、payload/notification 区分以及不持有账户/结算/session authority 仍与当前入口相符。不能因旧记录把它作为“候选”就当成当前待实施功能。
- **`stock_auction.rs`：保留局部领域边界，无须再抽象。** `AuctionPhase` 和 `StockAuctionState` 位于 `stock_auction.rs:17-37,91-113`；清算/收据局部 helper 仍由当前 caller 使用，`day_end_release_receipt` 位于 `:557-567`，例如 `auction_day_end.rs:1451` 调用。复核材料未提出额外必须新增的 owner/对象，OOP 提取本身不是正确性修复。

## 编号、规则与新候选反证

- 三篇文档没有建立到 G01–G68 或未决 Q 的直接产品需求映射。G 总账只作为范围核对；G08 是上述相邻但独立的散户缺口。Q11 的机构个人经验补充已由 ADR-0026 定义，Q12 已由 ADR-0024 核销；不从旧 Q 描述恢复未决状态。
- 相关契约以 ADR-0017/0018 为准：临时并行 tick、stock-owned shadow/outbox 及局部续行约束支撑现有 stream 边界；ADR-0018 已明确更新 ADR-0017 的特定观察/顺序语义，应避免引用其被取代部分。ADR-0023–0028 分别更新合成历史、资金池、日终持久化、机构个人经验和部署发布等边界，没有要求改变本批撮合或竞价实现，也没有反证出新 OOP 修复。
- 大 A 语义：本批代码仍使用既有沪深竞价选择、撤单窗口及收据结算约定；来源记录对深市取舍明确称为游戏简化。未重新核对官方规则，不作新的法源背书。当前未发现跨层交易语义漂移或漏报必须测试的候选。

## 结论

三篇指定记录的旧事实在 `43b1aa5` 当前调用链下再证。10-A01 与 11-A01 已在产品实现，stock auction 记录不要求新增抽取；不存在由这三篇记录新推出的修复动作。文档审读通过，不能外推为全部 pipeline 源码复审、G01–G68 全量复核或产品验收。
