# 报告期间可用性独立复核

日期：2026-10-06

## 范围与结论

本次只复核新增 `PublicReportAvailabilityQuery`、`query_report_availability`、`CompanySystem::report_availability`、`GameSession` facade 和对应的 `source_tests`。工作区同时有大量其他未提交改动，未将其归入本次功能结论；此前 `PublicationSource` 的来源字段不在本次复核范围。

整体设计符合已读 ADR-0035 与信息契约：先按 `as_of` 查询已公开报告，且要求公司、报告期间、报告种类及 `scope` 精确匹配；不可用分支只返回原因，不泄漏内部报表。长期结算周期不会被摊成短报告。查询 DTO 校验自然月末、报告种类落点、公司身份及 `scope` 镜像关系。此查询不修改撮合、证券类别或 A 股结算规则，因此没有引入新的交易语义。

## 复核更正

初读 `opening_date` 时将其误解为游戏开局日。继续追踪 `SimpleFundamentals::create` 后确认，它设为首个生成周期开始日前一日的汇总期初日（`baseline_end`），不是游戏开局日。`SimpleFinanceState::validate` 又要求实际生成期间从 `opening_date.next()` 连续开始。故 `first_date <= opening_date` 对应查询窗口覆盖尚未生成的期初基准日；允许用该账面起点伪称一段完整期间材料会制造部分历史。首个可完整生成的自然月首日严格晚于 `opening_date`，应继续分类为尚未结算或未代表，不是 `BeforeOpening`。原发现撤回，无需改变比较符。

本结论据当前 `state.rs` 的真实构造和 `finance_validation.rs` 连续期间校验；实施者计划新增首期月可用性短测，测试结果待 fresh 验证。

## 其余核验

- `reports_for_company(company, as_of)` 过滤 `published_at <= as_of`；`Available` 只携带该公开材料的 DTO，不读取 `CompanySystem` 私有财务值。
- 缺少报告时，`NotScheduled`、`NotYetSettled`、`BeforeOpening`、`PeriodNotRepresented`、`NotYetPublished`、`ScopeNotRepresented` 均不附带财务字段或零值。
- 三个新增测试覆盖已公开材料可用及历史时点不可见、年度生成周期不制造月报、未来/更早开局前/排期/范围原因和输入拒绝。首期月边界短测可进一步固定构造契约，但当前代码边界与期初语义一致。
- 可用性查询是纯读取，不存在按请求重复结算或副作用性能风险；扫描公开报告沿用现有公司报告列表查询。

## 验证状态

限定范围签核：查询实现、DTO、Session facade、相关 source guard 与测试已复核；`opening_date` 误读已撤回。`host64-short-tests.log` 中可辨认出 `information::source_tests` 的实际结果为 10 passed、0 failed、0 ignored，耗时 0.02 秒，且含公开时点/未发布报告、长周期不伪造短报告、坏日期与 scope 拒绝等用例。该日志同时交错输出其他并行套件，本签核只采用明确标为 10-case information 的结果，不扩展为公司系统或股本功能完整验收，也不引用 host63 红阶段作为通过证据。
