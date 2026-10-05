# Simple 信息接线独立最终复核

日期：2026-10-06。复核者未参与本批实现。范围为 `packages/engine/src/information/{mod.rs,publication.rs,public_view.rs,simple_disclosures.rs,source_tests.rs}`、`packages/engine/src/company/query.rs` 和 `agents/company-system/information-source-red-harness.rs`；按 HEAD 合并检查 staged 与 unstaged 改动，不把工作区其他作者改动计入本结论。

依据：`AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`、ADR-0035、`agents/company-system/information-contract.md`、`agents/remaining-questions-and-features/q14-financial-model-design.md` 全文。初次静态复核未运行 Cargo 或测试。实施者随后提供 fresh host60 记录 `.tmp/checklist-wave4/host60-information.log`：source7 共 7 项通过、0 失败，耗时 0.00 秒；记录覆盖 source 测试集合，其中包含下述新增的成功发布短测。

## 结论

- 领域语义符合用户最新决定：`SimpleGenerated` 仅标识账面生成来源；发布仍要求 `ClosingEngine` 中已有的完整、可校验 `ReportSet`，不构造少量指标或伪造五表。DTO 继续投影共同 accounting/financials 结构，金额仍使用会计元字符串。
- 来源是必填的序列化字段，公开前史和 query DTO 都承载同一 `PublicationSource`；更正继承原公开来源，恢复逐条验证，缺少字段的旧存档会拒绝，符合不增加 schema 兼容的决议。
- 排期继续经既有 `ReportFrequency`：月报 custom/preset delay 由 `MonthlyReportSchedule` 承载并校验，季度、半年和年度沿既定 schedule。`seeded` 只描述来源阶段；没有发现按公司模式取消排期或提前读取未来报告的路径。本人获知仍由现有信息登记机制控制，本次没有改成“公开即所有 NPC 获知”。
- 更正公开链校验报告 ID、公司、范围、期间、种类、来源、版本单调性、私有前驱合法性和公开时间顺序。私有 `ReportVersion.supersedes` 可大于公开旧报告私有版本序号，只要求不小于被更正公开报告版本且小于当前版本；这保留了中间未公开私有修订的合法情形。
- 插入在校验和 digest 计算后再修改库；`publish_simple_scheduled` 对候选库批量发布，成功后整体替换，因此失败不会留下部分公开结果。未见交易制度或 A 股规则被改动；本功能是来源标记与披露契约，不需要新增交易规则依据。

## 发现

- 初次复核提出的成功路径测试已补齐：`simple_monthly_disclosure_obeys_public_time_and_repeated_scans_are_idempotent` 使用一个月的真实前史，断言公布前查询不可见、到期时按 `18:00` 发布、`SimpleGenerated` 来源、完整月报、重复扫描不重复创建，且逆序区间失败不改变库状态。提供的 host60 日志显示该用例及其余 6 项 source 测试全部通过。

## 完成门禁

## 范围限制

本次签核只覆盖月度披露基础契约、来源标记、共用完整 `ReportSet`、本人获知边界、排期时点和本报告列明的更正校验。它不签核年度／任意结算期间的完整报告供给：基本面结算 cadence 与 `ReportFrequency` 之间，当所需报告期间尚未由核心公司层定稿登记时，期间数据可用性仍须由 core 接续解决；本次 helper 只发布实际已登记且关闭的报告，不伪造缺失期间的五表、不均分长周期金额，也不取消既有报告要求。

当前限定范围复核通过，未发现需要修复的有效 finding。host60 日志是实施者提供并由复核者读取的 fresh 验收证据；没有运行更广泛的 Cargo／index 验收。本结论不代表 `Simple` 全部期间披露、整个公司系统或完整仿真已完成。未对代码作修改。
