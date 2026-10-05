# CompanyOperations 测试迁移独立复核

复核日期：2026-10-06。复核范围为 `packages/engine/tests/company_operations/{industry_sessions,income_tax,session_boundary,maturities}.rs` 相对 `HEAD` 的完整可见差异，以及 `agents/company-system/operations-test-migration.md` 和 `legacy-operations-session-reference.rs`。未运行 Cargo；当前复核只覆盖静态结构，不推断动态编译或测试通过。

## 结论

迁移架构符合当前所有权边界。行业材料由真实 `CompanyOperations`、`CivilClock`、`DisclosureDispatch`、`ClosingEngine` 与 `PublicLibrary` 产生；税务与 maturities 断言使用真实低层 books/ledger/journal。没有自造业务 wrapper，也没有把这些 Ops 结果称作 Simple Session 行为。`Simple` Session 不再承载旧 `company_operations`，旧耦合的年末 Session 回滚与 payment-history Session 日历策略没有被伪装成新路径覆盖；迁移文档和 legacy 记录对此有限定。没有发现沪深 A 股撮合、证券制度或交易单位语义变化。

## 有效发现

1. **已关闭：RealEstate 年末无税断言。** 作者在 `positive_tax == false` 分支增加 `current_tax.is_zero()`；结合现有付款数和 `222104` 余额断言，意外正税额不能再通过。静态增量复核通过。

2. **工业年末低层用例保留必要证据。** `income_tax.rs::industrial_year_end_accrues_tax_once_and_depreciates_the_asset_to_84_months` 在真实 `CompanyOperations::generate_history` 后推进年末日，检查当日恰有一笔 `TaxAccrual` 且 `FA-1` 剩余 84 个月。它覆盖 Ops 年末成功结果，不声称旧 Session 注入时钟故障后的整日回滚/重试。没有发现 A 股语义漂移或多余 wrapper。

## 范围限制

- `session_boundary.rs` 以 Ops serde 恢复保留 failure history，再以 `TradingCalendar::current_default_calendar` 单独验证运行日期窗口；它不再证明旧 Session 在 restore 时对 payment-history 日期执行 runtime calendar policy 校验。文档准确承认此证据缺口。
- 旧 Sessionclock/max-due 失败回滚及税收、FA84 一次性提交并重试的耦合场景，不属于现行 Simple Session 契约。低层 Ops 的税收与 FA84 成功覆盖仍在独立 case；不能据此宣称旧 Session 触发已由新版验证。
- `income_tax.rs` 没有可从 `HEAD` 恢复的旧原文；legacy 文件声明了这一点，没有猜写旧测试内容。因而对此文件只能复核现有测试和迁移说明，不能声称逐条断言均有历史映射。
- root 提供 `.tmp/checklist-wave4/host76-migration-short-tests.log`：其中 RealEstate 年末税 case 1/1（0.01s）、工业 TaxAccrual/FA84 case 1/1（1.10s）、真实 Simple 月末披露信息不变量失败后的 whole-day rollback 与修正重试 case 1/1（0.57s）通过。它们是三个代表性短 case，不是完整 suite 或 regression；本复核未自行运行。host70 消费者曾有编译失败记录，host76 日志不证明整个迁移套件动态通过，也不覆盖完整消费者编译/回归。
