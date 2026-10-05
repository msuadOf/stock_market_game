# Simple Finance 独立复核

- 复核对象：`simple/finance.rs`、其指定拆分文件与测试，`accounting/journal.rs`、`accounting/error.rs`、`accounting/closing/mod.rs`、`company/income_tax.rs` 中本轮新增接口，以及 `finance-red-harness.rs`。
- 设计基准：`agents/remaining-questions-and-features/q14-financial-model-design.md`，SHA-256 `0532fb454ca6d9b2b...`（本机完整摘要为 `0532fb454ca6d9b2cd222054863268c137de02212bdb5bebb45c42c2e88c9e46`）。
- 结论：**基础 Finance PASS**。独立静态审查通过；root 提供的 `.tmp/checklist-wave4/host63-finance.log` 记录 Finance 11 项全绿（0.09s）。本复核者未自行运行测试。此结论不包含 Simple 生产路径中 Q17 更正接口接线/集成验收，该部分由其他 owner 负责。

## 发现

### 已修复：恢复校验没有把 Closing 报告绑定到权威账簿

`SimpleFinanceState::validate()` 校验账簿试算平衡、税务位置与重述映射、事件游标及已生成期间连续性，但没有验证 `ClosingEngine` 中缓存的 `ReportSet` 是否确由当前 `Books`、覆盖期间和重述工作底稿生成。该状态支持 serde 反序列化；`report()` 与共同入口的 `closed_reports()` 直接返回缓存的最新报告。因此可构造一份自身勾稽有效、但数字与权威账簿不符的缓存报告，使其通过 owner 校验并作为公开材料提供。更正安装接口也依赖 owner 校验来确认候选账簿和 Closing 状态一致。

状态：本轮增加的 `finance_report_validation.rs` 已检查报告键、Scope、覆盖期、历史版本结构/序号/更正链，并以当前账簿和重述底稿重建 latest 后作精确比较；这足以修复本项。测试覆盖不同合法账簿报告替换、外部 Scope 与覆盖期越界的恢复拒绝。

### 已修复：删除整组已生成报告不会被拒绝

此前只遍历 Closing 当前保存的键；删除完整 `(period, kind)` 版本组会令查询将本应已生成的报告表现为不可用。当前 `validate_reports()` 按 `recognized_periods` 真实终点、报告类型端点和 `covers` 检查必需键存在，并保留通过 Books/coverage/版本链校验的额外真实登记键；测试覆盖移除生成报告后的拒绝。该边界符合现有 `close_generated_period` 生成条件。

## 语义与范围评估

- Simple 将汇总收入借记应收、收入入账；费用借记费用、贷记应付，并标记 `NonCash`，符合蓝图“仅展示汇总账面数据、无真实款项来源”的边界。禁止 `SimplePeriodSummary` 事件声称现金流，能防止把汇总变动伪装成真实现金收付。
- 余额现金不用于本模块的分红可用性判断；投资者账户现金/股份没有在本范围内被覆盖。该实现通过同一个 `Books`、`IncomeTaxPosition`、`ClosingEngine` 生成报表，没有另算净利润率或独立覆盖净利润。
- 所得税使用现有 YTD 级联并按目标差额入账，符合本轮明确的“每个实际结算期间共享税务 policy”决定；不得将其描述为完整法定预缴、汇算清缴或 CAS 18 完整实现。`docs/company-accounting.md` 将完整 CAS 18 原文状态列为取证受阻，同时记载 2026-10-05 取得企业所得税法原文与游戏级联简化边界；本文本身不是现行 A 股交易制度的新增依据。本次没有核验任何交易所交易规则变更。
- `ClosingEngine::PartialEq` 比较真实 `versions` 与重述工作底稿、忽略派生的 `OnceLock` 哈希缓存，范围精确且语义合理。
- `apply_period` 对实际输入范围显式限制连续、月初起、月末止且不跨自然年；不把季度总额伪装成三个月度金额。较长结算周期覆盖短报告周期的数据分配仍是蓝图明确未决项，当前应保持“不生成无法证明的短周期报告”，不得均分或推断。
- 变更总体限于 Simple 财务接线、摘要事件防护、Closing 比较语义和税务只读 accessor，必要性与范围基本适当。Red harness 是测试隔离工具，不是产品运行路径。

## 验收边界

- 静态 diff 未发现基础 Finance 阻断；`apply_period` 候选态事务、税务 YTD 级联、报告覆盖、Closing 重建校验和 correction owner 状态安装契约一致。Finance 独立短测以 root 提供日志为证通过。
- 不据此宣称 Simple 生产 Q17 更正接口接线或集成已完成；该范围尚由其他 owner 验收。本复核者没有运行测试，也没有扩大审查到其他领域改动。

## 补充窄范围复核：前史基准绑定

- 通过 `SimpleFundamentals::validate()` 将每家公司财务 `opening_date` 精确绑定到 `history_start.prev()`，可拒绝内部财务自身合法、却来自不同前史基线的替换存档。该约束对应创建流程：`baseline_end = history_start.prev()`，且 `SimpleFinanceState::create` 将它记录为开账日。它不会约束期间长度或生成金额，只校验同一个前史基准，因此必要且范围最小。
- 负例构造同配置/发行人、较早的 2029-11-30 开账财务并推进合法 12 月零额期间；该 Finance 子状态先通过自身 `validate()`，再替换到 history_start 为 2030-01-01 的整体存档，整体恢复应拒绝。这证明测试覆盖了跨层不一致而不是普通内部损坏。该断言与月度及其他 settlement cycle 的共同公式兼容。
- `parts_mut` 与 `reconcile_after_correction` 已从 finance state 移除；唯一跨层修正安装口为私有候选克隆、校验后提交的 `install_correction`，由 `company::correction` 路径调用。此收紧避免绕开原子校验，未影响正式共享更正编排。
- 本窄范围先以 `host65-baseline-red.log` 确认真实失败复现；后续 root 提供的 `.tmp/checklist-wave4/host66-baseline-q17-green.log` 中，baseline/company simple core 为 16/16 通过（0.05s），包括 `restored_finance_baseline_must_match_fundamentals_history_start` 与 `simple_restore_without_history_rejects_initial_baseline_drift`。因此该 guard 动态 PASS。日志同一批的 Q17 correction 为 6/7，`stale_publication_is_a_recoverable_input_conflict_and_can_be_cancelled_and_reselected` 失败；这不影响本 guard 结论，也不得表述为 Q17 集成通过。本复核者没有运行测试。

## 补充静态复核：四种 CompanyKind 的 Simple 汇总报表

- 方案使用四行业既有 AccountChart 为各自基表，仅 Simple chart 追加五个明确 `simple_*` 科目；`SimpleFinanceState::validate()` 以 `kind` 重建 chart 并要求精确一致，恢复不能用银行状态冒充工商或反向替换。`SimpleCompanyConfig.kind` 也与 issuer `CompanyKind` 校验。
- `ReportClassification::from_industries`/`from_members` 只为实际存在于 chart 的五个 summary code 添加相同列报分类；旧 Simulation charts 不含这些 code，因此分类行为不变。四行业展示复用现有 `industry_presentation(kind)`；财务 posting、报告生成/恢复重建和 correction 输入使用对应口径。Simple 会计类别不被误当证券板块。
- 汇总事件使用五个 Simple 专属科目、`NonCash`，分类到应收、应付、营业收入、管理费用及 OperatingCost。文档明确这些只是游戏摘要分类，不表示真实客户合同、生产成本/毛利、银行利息/存贷款、保险保费/赔付/LIC 或地产交付。没有新增现实会计或 A 股经营事实主张，依据是用户批准的模型边界与游戏假设，不是官方法规。范围与需求相称；未见不必要的 Simulation chart 改动。
- 静态未见阻断。Q17 生产 correction 路径的 hardcoded `Industrial` 调用不属于本批 Finance diff，由对应 owner 修复/验收；不得据本节宣称该集成完成。此四 kind 新增动态验证结果待 root 提供，不由本复核者运行。

### 四种 CompanyKind 增量动态签核

- 已亲读 `.tmp/checklist-wave4/host69-real-kind-red.log` 与当时全文 `.tmp/checklist-wave4/host69-kind-shares-red.log`。前者确实复现 Finance chart/restore mismatch 与 issuer-config mismatch。后者除 `company::spec::tests::issued_shares_uses_canonical_decimal_string_and_round_trips_u64_max` 的无关失败外，还在后续追加的两段中真实复现 `explicit_config_kind_is_required_and_must_match_issuer` 与 `restored_issuer_kind_must_match_finance_kind`；这两项可作为本增量 kind/issuer link 的有效红证据。shares case 仍与本增量无关，不归因于本改动。
- `.tmp/checklist-wave4/host70-simple-short.log` 显示本次要求的相关绿组：Simple Finance 13/13（0.11s），Simple/core 18/18（0.05s），source 10/10（含原 Bank source/cash regression），Session 12/12（0.96s），Q17 原有 correction suite 7/7（1.09s）。本次只将它们作为这些已命名 case 集的动态证据，不扩大成完整回归、Q17 新增跨行业更正覆盖或完整股本行为验收。
- 因上述静态复核及目标绿组日志，四 kind Finance/chart/kind-link、原 source Bank 回归和前史基准 guard 增量 **PASS**。该签核仍不包括 Simple 生产 Q17 correction 的四行业 Finance adapter 集成，也不证明公司行为/真实 A 股制度覆盖完整。复核者未运行测试。
