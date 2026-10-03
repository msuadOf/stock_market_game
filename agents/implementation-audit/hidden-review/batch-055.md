# 独立审读：批次 055（owner 5）

## 基线与范围

- 目标 worktree：`.worktree/implementation-reaudit`，HEAD `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`，与 scan-plan 的来源基线一致。
- 按 scan-plan 连续全文读取至 EOF 的三份材料：`agents/oop-refactor-audit/exhaustive/reviews/engine-foundation-02.md`（40 行，SHA-256 `d3bb55b7d36bc8743373d140167c5066c822ac5dd3a4f65b228ad87272358e68`）、`agents/oop-refactor-audit/exhaustive/reviews/engine-foundation-03-delta-evidence.md`（29 行，SHA-256 `5a070cb97d0e350e65f3d6ff4cff295b0e25a11221220c3229fefd64829c555b`）、`agents/oop-refactor-audit/exhaustive/reviews/engine-foundation-03.md`（32 行，SHA-256 `22bfe1427caa8a334a3a458d6e62b8244fbb570ef2ea0a3d1d630f0189030ee9`）。三项计划行数与 SHA-256 全部匹配，均为 aliases=1。
- 对照 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`、现行 G 编号总账、最新 ADR-0023–0028，并在基线源码中静态检索相关 owner/caller/consumer。没有运行测试或构建，没有修改产品代码。

## 审读结论

1. **两份 canonical 结论边界清楚。** foundation-02 记录的是 14 个源文件基线复核与四项 delta 证据的合并；foundation-03 记录的是 9 源旧审查及 `orderbook.rs`、`market.rs` 两项增量全文复核。二者均说明自身是结构/材料审计，不代表未读适配层、测试执行或已修复源码风险。delta-evidence 与 canonical 记录的源文件范围、局部非回滚差异及审计限制相互一致，未见材料内部矛盾。
2. **43b1aa5 的 owner/caller/consumer 定位。** `GameSession` 在 `session.rs` 建立各 `Market`，`Market` 持有 `OrderBook`；生产连续撮合由 session pipeline 的 per-stock 执行/流路径消费市场与订单簿，日终竞价 `auction_day_end.rs` 通过受理回执调用 `Market::record_filled_order`。foundation-03 所述 `(price, seq)` 价格时间优先、订单 ID 身份索引，以及订单簿局部修改失败不能一概称事务，仍适用于这些责任边界。`Money` 是 engine 的金额值类型，foundation-03 所述分单位及舍入边界与现行 A 股单位表达不冲突。
3. **需按当前基线收窄一项历史风险。** foundation-03 增量将 `Market::set_last_price` / `set_last_close` 描述为 public 写口；43b1aa5 中对应写入入口为 `fixture_set_last_price` / `fixture_set_last_close`，限定 crate 内测试设施使用，检索到的调用均在测试路径。不能把旧的 public API 暴露结论直接当作当前外部调用方可达缺陷。`record_filled_order` 当前为 `pub(crate)`，生产 consumer 是 `auction_day_end.rs`；其作为身份索引写口、不证明成交真实性的语义仍应准确保留，但不应扩写成外部 public API 风险。OrderBook 部分应用和恢复路径风险仍由既有材料明示；本批只定位调用边界，没有重新阅读全文验证实现行为。
4. **G/Q 与 ADR 对照。** 实现审计总账登记 G01–G68（G27已核销），Q11/Q12 在 `open-questions.md` 中有后续决议，且不存在将这三份 foundation 审查条目直接映射到某个 G 或当前未决 Q 的证据。不得仅因同属 engine 模块而将对象边界审查硬配给 G 编号。相关近年 ADR-0023（合成历史/撮合边界）、0024（现金池）、0025（日终存档）、0026（机构个人经验）及最新 ADR-0027/0028（运行时部署与发布）未改变本批所述对象归属；本批没有新增交易制度主张。

## 三项门禁

1. **大 A 语义：通过（范围有限）。** 记录涉及订单簿优先顺序、金额类型与个人经历状态；没有提出交易制度变化或官方规则新主张。现行规则依据沿用项目 ADR/领域文件，本轮未另行联网核验。
2. **必要性与范围：通过。** 三份材料只归档既有对象与增量复核。本批没有据此提出 OOP 迁移或源码修改要求；仅指出基线中旧 setter 可见性表述需要在后续引用时收窄。
3. **遗漏、漂移与复杂度：有一项版本漂移需留意。** `set_last_price` / `set_last_close` 从旧材料所述 public 写口变为基线中的 crate-private fixture setter；`record_filled_order` 亦为 crate-private，并有明确生产消费者。订单簿非事务路径、Market 恢复/索引契约等其余风险没有在本批重新行为验证。未发现应新增的 G/Q 映射，也未运行边界测试。

## 限制

本批的“当前 caller/owner/consumer”是基于 43b1aa5 的生产源码检索与定位，不是对 engine 全部调用链逐文件全文复审。原三份审查报告所说的旧 baseline 与其复核范围保留为历史证据；本记录不将其冒称为对全部源文件的当前完整复审，也不代表风险已经修复或产品验收通过。
