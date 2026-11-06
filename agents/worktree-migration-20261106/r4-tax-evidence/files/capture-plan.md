# Tax03 同日 capture 最小实施计划

日期：2026-10-29。状态：等待修复年度 fixture 的真实目标首红，本文不是已实施证据。

- 唯一 producer 仍为 `GameSession::prepare_ordinary_tax_day_end(day, committed_market_close)`，只在 civil shadow 上工作；不增加 Admin 事务、日期 authority 或税计算器。
- cash-dividend `Approved` 只在方案 `announced_on == day` 进入 `Announced`，错过公告或登记日明确拒绝。
- 每证券从同 candidate 的 due 未登记 cash-dividend 方案构造 `SharePostAction::CaptureRegistration { event_id: plan_id }`，与唯一实际市场日请求交 `TaxDayBoundaryFacts::prepare`。不能事后第二次 `registry.register` 绕过 typed post 声明。
- 仅使用该返回 REG 内真实 snapshot 推进对应 `CashDividendBook::register`；核发行人、证券、日期、receipt frontier、已批准 `CompanySystem::dividend_plan_facts` 与真实分配总额一致。
- 对每个已登记真实账户 entitlement，借返回 `TaxRegistrationAttribution` 核 event/account/stock/date、snapshot settled_receipts、完整税 lots 与 market book 的 operation frontier。`after_book_operation` 对照不可变 `prepared.books()` 的市场尾，不能对照已经登记第一份方案后递增的 mutable book seq；同一市场尾允许多份真实方案。
- 通过核验后沿既有 `register_dividend("dividend-tax:<plan>", day, ExactDividendTaxAmount)` 冻结税基。无持仓/无 entitlement 的空账不注册红利；税率、持期、FIFO 与转让扣收实现不变。
- 本片只到 `Registered`，没有 `record_payment`、`Paid`、`Credited`、现金或新股操作。付款与收缴需中央同 pending/id 的真实 Admin 回执，不借本片候选发布。
- 新增防御范围冻结到已有个人holder缺book和孤儿book（Tax04/05）。待各自真实目标红后加显拒守卫，不补历史、不补账户。
