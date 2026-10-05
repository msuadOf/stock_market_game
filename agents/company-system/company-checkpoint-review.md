# CompanySystem checkpoint 独立复核

日期：2026-10-06。复核对象为当前 HEAD 至 worktree 的 `packages/engine/src/company/` 全部变更，包含新增未跟踪源码；这是静态复核，不运行测试、不修改产品源码、不暂存或索引文件。已有独立记录与本次工作树日志只用来界定各项验证声明，不替代本次实际源码检查。

## 结论

初审曾发现阻断问题：Simple 丢失发行人的 `CompanyKind`，所有公司均使用 Industrial 会计科目表与报表列报。root 随后为四种 kind 接入了显式映射，本次增量复核确认该 finding 已关闭，详见下方“增量复核”。

除此项外，本次 Simple period growth/finance、严格恢复、共享报告更正以及描述中的实际边界与已登记设计大体吻合。该判断**不等同整批通过**：能力接口如实标记 `cash_settlement: false`，因为共同股本行为的真实投资者结算未接入；不能据此声称分红、认购或回购已完成，也不能误述为因 Simple 账面现金不足而不支持。低层 ownership、所得税、股权登记模块属于与 Session Simple 不同的底层能力；不把它们描述成已被 Simple Session 调用或完成 Simulation。

## 语义依据与边界

- 规则基线见 `AGENTS.md`、`docs/principles.md` 与 `docs/trading-rules.md`。本 diff 没有改变证券撮合、T+1、证券类别或委托规则；Simple 增长率和金额仍以公司账面 `AccountingAmount`/元为单位，不改变账户交易 `Money`/分或股数。
- 公司行为和个人现金股息税的登记依据在 `agents/remaining-questions-and-features/corporate-actions-research.md`、`dividend-research.md`：上深交易所 2026 修订规则自 2026-07-06 施行，登记日/除权日与各自除权参考价规则有明确差异；财税〔2015〕101 号针对公开市场个人股息红利按持有期计税。不可将这些规则外推成所有转增来源或所有股东身份的统一税务规则。本次 Simple 不新增该制度实现。
- 企业所得税研究依据 `agents/remaining-questions-and-features/q23-tax-official-research.md` 和 `docs/company-accounting.md`：税务总局现行《企业所得税法》页面标全文有效，2026-10-05 核对，按公历纳税年度并要求预缴/汇算；CAS 18/28 全文仍有取证缺口。Simple 按已记录游戏简化执行 YTD 差额计提，不能报告为完整法定税务或 CAS 合规。公司税不是 A 股交易制度变更。
- `Simple` 的账面现金不充当真实付款预算；投资者资金与股份须按实际交易事实结算。ADR-0035 明确未授权 `SyntheticFunding`。`CompanyCapabilities.cash_settlement=false` 及其理由清楚标示尚未接通，不把“接口字段存在”当作能力已实现。
- Simple 历史中的 `prehistory_periods` 是完整配置周期，不把部分开局月提前确认；历史解释按生成输入校验；账簿摘要列作 NonCash，符合不伪造公司资金流向边界。Q17 共享更正作者记录指出实际路径先在候选上准备/验证并以整日候选提交；root 提供的 host68 为 Q17 Session 7/7，限于该组短测，不扩为完整回归。本审查未自行运行该命令。

## 完整文件清单

以下包含本次检查范围内 HEAD->worktree 的全部已跟踪修改及新增未跟踪文件（按 `git diff --name-only HEAD -- packages/engine/src/company` 和 `git ls-files --others --exclude-standard packages/engine/src/company` 取得）：

```text
packages/engine/src/company/api.rs
packages/engine/src/company/bank/behavior_tests.rs
packages/engine/src/company/bank/chart.rs
packages/engine/src/company/bank/claim_source_tests.rs
packages/engine/src/company/bank/claim_sources.rs
packages/engine/src/company/bank/config.rs
packages/engine/src/company/bank/error.rs
packages/engine/src/company/bank/income_tax.rs
packages/engine/src/company/bank/income_tax_tests.rs
packages/engine/src/company/bank/interest.rs
packages/engine/src/company/bank/lending.rs
packages/engine/src/company/bank/loans.rs
packages/engine/src/company/bank/mod.rs
packages/engine/src/company/capabilities.rs
packages/engine/src/company/cash_dividend_tax.rs
packages/engine/src/company/cash_dividend_tax/tests.rs
packages/engine/src/company/config.rs
packages/engine/src/company/correction.rs
packages/engine/src/company/economy/system_sim/DESIGN.md
packages/engine/src/company/economy/system_sim/README.md
packages/engine/src/company/identity.rs
packages/engine/src/company/income_tax.rs
packages/engine/src/company/industrial/error.rs
packages/engine/src/company/industrial/expenses.rs
packages/engine/src/company/industrial/income_tax.rs
packages/engine/src/company/industrial/mod.rs
packages/engine/src/company/industrial/ownership_tests.rs
packages/engine/src/company/insurance/behavior_tests.rs
packages/engine/src/company/insurance/chart.rs
packages/engine/src/company/insurance/config.rs
packages/engine/src/company/insurance/error.rs
packages/engine/src/company/insurance/groups/restore.rs
packages/engine/src/company/insurance/income_tax.rs
packages/engine/src/company/insurance/mod.rs
packages/engine/src/company/insurance/session_restore_tests.rs
packages/engine/src/company/mod.rs
packages/engine/src/company/operations.rs
packages/engine/src/company/operations/config.rs
packages/engine/src/company/operations/core.rs
packages/engine/src/company/operations/error.rs
packages/engine/src/company/operations/industrial.rs
packages/engine/src/company/operations/restore.rs
packages/engine/src/company/operations/settlements.rs
packages/engine/src/company/persistence.rs
packages/engine/src/company/query.rs
packages/engine/src/company/real_estate/chart.rs
packages/engine/src/company/real_estate/config.rs
packages/engine/src/company/real_estate/development.rs
packages/engine/src/company/real_estate/error.rs
packages/engine/src/company/real_estate/income_tax.rs
packages/engine/src/company/real_estate/mod.rs
packages/engine/src/company/real_estate/owner_state.rs
packages/engine/src/company/real_estate/owner_state_tests.rs
packages/engine/src/company/real_estate/ownership_tests.rs
packages/engine/src/company/real_estate/projects.rs
packages/engine/src/company/report_correction.rs
packages/engine/src/company/rng.rs
packages/engine/src/company/share_registry.rs
packages/engine/src/company/share_registry/tests.rs
packages/engine/src/company/simple/config.rs
packages/engine/src/company/simple/environment.rs
packages/engine/src/company/simple/finance.rs
packages/engine/src/company/simple/finance_config.rs
packages/engine/src/company/simple/finance_period_tests.rs
packages/engine/src/company/simple/finance_posting.rs
packages/engine/src/company/simple/finance_report_validation.rs
packages/engine/src/company/simple/finance_state.rs
packages/engine/src/company/simple/finance_tests.rs
packages/engine/src/company/simple/finance_validation.rs
packages/engine/src/company/simple/growth.rs
packages/engine/src/company/simple/mod.rs
packages/engine/src/company/simple/period.rs
packages/engine/src/company/simple/state.rs
packages/engine/src/company/simple/tests.rs
packages/engine/src/company/spec.rs
packages/engine/src/company/system.rs
```

## 增量复核：四种 CompanyKind

复核日期：2026-10-06。重新完整读取 Simple 财务 kind helper/state/posting/report validation/config、四 kind 测试、`simple_summary` 报表分类器以及 `docs/company-accounting.md` 对应新增边界；检查 Session/公开会计 kind 映射与 Web 当前严格 schema。当前版本把 `SimpleCompanyConfig.kind`、`SimpleFinanceState.kind` 作为必需事实，创建时要求配置 kind 与 issuer kind 相同；finance owner 由 issuer kind 派生构造，状态恢复校验 kind、chart、issuer 和配置一致。报表结账、恢复重建和 correction 准备均读取同一个 `finance.industry()`，不再落回 Industrial。

四种 Simple 使用各自基础 chart，再增加五个专属 summary 科目；这些科目均为明确 `simple_*` 代码，并只在 chart 存在该科目时参加分类。收入费用映射到通用摘要行，但银行利息/贷款、保险保费/理赔、地产交付等专用行不会被虚构摘要事件填充。摘要凭证保持 `NonCash`，不形成真实公司现金收付；文档已说明科目为游戏专属非官方编号、固定/变动费用的分类是游戏摘要列报，不冒充 gross margin、合同、客户或行业经营。没有为支持四 kind 隐式根据证券代码推行业；Session issuer identity 测试直接覆盖显式 kind。

对“共享分类只在基础 chart 存在相应 account 时才映射”的实现，核对了普通与 consolidated `ReportClassification` 两条路径：新增 summary assignments 分别按成员/合并 chart 实际 account set 过滤，Simulation 行业 account set 不包含 `simple_*` 时不会被赋予额外分类；四 kind Simple 报告仍执行完整 `ReportSet::validate`。kind 更改或删除 kind 的恢复负例存在，避免保存状态通过强制另一 kind 静默更换 report presentation。没有发现本次四 kind 增量的新 A 股语义漂移或多余扩张。

root 提供 `.tmp/checklist-wave4/host70-simple-short.log` 与说明：fresh engine lib 编译成功；定向短测 9 组 85 项通过，其中四 kind Simple 完整报表、issuer/config kind 一致、Session 显式配置和恢复、更正、增长/财务、information 与既有 group consumer 均有覆盖。这不是独立重跑，也不是 full regression/WASM/宿主发布验收；日志由多组并行输出合并，不能扩称整套公司系统完成。先前 `cash_settlement=false` 的未接线边界仍有效。

**增量结论：初审唯一 P1 已修复且复核通过；当前本审查未发现其他必须修复项。** 产品完成声明仍须遵守 ADR-0035 验收边界，尤其 Simple 与真实股本结算的对接、各宿主/full regression 不由本次四 kind 结果代替。

## 复核记录

已有模块级审查材料包括 `agents/company-system/q17-simple-independent-review.md`、`simple-core-independent-review.md`、`final-period-core-independent-review.md`、`growth-factor-independent-review.md`、`simple-finance-review.md`、`session-review.md`、`low-level-restore-review.md` 和 `agents/remaining-questions-and-features/q23-tax-official-research.md`。其中既有记录明确限制自己的 scope 与验证范围；不得将局部 PASS 拼写成全部公司系统完成。待上述 CompanyKind/reporting finding 修复后，对完整新 diff 再做一次独立复核。
