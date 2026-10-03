# accounting 9 动作实施记录

基线：`b89afb3346743a4b4fccf26c9ac9ff108595f696`。实施范围只包含
`packages/engine/src/accounting/**`；工业生产 caller 由 industrial worker 同步迁移。
依据已全文读取的 `AGENTS.md`、`docs/principles.md`、`docs/testing.md`、
`docs/architecture.md`、`docs/open-questions.md`、ADR-0002/0003/0005、
`docs/company-accounting.md` 及 challenge action-index 的对应动作正文。

本轮只改变 owner 与调用组织，不改变任何 A 股交易制度或会计游戏假设。
公司金额仍为 `AccountingAmount` i128 分，存货数量仍为整数件，资产寿命仍为月，
集团规格的股票数量仍为股；不引入投资者 `Money` 或现金分配。
VAT/CIT 法源取证阻塞、CAS 18 全额 DTA、存货移动加权平均、减值后剩余寿命摊销、
固定控制与合并工作底稿当期归属的既有简化仍按原代码执行。

## 逐动作

| 动作 | 文件与 owner 方法 | 真实 caller | 行为保护及未完成 |
| --- | --- | --- | --- |
| domain-N05 | `reports/window.rs`：`Accumulator::add_current_worksheet(&[WorksheetEntry])`；`bucket_all` 收窄为 private | `WindowConsolidationBuilder::apply_consolidation_output`，经 `consolidated` 与 `generate_report_set` | 新增借贷符号、空输入、cash/prior 桶不变、neg/add 溢出及跨桶写入次序用例；原自由 `apply_worksheet` 删除。root 指定短测通过；独立静态复核已完成 |
| domain-N06 | `tax.rs`：`VatPolicy::output_vat_on/split_input_vat`；`IncomeTaxPolicy::compute` 持有唯一算法主体；公开 free API 仅委托 | industrial worker 已迁 `sell_credit`、`purchase` 与 `IncomeTaxPosition::preview_income_tax`，详见 `industrial-result.md` | 新增两次 half-even、结构校验不隐式新增、到期边界、零税前与 pool 不变的用例；FIFO/负税前由原 `tax_gold` 保护。root 指定短测通过；独立静态复核已完成 |
| domain-R2-N01 | `closing/mod.rs`：private `RestatementRegister::record_correction/for_scope/save_rows/from_rows`；`closing/save.rs` 使用确定性行投影 | `ClosingEngine::correct` 成功 `Books::post_batch` 后登记；`generate_validated`；`EngineSave` 双向转换 | 新增重复 Scope/source 最后覆盖、空 Scope 行跳过、Scope 隔离与确定性行顺序；现有 correction lifecycle/serde 用例保护外部行为。保留入口 hash 缓存失效与生成失败前已登记的写入行为。root 指定短测通过；独立静态复核已完成 |
| domain-R2-N02 | `fixed_assets.rs`：`FixedAssetEntry::preview_depreciation/apply_depreciation/validate_impairment/apply_impairment` | `FixedAssetRegister` 查找后委托；工业 `depreciate_month/impair_asset` 保留 preview→post→apply | 先新增残值地板、减值不改寿命、末月 half-even、非法金额优先于 UnknownAsset 的行为测试再迁移；公开 Register API/serde 不变。root 指定短测通过；独立静态复核已完成 |
| domain-R2-N03 | `inventory.rs`：`InventoryItemState::new/validate_receipt_input/receipt/preview_issue/apply_issue`，原 `weighted_issue_cost` 主体归入 state | `InventoryLedger` 查找或创建后委托；工业开局 seed/purchase/sell_credit/produce 间接调用 | 先新增 receipt 成本溢出保留数量先写的测试再迁移；原 subledgers 用例保护数量拒绝、科目绑定和末批成本归零。quantity→cost 写入次序和金额 checked 运算不变。root 指定短测通过；独立静态复核已完成 |
| domain-R2-N04 | `consolidation/sale.rs`：`IntercompanySale::validate_for` 与 private 借用字段的 `ValidatedIntercompanySale::unrealized_profit/to_worksheet_entry`；`consolidation/eliminate.rs` 接线 | `build_worksheet` 逐 sale 验证后生成；`consolidate` 的成员、余额与少数分配协调保留 | 新增科目错误优先于非法 invoice、半偶边界与零未实现利润不生存货行用例；原 intercompany gold/failures 保护上下游及成员现金。未修改 Balance 配对或成员 Books。root 指定短测通过；独立静态复核已完成 |
| domain-R2-N06 | `reports/balance_sheet.rs`：private `EquityPresentation::from_closing/from_prior/lines/total_equity/equity_to_parent` | `balance_sheet::generate/prior_lines` | 新增单体负 retained、合并扣除非根资本、少数权益、prior_split 缺失省略权益但保留资产行、有 prior_split 的用例。比较期不调用总额方法，避免增加原先没有的 checked 求和失败面。root 指定短测通过；独立静态复核已完成 |
| domain-R2-N07 | `reports/notes.rs`：`ReportClassification::from_industries/target_for/iter`，map private；`reports/income.rs`、`balance_sheet.rs`、`mod.rs` 改用有效分类 | `generate_report_set` 两 source 分支构造一次后交 income/balance_sheet/notes | 新增四行业覆盖、额外行业映射保留、同目标幂等、漏分类拒绝；原公开 `merge_assignments` API 保留兼容，生产不能把半成品 map 传入报表计算。原 duplicate_classification 拒绝集不变。root 指定短测通过；独立静态复核已完成 |
| domain-R2-N08 | `reports/consolidated_window.rs`：private `WindowConsolidationBuilder::new/scan_members/apply_consolidation_output/finish`，拥有 acc/defs/历史权益/非根权益/root capital | `consolidated`；输出 `StatementWindows` 仍被 equity/notes 消费 | 新增三个成员、不同 minority bp、无上年历史与上年 root capital/parent/minority 拆分用例。保留 members 输入扫描顺序、BTree 求和顺序、worksheet 应用→prior_split→Scope 检查次序。root 指定短测通过；独立静态复核已完成 |

## 验证状态与精确入口

实施者按 root 要求未执行 cargo、产品测试或 Git 写命令，未把未运行测试记录为通过。
已有行为与 gold 用例作为本次无行为变化重构的保护；没有虚构新增功能的红灯结果。
静态 `git diff --check -- packages/engine/src/accounting` 已通过；仅对 own 文件执行
`rustfmt --edition 2021 --config skip_children=true`。

所有新增单测均位于原 own 源文件 `cfg(test)` 模块，精确 lib filter 为 `accounting::`，
无必需 features。建议 root 统一编译后在 10 秒外部进程树 deadline 下执行该 filter，
显式 `--test-threads` 使用 CPU 预算；不得把编译耗时计作普通单测。
既有短集成 targets：`industrial_accounting`、`consolidation`、`industry_reports`，
无必需 features，可按独立测试二进制并行执行。相关 filters 分别为
`subledgers::`、`tax_gold::`、`intercompany_gold::`、`failures::intercompany::`、
`consolidated_gold::`、`correction_restatement::`、`lifecycle_gold::`、`failures::rejects::`。

本次实现新增 13 个短单测。root 最终编译与指定短测已通过；独立完整 diff 静态审查已完成，见 [accounting-review.md](accounting-review.md)。
本组门禁已核销，汇总见 [final-summary.md](final-summary.md)；不宣称完整回归通过。

## 源文件清单

本 worker 无新增源码文件。修改的 13 个源码文件：

- `packages/engine/src/accounting/closing/mod.rs`
- `packages/engine/src/accounting/closing/save.rs`
- `packages/engine/src/accounting/consolidation/eliminate.rs`
- `packages/engine/src/accounting/consolidation/sale.rs`
- `packages/engine/src/accounting/fixed_assets.rs`
- `packages/engine/src/accounting/inventory.rs`
- `packages/engine/src/accounting/reports/balance_sheet.rs`
- `packages/engine/src/accounting/reports/consolidated_window.rs`
- `packages/engine/src/accounting/reports/income.rs`
- `packages/engine/src/accounting/reports/mod.rs`
- `packages/engine/src/accounting/reports/notes.rs`
- `packages/engine/src/accounting/reports/window.rs`
- `packages/engine/src/accounting/tax.rs`

唯一新增工作记录即本文件。
