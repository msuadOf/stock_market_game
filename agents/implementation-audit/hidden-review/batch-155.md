# 批次 155 独立复核

## 结论

三份指定来源都标明“候选设计，未实施”，讨论的是现存职责归属与对象化建议，并没有证据表明这些提议曾被批准为生产交付承诺。因此，不因其 `retain` / `support` 盘点或提议未实施而登记承诺遗漏。按当前 `43b1aa5` 基线定向追踪 caller、owner 与 consumer 后，大部分保留结论仍成立；本批确认一个来源中的缺陷判断应改为“未证实”，并确认一个跨恢复边界候选已由更新的正式审计明确保留。

- **D01：不接受其缺陷结论。** 来源称 `EclPolicy::validate` 必须拒绝 `version=0`，但没有给出 `version >= 1` 的正式契约依据。当前 [`ecl.rs`](../../../packages/engine/src/company/bank/ecl.rs) 的 `EclPolicy::validate` 校验两张情景表非空、单项范围及权重合计；`BankBooks::new` 在过账前调用该验证。`version` 的存在本身不足以证明 0 非法。来源建议新增版本下限测试不是已批准契约，不能据此作为生产缺陷或变更依据。
- **ECL 恢复校验：保留为独立候选，不归入 D01。** `BankBooks` 可由 serde 直接恢复，不经过 `new`；`issue_loan` 调用初始 ECL 计量，而该路径按现有契约不重验恢复的政策。最新审计 [`luna52.md`](../exhaustive-review/luna52.md) C02 已沿正式存档恢复路径精确确认空表/权重和错误的 ECL policy 未被 `validate_company_domain` 校验。它是跨恢复边界的独立候选，不是“版本 0”缺陷，也不要求每次发放贷款重复校验。不得用本批历史来源或旧保护测试将 C02 核销。
- **D02：代码路径存在，标准发布路径可达性受限。** [`notes.rs`](../../../packages/engine/src/accounting/reports/notes.rs) 的 `build_notes` 确有分类缺失时回退 `PaidInCapital` 的分支；正常 `generate_report_set` 先构建并校验分类，拆分事实来自账簿成员，因此来源将其限定为内部输入一致性边界是审慎的。本批未构造或证明该缺项可由标准发布路径触发；不将其升级为已确认发布缺陷，也不因 OOP 提议要求额外对象化。
- **授信和经营模块：保留既有 G 边界。** `ContractBook::outstanding_borrowings` 按贷款人和合同本金求和；公司登记合同入口会做对手方及授信校验。正式总账已将 `IndustrialBooks::borrow/available_credit` 跨贷款人余额问题记为 G58，不能由合同登记簿既有方法存在来核销，也不在本批重复开项。`CompanyOperations::advance_civil_day` 是日推进 owner，按行业配对分派流；未调用的 `impairment_signal_bp` 与地产减值入口仍是“当前无该经营 caller”的明示边界，不代表 OOP 漏迁。
- **无 A 股规则变化。** 三份材料讨论的是公司会计、经营状态、报告投影与 RNG 结构；不提出新的证券交易制度。金额分、经营单位与证券股份的语义保持分离。ADR-0024 继续规定投资者资金池可减少且公司经营资金不转入投资者账户；ADR-0026 的机构行为方向与这些经营 OOP 候选无关。Q1–Q12 当前状态见 `docs/open-questions.md`；没有由这些候选产生的新待决 Q。

来源中的报表纯投影、`BankBooks` / `RealEstateBooks` 账套 owner、`CompanyOperations` 日推进协调、错误类型和序列化 DTO 保留判断均与当前 caller/consumer 结构相符。当前业务缺口仍按总账已有精确范围处理（包括 G28、G35、G36、G58）；不从“owner 已存在”推导相关产品范围完成。

## 来源校验

按冻结的 `scan-plan.json` batch 155，三份来源均连续读取至 EOF。实测行数及 SHA-256 与计划一致，逐项凭证见配套 JSON。产品基线 `HEAD` 为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`，与计划 `source_baseline` 相同。已读 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`，并核对 ADR-0016、ADR-0024、ADR-0026、G/Q 总账、ECL 恢复审计及上述直接调用路径。

本批只作定向静态复核，不是穷尽全仓调用图或交易所规则重新取证；未运行测试/构建，未修改产品源码、测试、G/Q 总账或 Git 状态。仅新增本 Markdown 与配套 JSON。
