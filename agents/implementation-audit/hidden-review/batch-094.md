# Batch 094 独立复核

## 范围与来源完整性

按 `scan-plan.json` 的 id 94、owner 4 执行。本次仅写此复核记录，没有修改产品源码，没有运行测试或执行 Git 写操作。三份指定来源均全文读取至 EOF；计划所列行数及 SHA-256 与实文件一致，逐源记录见 `batch-094.json`。

已阅读主仓 `AGENTS.md`、`docs/principles.md`、ADR-0017、ADR-0018（状态仍为 proposed，明确接受的具体目标按后续决定分别处理）及 `docs/trading-rules.md`。产品基线 `/data1/baiyifan/workplace/stock_market_game/.worktree/implementation-reaudit` 的 HEAD 为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`，与计划基线一致。

## 三份材料复核

- `engine-pipeline-08.md` 是中文化差异的独立复核记录，不是重新执行原始语义审计。原审阅记录的关键限定成立：`AccountReceipts` / `admit_ready_batch` 的职责边界与当前 `local_admission.rs` 一致；依赖检查验证候选唯一、前驱存在且在替换前、同账户同股以及 Cancel→Place，不校验 Side。账户 receipt ordinal 先递增后续验证失败时不回滚，当前材料仍明确要求沿 caller 核实该状态是否随失败 tick 丢弃。`EnvelopeLedger` 已拥有现存 transition 方法；public clone/swap 与 private mutation-before-Err 的差别在代码中仍存在，private 调用方承担候选整体丢弃责任。没有发现材料的通过结论掩盖上述边界；也不能把 caller 原子性说成已证明。
- `engine-pipeline-09.md` 准确限定为 `pre_open_transaction.rs` / NPC lifecycle 测试计数的 delta，不冒称重审该 pipeline 全模块。当前 `PreparedPreOpenTick::commit`、`PreOpenTickResult::into_commit` 及 `apply_tick_shadow_pre_open_transaction_inner` 与记录一致。此函数 `take` 掉局部 `decision_resources` 与 session 后若失败不会在本函数恢复；成功后由 P9 commit 发布。错误后候选可复用性取决于外围丢弃，不能声称 authority 已提交或局部可重试。此 delta 记录的 7 项测试计数是静态源文件计数，不是执行结果。
- `engine-pipeline-10.md` 清楚注明 A01 只是候选记录复核、未实施。当前机构投影调用链从 `prepare_settlement_transaction_with_beliefs` 到 `project_institutional_experience` 传入同一个必需 `ExperienceMoment`；现有投影接口不再是可空 moment，因此 A01 中把 `Option<ExperienceMoment>` 与私有 enum 组合改成单一必需 payload 的类型改造已被当前实现吸收/取代。材料没有把候选误报成尚未完成的代码改造。此调用链改变实际成交后的机构 experience 派生，不改变撮合、费用、资金、持仓或 A 股制度。

## 基线调用链与旧结论检查

在基线源码定位并读取了 `local_admission.rs`、`ledger.rs`、`pre_open_transaction.rs`、`account_settlement.rs` 与 `institutional_experience_projection.rs` 的对应 owner、caller 和 consumer。当前职责证据支持三份源文件引用的窄结论；其中 08 的跨层失败丢弃仍是未证边界，09 的候选 take 后失败仍须依赖调用边界丢弃，10 的 A01 Option 类型候选则已不适用于当前接口。未发现当前实现反证其余明确描述。

逐项对照现行缺口总账 G01–G68、其余分章、Q 项及裁定记录：这三篇仅讨论 engine pipeline 所有权、候选失败局部状态、机构 experience 输入类型和中文版本对应；与总账 G 项没有同一缺口映射，也没有发现会核销或重开某个 G 项的证据。ADR-0017 / `trading-rules.md` 给出托管账本与现行交易规则的边界，ADR-0018 §7 的已采纳局部冲突顺序优先于 ADR-0017 被取代的历史来源类序；本批材料没有声称改动这些规则。Q13 在 ADR-0017 关联范围内；这三篇记录不提供 Q13 新结论，也没有证据应将其另行归入 Q 项。未重新查询交易所或中国结算规则，不对官方规则作新的断言。

## 门禁结论

1. **大 A 语义：** 复核对象没有引入交易制度主张；账本是游戏内资源预留与审计模型。Side 资源 lane 不应误读为交易所买卖优先规则。沿用现行 `trading-rules.md` 与 ADR-0017/0018 的已记录边界，本轮未查官方来源。
2. **必要性与最小范围：** 复核属于中文化/版本映射记录和已有候选结论核对；没有理由扩张产品改动。三篇文档标明其审阅范围及不覆盖的重审内容，范围表达恰当。
3. **边界测试、跨层漂移与复杂度：** 已披露的两类失败丢弃责任仍需 caller 级证据；候选局部 take 错误不可写成 authority rollback 已证。当前 G/Q 总账未覆盖这些具体边界并不构成漏登记的已确认产品缺口。未运行测试。

**结论：** 指定材料的中文化复核与范围表述可接受。保留 08/09 的 caller 生命周期未证限制；将 10 的 A01 Option 类型候选视为已被现行必需 moment 接口取代，而非当前待实施项。本批没有发现需新增或修正 G/Q 总账结论的证据。
