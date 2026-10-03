# luna31：Task 4/7/8 全文 EOF 复核

- 范围：产品树 `08e4fc7`（merge 同树）；全文读取 `.omo/evidence/company-information-npc-intentions/task-4-review.md`（22 行）、`task-7-review.md`（19 行）、`task-8-review.md`（162 行），均读至 EOF；并读 `AGENTS.md`、`docs/principles.md`。
- 方法：只做源码静态追踪，不运行测试、不写产品代码、不操作 Git。以下路径与行号按本次复核树。
- 结论：历史 Task 4/7/8 复核的大部分正向结论仍与当前调用链相符；Task 8 的 O1–O6 仍是覆盖/设计观察，不应直接改称实现缺陷。确认 G35 工商期末经营闭环缺口仍在。新增两个有源码反证的候选：多 lender 授信误把全体债务合计用于单 lender 限额；零应折旧基础资产导致折旧月扫提交零金额行并失败。建议纳入总账候选并由独立复核进一步确认。

## EOF 章节矩阵

| 原文章节 | 当前调用链/复核 | 结论 |
|---|---|---|
| Task 4 全文：日历算法、冻结策略、覆盖及非阻塞项 | 合同使用 `CivilDate`，工商贷款计息在 `company/industrial/loans.rs` 按自然日 `days_since` 计算 ACT/365F；滚动计息通过 `operations/injections.rs` 入队、`dispatch.rs` 派发。Task 4 记录的交易日历/冻结规则不是此领域调用者。此轮没有重新取证官方日历或重跑原测试，不能把历史测试记录冒充本轮验证。 | 历史 APPROVE 的范围证据保留；Task 4 本身没有揭示/核销工商账务缺口。 |
| Task 7 第 1 节：资金边界、公司映射、开局凭证、合同/授信、覆盖 | 当前公司仍持有独立 Books；工业开局先 `post_batch`，随后 `config.rs::seed_debt_after_counterparties` 将 `OpeningDebtTerms` 与 2001 贷方余额核对，再注册 `OPENING-DEBT` 合同和 `LoanPortfolio` 状态。`interest.rs::borrow` 校验登记对手方、合同唯一性及授信，之后过账、登记合同/loan state、记录 counterparty flow。 | 开局隐式债务合同这一历史 O2 治理决策已落地；其计息调用可用。注意授信的 lender 计算另见新候选。 |
| Task 7 下游观察 O1–O3 | Task 7 的 serde/恢复观察不据旧结论推定所有当前恢复入口安全；本任务只复核与此三篇有关的工商当前路径。工业账套仍独立于 session 投资者账户，借款现金及还款由公司账套会计处理器承载。 | 未将 O1–O3 误作 Task 8 完整经营接线证明；无新增结论。 |
| Task 8 §§0–1：复核范围、准则/游戏假设、守恒 | `accounting/journal.rs` 仍将 `BusinessKind` 与 `CashFlowClass` 分开；分录行要求正金额。VAT、ECL、tax/asset policies 与 journal/ledger 仍各有独立 owner。`industrial/expenses.rs` 所得税按总账 1–12 月收入费用计算、仅成功过账后提交亏损池；`reports/industrial.rs` 从总账读取余额。 | 不据历史独立手算替代本轮重算；当前类型/路径仍对应 Task 8 说明的价外税、ECL、税率 Fixture 和明确简化。未重新验证大 A 法源。 |
| Task 8 §2：增量范围/冻结区 | 当前 `journal.rs` 的枚举后来增加银行、地产、保险标签；工业模块仍消费若干通用标签。`BusinessKind` 是分录标签，现金流另由 `CashFlowClass` 标记。 | 枚举后续扩充不改变本轮观察：粗标签尚存，但 Task 8 §4(3) 和 Task 13 §3 明确认可延后评估/迁移，不据此升级为阻断发现。 |
| Task 8 §3：失败面、成功路径、O1–O6 | 当前工业处理器仍经 `post_with_commit` 进入 `Books::post_batch`；经营日循环还会处理现金不足的生产/补货/费用。O1–O4 是具体未覆盖成功分支，源码没有显示这些分支必然错误；O5 是只读派生访问器，借款权威路径另做 checked 计算；O6 是算法副本。 | O1–O6 分类维持非阻断观察。零折旧候选与 O1 不同：它不是测试未触达但可成功的分支，而是配置允许后月扫确定性失败，详见下节。 |
| Task 8 §4：deviation 裁定 | 当前源码仍保留 12 个职责文件、单场景金样、简化登记及通用标签使用；journal 仍不由 kind 驱动过账/报表分类。 | 不将原已接受 deviations 重开。粗标签迁移仍属延后批准的设计债，不能写作当前遗漏或自行要求改枚举。 |
| Task 8 §§5–6：开局债务、G35、残余风险 | 见下节当前生产 caller。借款及利息内核存在；但工业 maturity 未见 dispatcher 路由，税务与折旧也无经营调度。 | G35 当前仍成立；“利息全未实现”不成立。 |

## 当前 caller 跟踪

### 工商自然日与期末经营

`session/company_operations.rs::run_day_end` → `CompanyOperations::advance_civil_day` → `OperatingDayRun::advance`（`company/operations/day.rs:67–86`）依次执行 shock、到期、行业日流、次日利息排队。工商行业 caller `company/operations/industrial.rs:43–179` 仅做减值 shock、生产、赊销及 AR 到期登记、补货、管理费、坏账准备，没有调用 `depreciate_month`、`accrue_income_tax`、`pay_vat`、`pay_income_tax`、`pay_interest` 或 `repay_principal`。

工商借款日常利息计提确实接入：滚动 `InterestAccrual` 由 `operations/dispatch.rs:23–55` 派发到工业 `accrue_interest`。但 `dispatch_maturity`（同文件 `:60–103`）工业只处理 `AR:`，不处理借款合同 maturity；全仓 operations 对这些工业偿还方法仅见 API 定义而无 caller。税费方法也仅在工业账套层定义。月/年封账入口不是这些 operation caller，故不能抵消日常经营调度缺失。

此证据支持既有 G35：“工商折旧、所得税及商业债务支付未进入经营闭环”。其边界要准确表述为处理器存在、计息已接、计提/支付与折旧的生产调度缺，不是整个工商会计系统不存在，也不是股东分红/投资者现金需求。

### 新候选 A：多 lender 授信被错误合并

`OperatingBudget` 保存 `CounterpartyId → limit` 多额度（`company/contracts.rs:140–193`）。同文件 `ContractBook::outstanding_borrowings(lender)` 已按合同 counterparty 汇总单一贷款人的未偿金额（`:125–137`）。但是当前 `IndustrialBooks::borrow`（`company/industrial/interest.rs:56–65`）读取 `self.loans.outstanding_total()`，将所有贷款人的余额加总，再与当前 lender 的额度比较；错误上下文也把该全体合计写成该 lender 的 outstanding。

由此可构造确定反例：L1 已借 90，L1 额度 100；L2 额度 100、对 L2 尚无债务。向 L2 借 20 时，按其自身额度应可用，但目前用全体余额 90 + 20 > 100 拒绝。它在当前逻辑下偏保守，不会因此让某 lender 超额度；但多 lender 配置接受集与实际借款行为不一致。建议列为授信边界遗漏候选，并补双 lender 成功/拒绝用例；不可拿任务 7 单 lender 金样声称已覆盖。

### 新候选 B：合法零折旧基础让月扫失败

`FixedAssetRegister::validate_policy` 允许 `salvage_value == cost`（`accounting/fixed_assets.rs:147–163`）；初始累计折旧/减值为零。`preview_depreciation` 在剩余寿命仍大于零时直接将剩余基础除以剩余月数，因此该资产每月结果为零（`:36–49`）。`IndustrialBooks::depreciate_month` 只按 `remaining_months() > 0` 过滤，仍为它构造两条零金额分录并调用 `post_with_commit`（`company/industrial/capex.rs:61–99`）。`Books::post_batch` 拒绝非正行金额（`accounting/journal.rs:222–227`），所以整次月扫返回错误，子账寿命也不会推进；该合法种子会在其寿命内重复阻断月扫。

同样可由资产减值把剩余可折旧基础耗尽而保留剩余月数触发，因 `validate_impairment` 允许金额等于账面价值减残值（`fixed_assets.rs:64–80`）。这是合法配置/处理器组合的行为错误候选，不是 O1 的“零加工费生产成功分支无金样”；需测试精确捕获 salvage=cost、以及减值后剩余折旧额归零。

## BusinessKind 标签裁定

当前仍能看到工业粗标签：赊购使用 `CreditSale`、零加工费生产投入/成品结转使用 `Depreciation`、坏账计提/核销也出现 `Depreciation`/`ReceivableCollection`、资本开支使用 `CashExpense`（见 `purchasing.rs:81–110`、`production.rs:78–112`、`sales.rs:164–251`、`capex.rs:42–51`）。然而标签与科目行、现金流分别表达；当前 `BusinessKind` 无会计过账派发逻辑。Task 8 原 reviewer 明确将这列为冻结区下接受且有后续义务，Task 13 又裁定“无消费者时可延后，存档兼容不构成必须迁移”。因此本次不把它列为新缺陷，不借后续新增行业枚举倒推批准已经失效；若以后增加读取 kind 的产品能力，再按批准条件重审一致性与存档迁移。

## 交接结论

- 历史 Task 7 O2（开局 2001 债务没有合同）已有当前实现反证：有隐式 `OPENING-DEBT` 合同、借款状态及统一计息路径。
- 历史 Task 8 O1–O6 仍按观察/设计取舍理解；其中零折旧资产失败是本轮新增、不同的具体候选。
- G35 保持。另建议总账登记候选：单 lender 授信比较误用全体债务余额；折旧月扫遇到合法零基础固定资产会因 journal 正金额守卫失败。
- 大 A 官方规则未重新取证；未运行测试，结论仅基于完整静态源码调用链。
