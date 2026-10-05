# Simple Finance 实施记录

## 范围与依据

- 依据 `../remaining-questions-and-features/q14-financial-model-design.md` 的当前已确认设计；Simple 仅做账面展示，没有真实客户、付款网络或 SyntheticFunding。
- 收入以汇总 AR 对收入、固定和变动费用以费用对汇总 AP 入账；摘要事件必须是 `NonCash`。零 Cash Flow 是合法结果，不是假造现金来源，也不是 unsupported。
- 复用共同 `Books`、`ClosingEngine`、`ReportSet`、`IncomeTaxPosition` 与税务 policy。每个真实生成期间按 YTD 目标税额差额计提，不另算净利润率。
- Finance 接受连续完整、同一自然年度内的生成期间，不将季度总量均分为月度。仅在 `recognized_periods` 能完整拼接覆盖时登记对应报告。
- Simple 账面现金不在此模块充当真实分红预算；股份、投资者现金、真实订单与公司行为分别由对应 owner 负责。本次没有改动 A 股撮合或交收制度。

## 实现

- 显式期初配置与汇总规则；防止期初藏入收入或费用。
- 候选 clone 上原子应用摘要、共享税务、封期与报告登记；失败不提交。
- 恢复校验税务 Owner 与账簿、重述工作底稿、时间范围、事件序号及期间覆盖。
- 原子 correction 安装接受同一候选 Books、Closing、TaxPosition 与事件游标，校验通过后提交。
- `ClosingEngine::PartialEq` 比较真实报告版本与重述底稿，忽略派生 `OnceLock` cache。
- 最终公共金额 DTO 统一为 `company::api::PeriodAmounts`，摘要业务 kind 统一为 `SimplePeriodSummary`；不保留 Monthly 旧名的兼容 alias，历史测试日志保持原始证据。

## TDD 与当前状态

- root 的 `host58-finance-red.log` 已记录初始真实 4 红；红后实施摘要过账与共享税务。
- 月度测试的所得税断言根据已确认“每期 YTD 差额”语义调整，净利润为 2250 分而非免税的 3000 分；不是为测试变绿弱化断言。
- 补充真实季度不冒充 March Monthly、合法零摘要无零金额凭证、未来税年份及重用事件游标恢复拒绝测试。
- 独立 reviewer 的 `simple-finance-review.md` 暂不批准：恢复的 Closing latest 尚未与权威 Books 绑定。
- 已新增不同 Books 的真实合法 ReportSet、foreign Scope、超出 recognized coverage 三个污染恢复用例；root 的 `host60-finance-guards-red.log` 记录实际 7 绿、3 红，耗时 0.03 秒。
- 红后新增只读 `ClosingEngine::report_versions()` 与 Finance 报告 Owner 校验。拒绝其他 Scope 或覆盖范围、历史版本结构及链错误；最新版本由当前权威 Books 与重述底稿精确重建比较。历史版本不重写，合法 Original 后续私有版本仍允许没有 supersedes。
- reviewer 再发现删除整组已生成报告键会静默 unavailable。第 11 个缺报告恢复测试在 `host62-finance.log` 实际得到 10 绿、1 红，耗时 0.09 秒；本 agent 已读取日志。
- 该红后补必需报告键存在校验：按每个 `recognized_periods` 末日、同一报告端点规则与 `covers` 推导必须存在的登记；不禁止其他已通过权威账簿重建校验的合法键。
- root 的 `host60-finance.log` 已记录原有 7 个 Finance case 全绿，实际执行耗时 0.03 秒；`host62-finance.log` 进一步证明 3 个污染用例已真实绿。缺报告修复后 11 个用例绿结果待 root 统一验证。
- root 的 `host63-finance.log` 已记录实际 Finance 11 个用例全部通过，耗时 0.09 秒；本 agent 已读取日志，两轮恢复污染 TDD 红绿闭环完成。
- host63 时 Finance 基础用例已绿，Simple 生产更正仍由 Q17 owner 从 direct Closing + reconcile 路径切到共享 `prepare_correction` 与 `install_correction`；不得把基础通过宣称为更正集成完成。编译与测试由 root 统一执行，本 agent 未自行启动 Cargo、rustc 或测试二进制。
- Q17 owner 已将生产更正接到共享 `prepare_correction`，候选 Books、Closing、税务 Owner 和 library 同时安装；已删除无调用者的 `parts_mut` 与 `reconcile_after_correction` 逃逸入口。该接线动态结果仍由 Q17 owner/root 验证。
- 开账基准跨层校验补充：`host65-baseline-red.log` 实际显示内部 Finance 合法、但开账日早于 `history_start.prev()` 的存档被放过，期望恢复拒绝断言失败。红后仅增加 `SimpleFundamentals::validate` 的开账日链接 guard，与 Web 严格 schema 和唯一 create baseline 一致；待 root fresh 验证。
- `host66-baseline-q17-green.log` 中 Core 16/16 实际通过、耗时 0.05 秒，包含该基准错配拒绝 case；本 agent 已读取对应行，开账基准 TDD 闭环完成。同日志 Q17 为 6/7，不能据此宣称 Q17 集成通过。

## 已知所得税简化

- 现有共享 `accounting/tax.rs` 将 DTA 定义为未用亏损池乘以税率，明确标注“全额确认简化”，不含未来应税利润很可能足够的判断参数。
- Finance 不另造该参数，也不把共享简化宣称为完整 CAS 18。永久亏损情景仍可能生成 DTA，属于共享 policy 已知边界；严格可回收性政策需要单独领域决策并迁移所有 Owner。

## 四种 CompanyKind 的汇总账套

- checkpoint 发现 P1：旧 Simple 固定 Industrial 账套和列报，却允许 Bank、Insurance、RealEstate 身份，形成静默错列报。
- 用户设计仍要求四种 CompanyKind 共同财务功能；root 明确不采用拒绝其他 kind 的收窄方案。四种类型均使用对应基础 chart 与 IndustryPresentation，但不运行 Simulation 行业经营。
- `SimpleCompanyConfig.kind`、发行人 `CompanySpec.kind` 和 `SimpleFinanceState.kind` 都是必需事实且须一致；不兼容缺字段，也不从证券代码推断 kind。
- 新增 Simple 专属 `simple_receivable`、`simple_payable`、`simple_revenue`、`simple_fixed_expense`、`simple_variable_expense`。中文名称全部含 `Simple 汇总`，共同分类为 Receivables、AccountsPayable、OperatingRevenue、AdministrativeExpense、OperatingCost。
- 该映射不冒充银行利息或存贷款、保险保费或赔付、地产交付；既有 Simulation chart 不变，分类仅在实际 chart 含汇总代码时增加，合并成员采用同一规则。
- `host69-real-kind-red.log` 的两项 Finance case 已实际红；`host69-kind-shares-red.log` 后段第 34、51 行分别显示配置 kind 和恢复发行人 kind 的 case 真红。本 agent 读取日志后才实施对应 chart、过账和严格恢复校验。`kind-shares-red` 前段的 issued_shares 失败不是本增量的红证据，不能归因于 Finance kind 改动；证据归属必须按实际 case 而非日志文件名判断。
- `information/source_tests.rs` 的原 Bank fixture 保留 Bank，公司配置同 Bank，合法期初 cash 改为 1003；不用改成 Industrial 回避四种类型覆盖。
- 正式边界已写入 `docs/company-accounting.md`。`host70-simple-short.log` 实际记录 Finance 13/13（0.11 秒）、Core 18/18（0.05 秒）、Source 10/10（0.02 秒）、Session 12/12（0.96 秒）、原 Q17 7/7（1.09 秒）全部通过，本 agent 已读取日志；四种类型与 kind 错配增量有新的绿证据，不沿用 host63 旧 scope。
- Q17 生产 caller 的 IndustryPresentation 已改为 `finance.industry()`。本轮原 Q17 suite 通过不等于四种类型的更正或完整股本场景全覆盖，相关能力仍须对应 owner 的独立验收。
