# 独立低层恢复复核

- 日期：2026-10-06
- 范围：`CompanyOperations` 独立恢复、指定 Session 存档测试迁移及公司 collection 测试；未审查并行改动的其他文件。
- 依据：`AGENTS.md`、`docs/principles.md`、ADR-0035、ADR-0036。

## 结论

本范围的恢复拆分方向符合 ADR-0035：独立经营状态可反序列化并校验，Session 仍通过 `CompanySystem` 校验配置和发行人到股票映射；无需保留旧 `FullFinancial Session` 作为验证容器。范围内没有增加 schema 代际、兼容或默认字段，也未见改变人民币分、股数或 A 股交易制度的代码。最终静态审查未发现本 scope 的有效问题。

迁移后的测试继续覆盖保险子账损坏及公司/合同/科目上下文、银行 ECL 表非法值、工业贷款负数与汇总溢出、独立 scheduler 重复 due ID；Simple Session 测试保留 plan horizon 校验，并继续检查 clock 重复 ID、策略外未来 due 及合法 clock 事实恢复。规模用例覆盖 257 家 Simple 公司反序列化、重复上市股票拒绝和发行人股数与股票配置不匹配；新财务生成配置以批准的 `simple_company_fixture` 完整字段模板构造公司，并将变化限定于本用例的公司 ID 与 `prehistory_months = 1`。

委托方提供并要求核读的 `.tmp/checklist-wave4/host60-financial-restore-green.log` 与 `.tmp/checklist-wave4/host60-restore.log` 均记录 1 passed、0 failed、0.01s：前者覆盖 Bank 四类非法 ECL policy，后者覆盖 Insurance 损坏状态的 company、GROUP、GMM 上下文。该证据仅证明这两项 owner 级恢复 case 通过，不扩展为贷款、scheduler、规模用例或全量回归通过；这些待集中验收。此前 `.tmp/checklist-wave4/host58-financial-restore-red.log` 是实现前红测证据。

## 需处理发现

1. 初审时曾将 `core.rs` 内的 `OperatingReportCorrection` 与两套报告更正 API 误判为本批新增。委托方澄清：它们在本次任务开工前已存在，属于 Q23/平行金融工作。本次低层 restore 只改 `CompanyOperations` 的 serde derive/cache 可见性，不将这些既存 API 归为本批发现，也不要求拆出；它们仍属独立 scope，应由其对应审查覆盖。
2. 本次银行策略迁移把完整 Session 路径上的非法 ECL 拒绝保证移至 `CompanyOperations` 自定义 Deserialize 校验；测试仍断言四种损坏形态均拒绝且带公司上下文。host60 指定日志验证通过。独立反序列化即时校验，无静默 fallback。

## 限制

未运行 Cargo，未检查低层文件以外的其余 diff。贷款、scheduler、规模用例及完整回归尚待 root 汇总验证；本记录只确认两个 host60 owner 级恢复 case 的日志结果，不宣称本批完整验收通过。审查期间工作区含有大量并行更改。
