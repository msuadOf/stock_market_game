# 隐藏扫描批次 137（owner 2）

## 来源与完整性

- 按 caller 的 `scan-plan.json`，本批 owner=2，来源基线 `43b1aa5`。逐篇从主工作区连续读取三份指定材料至 EOF；实测路径、行数与 SHA-256 均与计划一致，详见配套 `batch-137.json`。
- 已阅读 caller 的 `AGENTS.md`、`docs/principles.md`、`docs/architecture.md`、`docs/open-questions.md`，并核对当前 implementation audit 总账、相关 ADR 与既有候选裁定。仅作历史材料复核；未运行测试/构建、未执行 Git 写操作、未改产品代码或 G/Q。

## 来源结论与当前核对

- `reviews-engine-company-04.md` 是当时对 29 个公司/地产文件的 OOP 复核，结论为 retain/support，并确认其清单、owner、调用与既有覆盖相符。它没有提出行为改动，也未将公司会计简化冒充现行准则。此历史对象边界结论不单独证明当前完整经营闭环；总账 G35/G36 等现行缺口仍依其生产调用链维持。
- `reviews-engine-company-05.md` 记载 `OperatingScheduler` 与 `CompanySpec` 的 retain 判断，并明确列出 scheduler 恢复重复 `ScheduledDueId` 与 `next_seq` 耗尽为独立、未实施的缺陷线索。caller 的 `batch-037.md` 已按当前 scheduler、SaveSlot/恢复 caller 与 clock consumer 复核这两项；`candidate-resolution-01.md` 也将它们保留为独立恢复边界候选，未映射既有 G/Q。本批不重复升级或修改台账，也不将自然日经营队列混称为证券委托队列。
- `reviews-engine-foundation-01.md` 记载 Account 封装迁移、日历策略摘要、费用注释及对象归属等历史复核。caller 的 `batch-001.md` 与 `batch-038.md` 已依当前代码确认 A03 已实施，Account 替换策略仍经受控写入口，AccountBook 缓存失效约束保留；故不把旧 A03 提案重开。其余摘要完整性与注释线索仍需按现行消费边界理解：历史材料本身不构成已批准需求或故障复现，caller 总账没有相应已批准 G/ADR 修复承诺，不据此新增 G/Q。来源提及过户费和金额单位时，本复核未重新核验官方费率依据；不改变金额为分、数量为股及既有沪深交易语义。

## G/Q、决策与范围

- 对照当前总账 G01–G68、Q01–Q23、`batch-037.md`、`batch-038.md`、`batch-001.md` 及 `candidate-resolution-01.md`：本批材料没有提供新的已批准承诺遗漏或旧 G/Q 错误核销证据。scheduler 两候选已在现行候选裁定中登记，A03 已由当前实现核实；其他对象保留意见不替代总账所列生产 caller 缺口。
- `docs/principles.md` 的显式校验/TDD原则、`docs/architecture.md` 的 engine/外壳边界与现行开放问题、ADR-0016/0019/0023/0024 的公司披露范围、存档编辑、虚拟历史和投资者资金边界未因这些历史 OOP 材料改变。未发现本批要求变更证券交易制度；没有重新联网核验交易所规则。
- 结论限于本批三篇来源及上述 caller/总账交叉核验；没有宣称历史建议已实现、所有相关模块已完整复审或测试已通过。无需改动 G/Q。
