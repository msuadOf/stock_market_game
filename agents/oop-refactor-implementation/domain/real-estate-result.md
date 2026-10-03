# 地产 OOP 实施记录

基线：`b89afb3346743a4b4fccf26c9ac9ff108595f696`。本 worker 仅改动
`packages/engine/src/company/real_estate/` 与本记录；未调用 Cargo、产品测试或 Git 写命令。
已全文读取该目录原有 13 个源码文件、`AGENTS.md`、principles/testing/architecture/
open-questions/error-handling，以及 ADR-0002/0003/0016/0024；动作依据为 challenge
`action-index.md` 的 domain-N04、domain-R2-N22/N25/N26/N27 正文。

## 逐动作核销

| 动作 | 文件与方法 | 真实 caller | 行为保护 |
|---|---|---|---|
| domain-N04（地产部分） | `config.rs::RealEstateConfig::check_opening_lines(&self)` 读取自身开局行，删除地产同名自由函数 | `mod.rs::RealEstateBooks::new` 保留 policy → max_projects → opening guard → post → counterparty 的顺序 | `opening_policy_precedes_first_seeded_account`；原 `rejects_opening_lines_seeding_real_estate_accounts` |
| domain-R2-N22 | `borrowing_costs.rs::BorrowingCostAccrualPlan` 私有暂态 owner 持有 through/base/items/entries/capital_by_project；`plan_accrual` 计算完整 split，`build_accrual_entries` 保持每合同先资本化后费用化与事件槽，`take_entries` 移交凭证，`aggregate_capitalized_by_project` 在原阶段 checked 汇总 | `RealEstateBooks::accrue_interest` 实际创建并消费 Plan；上游 `CompanyOperations::accrue_interest_for` 地产分支仍走此公开入口 | `accrual_tracks_mixed_loans_project_total_and_entry_slots`、`planning_and_post_failures_do_not_advance_accrual_state`、`loan_apply_overflow_preserves_existing_posted_partial_failure` |
| domain-R2-N25 | `projects.rs::ProjectState::validate_development/validate_suspend/preview_resume/validate_complete` 承接项目 guard；preview_resume 返回既有 Interruption 值；既有成本/窗口 owner 与 apply 保持 | `development.rs::RealEstateBooks::incur_development/suspend_development/resume_development/complete_project` 已迁移；delivery 的 preview_carry_out 与 accrue 的 capitalizable_days 继续调用现有 ProjectState | `project_guards_preserve_first_error_and_rejected_state`、`owner_guards_are_read_only_and_return_existing_context`、`delivery_last_unit_conserves_cost_and_collection_preconditions` |
| domain-R2-N26 | `loans.rs::ProjectLoanState::preview_interest_payment/validate_repayment` 承接单合同 guard；删除 map 级 apply_split 薄转发，在原位置直接调用现有 state.apply_split | `debt_service.rs::RealEstateBooks::pay_interest/repay_principal` 已迁移；accrue 在 post 成功后按原顺序 apply_split | `zero_amount_accrual_keeps_both_remainder_chains_and_date`、`owner_guards_are_read_only_and_return_existing_context`、`segmented_accrual_conserves_capitalized_and_expensed_chains` |
| domain-R2-N27 | `presales.rs::PresaleContract::validate_collection/validate_delivery/remaining_payment` 承接合同 guard 与 checked 余额计算；保持 collect/mark_delivered 原 apply | `collect_presale` 与 `delivery.rs::deliver` 已迁移；上游 operations::real_estate::advance_day 仍通过 Books 命令；跨合同 available_units 继续由 Books 汇总 | `delivery_last_unit_conserves_cost_and_collection_preconditions`、`owner_guards_are_read_only_and_return_existing_context`；原 guards/lifecycle 用例 |

## 测试与验证状态

先新增 `ownership_tests.rs` 的 6 个既有 API 行为保护用例，然后迁移 owner 与 caller，
最后追加 3 个直接 owner/双链守恒/部分失败用例，共 9 个短 unit tests。
因集中构建执行由主协调者负责，迁移前基线未执行，不将其描述为 TDD 红绿运行证据。
主协调者明确要求继续静态核对与 owner 迁移，最终冻结后集中重验。

- 新单测 filter：`company::real_estate::ownership_tests`，`--lib`，无额外 feature 要求，建议 harness `--test-threads=8`，执行上限 10000ms。
- 既有短测试 binary：`real_estate_accounting`，无需额外 feature；gold/failures 只验证地产会计，非全产品回归。
- 本 worker 执行 `rustfmt --edition 2021 --config skip_children=true`，精确限定于本次修改文件；成功。
- `git diff --check -- packages/engine/src/company/real_estate`：成功。
- 编译/指定短测运行：root 已执行并通过，9 个短 case；证据见 [final-summary.md](final-summary.md)。

## 语义与失败面

金额仍为 AccountingAmount 分、利率为 bp、项目单位为房屋套数，保持 ACT/365F 与
`FractionUnits = 1/3_650_000 分` 的双余数链；未修改持久结构字段、serde derive、公开 API
或输入接受集合。没有给开发/暂停/完工新增时间方向校验；仅 resume 保持原向前约束。
remaining_payment 返回原 checked sub Result，两个 caller 保留原 expect 文本。

计提仍按 `BTreeMap` 合同顺序规划，零天数跳过、零金额仍 apply carry/date、零金额不占 event
槽；提交顺序严格保留 post → 每 loan apply → checked project aggregate → project cost apply。
后段 apply 溢出仍可能出现已过账/事件推进而子账未推进，新增单测显式保护这个既有失败面；
本次不将它改成原子性修复。项目汇总也没有提前到 post 前。

预售签约无分录，收款贷记合同负债，完工交付才确认收入，交付后尾款走应收。
公司现金与投资者资金保持隔离；未改 A 股撮合、股份单位、T+1、交易日历或板块规则。
领域依据沿用 `docs/company-accounting.md` §2.5 已登记的 CAS 14（2017）条款；资本化与
中断阈值继续明确为 `game-assumption-borrowing-capitalization`，不声称真实 CAS 17 参数。

## 未完成门禁

实现已冻结并交主协调者安排针对性编译/短单测与未参与实现者独立审查完整 diff。
独立静态复核已完成且 RE-R1 已关闭；运行结果由 root 集中回填，不以静态审查宣称全量验收。

独立复核 RE-R1 已修正：`project_guards_preserve_first_error_and_rejected_state` 原 Fixture 的
开发日与完工日均为 date(1)，只证明同日接受；现仅该用例改为土地 date(1)、首次开发 date(2)、
完工 date(1)，并断言 `completed_on < dev_started_on`，真实保护原 API 的早日期接受行为。
生产 guard 未改。精确 rustfmt 与 diff 检查成功；该修正的指定短测与 reviewer 复审均已通过，证据见最终复核与 [final-summary.md](final-summary.md)。

## 独立复核回填（2026-10-03）

由未实施源码的 `/root/implement_domain/review_real_estate` 再次读取地产最终完整 baseline
diff 与未跟踪测试全文；详见 [独立复核记录](real-estate-review.md)。RE-R1 已关闭：局部
Fixture 的首次开发日为 date(2)，完工日为 date(1)，状态断言明确验证严格早于关系；共享
计息 Fixture 未改，生产 guard 未改。最终静态审查未发现新的有效问题，大 A 语义、必要
最小范围、边界/跨层/复杂度三门结论保持。

独立静态复核已完成；本 reviewer 未运行 Cargo、编译或产品测试，运行结果仍由主协调者
集中执行回填，不将静态审查描述为测试通过。最终 `ownership_tests.rs` SHA-256 为
`6fede901143cbf4f97b1a13d56d58680677be8bd5dcc8088dce8aff5cc65fc03`。
