# industrial 职责迁移记录

- 实施基线：`b89afb3346743a4b4fccf26c9ac9ff108595f696`。
- 源码范围：仅 `packages/engine/src/company/industrial/**`；本工作记录按协调者要求单独归档。
- 已全文核对 `AGENTS.md`、`docs/principles.md`、`docs/testing.md`、`docs/architecture.md`、`docs/open-questions.md`、`docs/error-handling.md`、ADR-0016/0024、`docs/company-accounting.md` 与相关 action-index 正文、industrial 完整源码及既有短测试。
- 规则范围：保持已登记的公司经营会计简化、ACT/365F、AccountingAmount 分与 serde 元十进制字符串、公司/投资者现金隔离。不增加或修改 A 股交易制度、税率、税法或 CAS 合规承诺；VAT/CIT 法源与 CAS 18 取证受阻的现有登记不变。
- 当前状态：实现、静态核对与独立静态复核已完成；IR-01 修复后复审通过且无待修有效 findings。root 最终编译与 7 个指定短 case 已通过，本组核销证据见 [final-summary.md](final-summary.md)。

## domain-R2-N13

- 文件：`config.rs`、`mod.rs`。
- owner：`IndustrialOpeningReconciliation<'a>` 借用已经过账的 Ledger 和 config 的 inventory/assets/debt 种子字段、开局日期。chart/opening_lines 已按原序转移给 Books，不克隆或复制 config 开局事实。
- 方法：`seed_inventory_and_assets`、`seed_debt_after_counterparties`；inventory/assets/debt 的核对算法主体改为私有 receiver 方法，旧 free helper 删除。
- caller：`IndustrialBooks::new` 先调用第一阶段，仍自行登记 counterparties，再调用第二阶段并接收 ContractBook/LoanPortfolio。
- 首错保留：tax validate → opening post → inventory → assets → counterparties.register → opening debt。无 debt terms 时仍不新增 2001 必须为零的验证；累积折旧拒绝、隐式 OPENING-DEBT 的短期科目语义保持。
- 保护：`opening_first_error_keeps_seed_counterparty_debt_order` 固定同时存在多项错误时的顺序；`opening_implicit_debt_roundtrip_keeps_short_account_and_credit_usage` 固定长到期隐式开局债仍记短期、序列化字节往返与授信占用。

## domain-R2-N14

- 文件：`loans.rs`、`interest.rs`、`repayment.rs`、`mod.rs`，开局 caller 的 `config.rs` 同步使用组合。
- owner：唯一 `LoanPortfolio` 持有原 BTreeMap；`#[serde(transparent)]` 保留 `IndustrialBooks.loans` 存档 map 形状。ContractBook、授信、CounterpartyFlow、Books 仍由原对象持有。
- 方法：`get`、`iter`、`outstanding_total`、`preview_accruals`、`preview_repayment_accrual`、`apply_posted_accruals`、`insert_registered_loan`、`settle_posted_interest`、`repay_posted_principal`。
- caller：`IndustrialBooks::{loan,loans,available_credit,borrow,accrue_interest,pay_interest,repay_principal}` 与开局 seed。原 `loans_mut`/`loans_outstanding_total`/`accrue_loan`/free `apply_accrual` 删除，不保留第二份算法。运行时接线是 `company/operations/dispatch.rs::accrue_interest_for` 的 `IndustryBooks::Industrial` 分支，通过原公开 `accrue_interest` API 使用唯一组合。
- `preview_accruals` 返回稳定序的逐项结果 iterator，不先把所有合同批量计算完：保持“本项计息 → event id → 下一合同”的既有错误/溢出顺序；不复制合同或贷款状态。
- 所有子账提交保持原 post 成功后的准确位置。pay_interest 的合同 counterparty 检查仍在 post 后、清利息前；repay 的合同 counterparty 检查仍在计提/还本后。原缺 map 项时 no-op 的内部接受行为保留，不借重构强化恢复校验。
- 保护：`positive_days_zero_interest_keeps_carry_date_and_event_gap`、`rejected_repayment_keeps_preview_uncommitted_and_serialized_shape`、上述开局隐式合同往返用例。零分正天数继续推进 carried/date，全部计提按 item 数消耗 event id，还本日零分计提不增加分录 event。

## domain-R2-N16

- 文件：`expenses.rs`、`mod.rs`。
- owner：`IncomeTaxPosition` 唯一持有 loss_pool；policy 从账套的 TaxPolicy 只读借入，VAT/CIT/DTA 科目余额继续由 Books 派生。
- 方法：`loss_pool`、`preview_income_tax`、`year_pretax`、`commit_loss_pool`；年度收入/费用汇总算法主体迁入 position，`IndustrialBooks::accrue_income_tax` 继续编排 JournalEntry、post 与原成功点 commit。
- serde：position transparent 包装 Vec；IndustrialBooks 字段 `#[serde(rename = "loss_pool")]`，字段位置与 JSON 名称/形状维持；不新增存档事实或 policy 副本。
- caller：公开 `accrue_income_tax` 和 `loss_pool`；尚无运行时经营 dispatch 调用税务计提，不声称新增税务日流。
- 保护：`repeated_annual_tax_keeps_existing_non_idempotent_loss_behavior` 明确现有同年重复调用会再次增加亏损且年度税前包含已记税费；`zero_line_tax_still_commits_expired_loss_pool` 固定原零分录路径也提交 ending_pool；`failed_tax_post_keeps_loss_pool_and_event_id_unchanged` 固定封期 post 失败不提交。

## domain-N06 协同 caller 迁移

- accounting worker 拥有 policy 算法与公共 free facade，本 worker 不修改 accounting。
- `sales.rs::sell_credit` → `VatPolicy::output_vat_on`。
- `purchasing.rs::purchase` → `VatPolicy::split_input_vat`。
- `expenses.rs::IncomeTaxPosition::preview_income_tax` → `IncomeTaxPolicy::compute`。
- 不增加 validate、默认税率、税款余额副本；进项两次 half-even 和 FIFO 等 policy 语义由 accounting worker 保持。

## 验证与未完成

- 新增 `ownership_tests.rs` 中 7 个针对性短单元测试；`mod.rs` 仅在 `cfg(test)` 挂载。
- 先添加行为保护测试，再进行实现迁移。协调者明确允许在集中 baseline 构建尚未取得时继续实施；本记录不宣称执行过 Red/Green 或 baseline 通过。
- 交给统一 runner：engine `--lib`、默认 features、filter `company::industrial::ownership_tests`，正式执行应使用既定 10000ms 进程树期限和显式多线程参数。必要的既有 `industrial_accounting` 短行为用例由协调者按允许范围选取；本 worker 未运行 cargo 或产品测试。
- 已执行：限定本簇 9 个 Rust 文件的 `rustfmt --edition 2021 --config skip_children=true`；`git diff --check -- packages/engine/src/company/industrial` 通过。未执行全仓格式化、Git 写操作或再派 agent。
- 当前未完成事项：无；root 指定短测通过且独立复核已闭环，不代表全量回归。
- 独立复核 IR-01 已修且复审通过：删除将 `company/operations/bank.rs` 的 BankBooks 贷款迭代误归属本组合的 caller 声明，逐行核对后只保留 dispatch 的 Industrial 分支真实接线；仅修改本记录，不改源码或历史 action-index。reviewer 已关闭 IR-01、确认无待修有效 findings；完整静态复核记录见同目录 `industrial-review.md`，不冒充编译/测试通过。
