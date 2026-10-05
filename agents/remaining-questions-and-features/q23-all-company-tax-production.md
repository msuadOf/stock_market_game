# Q23：四行业年度所得税生产接线复核

## 本批范围与独立证据

非作者 `review_q23_income_tax` 完整阅读了本批 `operations/config.rs`、
`operations/settlements.rs`、`session/persistence.rs` 的相关完整 diff，以及
`tests/company_operations/income_tax.rs` 的原六个及后补两个 case 与 `main.rs` 登记。
`persistence.rs` 还包含其他作者的 Q13 日历及相关运行状态改动；这些不作为本轮
root 所得税接线的实现或验收证据。本次未运行 Cargo、回归或 Git 操作。

`IndustryBooks` 将各行业真实 Owner 的年度评估、缴纳、候选安装、状态校验和重述身份
分派至对应账套，不用工业 Owner 冒充银行、保险或地产。`settle_period_end` 保持工商
月末折旧专有，在真实自然年末对四行业分别计提，然后按各自实际应交余额尝试付款。
真实现金不足只登记 `StatutoryPaymentFailure` 并继续其他公司；其余错误显式上抛。
下一自然日重试欠税，不自动补钱、退款、替换税基或新增投资者现金。

恢复时四行业分别校验税状态，并与本公司 `Standalone(MemberId)` 的
`ClosingEngine` 重述来源／有效期间逐值核对；没有结账映射时要求税务映射也为空。
该恢复路径不提供旧字段别名、默认补字段、迁移或新的 schema 代际。

## 领域边界

纳税年度与财报发布周期分离。显式 2500bp 税率、五年亏损结转及全额亏损 DTA 确认
沿用已登记游戏参数／简化，不代表所有现实纳税主体统一适用这些税率与优惠条件。
自然年末评估并立即尝试缴税、次日重试是既有游戏排期，不冒充法定预缴或汇算期限；
`StatutoryPaymentFailure` 标签也不证明当日已经依法逾期。官方依据和适用限制见
[所得税研究](q23-tax-official-research.md) 与正式公司会计文档。

本批仅接入内存经营和既有日级存档恢复，不新增日内持久化、退款程序或资金兜底。

## 验证状态与审查发现

root 报告 host15 的五个业务 case 已实际红；旧保险恢复夹具使原批次触及十秒 deadline，
第六个 case 不计作已运行红测。当前保险夹具已缩为每日一个合同组、两日保障期，保留
有效存档可恢复及损坏税基必须拒绝的原断言。最终 root 使用 host22 正式产物，八个
worker、每个进程 `Rayon=8`，各 exact case 外部十秒 deadline 并行验证。非作者实际
读完 `.tmp/checklist-wave4/host22-tax-*.log`：八个 case 均各运行一个真实用例并通过，
耗时分别处于 0.01–1.94 秒，没有零 case、跳过或将超时报告为通过。

静态复核未发现上述限定生产接线的阻断级逻辑问题，改动为需求所必需且保持行业边界。
此前向 root 报告的两项边界测试缺口已补齐并再次逐行核对源码与实际日志：

- 新增银行／保险／地产缴税 `PaymentFailed` 捕获至少需一组短经营 fixture 验证：
  首个公司现金不足时不补现金、不新增付款凭证，其他公司仍完成经营／付款，次日欠税
  仍在且不重复计提。`bank_tax_cash_failure_keeps_debt_and_does_not_stop_other_company`
  通过真实放贷耗尽银行可用现金，保留不足一元实际现金与欠税，不产生缴税凭证；后续
  保险公司实际缴税一次，次日银行欠税重试不重复计提。该 case 实际通过 0.01 秒。
- 四行业税务映射与结账映射交叉恢复需直接修改一个来源或有效期间的拒绝用例；
  `bank_session_restore_rejects_tax_closing_effective_period_mismatch` 从真实 Journal
  选择已存在 source，新增合法早于实际期的税务有效期映射；当前 `SaveSlot` 解析成功后，
  `GameSession::restore` 确切因税务／结账映射不同拒绝，而不是由未知 source 遮挡目标
  守卫。该 case 实际通过 1.93 秒，没有弱化原错误断言。

限定的 root 四行业年度所得税生产小批最终独立复核 **PASS**：实现必要、语义边界明确，
两项有效发现已修复且真实短测通过。本记录不核销整个 Q23、月报排期、四行业结构化
子账更正或完整税法覆盖；未执行完整回归。
