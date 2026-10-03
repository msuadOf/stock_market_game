# bank 动作实施记录

- 基线：`b89afb3346743a4b4fccf26c9ac9ff108595f696`。
- 范围：`packages/engine/src/company/bank/**`；不修改其他行业、公共会计层或产品接口。
- 动作依据：`agents/oop-refactor-audit/challenge-2026-10-03/action-index.md` 的 `domain-N04`、`domain-R2-N10`、`domain-R2-N11`、`domain-R2-N12` 权威正文。
- 已全文阅读：`AGENTS.md`、`docs/principles.md`、`docs/testing.md`、`docs/architecture.md`、`docs/open-questions.md`、ADR-0002、ADR-0005、ADR-0016；银行会计语义依据沿用 `docs/company-accounting.md` §2.3 与 §4 的既有官方核验及生效安排。
- 保护测试先加入并静态核对，之后迁移生产代码。按上层统一验证指令，本 worker 未运行 Cargo 或测试二进制；不声称已经取得 red/green 运行证据。
- 已完成精确 `rustfmt --edition 2021 --config skip_children=true` 和限定 bank 路径的 `git diff --check`。
- 实施状态：四项代码及独立静态复核已就绪，见 [bank-review.md](bank-review.md)；root 最终编译与 9 个指定短 case 已通过，证据见 [final-summary.md](final-summary.md)。

## domain-N04（bank 部分）

- 文件：`config.rs`、`mod.rs`；保护测试：`behavior_tests.rs`。
- owner / 方法：现有 `BankConfig::check_opening_lines(&self)` 读取自身 `opening_lines`。
- 真实 caller：`BankBooks::new` 在 `EclPolicy::validate` 之后、`Books::post_batch` 之前调用。
- 移除原自由函数；禁止科目集合原样保留，逐行扫描及首个非法科目原样保留，没有将禁止清单改成允许清单。
- 测试：`opening_policy_precedes_first_forbidden_account` 固定 ECL 首错及首个非法科目。
- 验证状态：三行业合并动作由各 worker 核销；bank 最终编译/指定短测与独立静态复核已完成。

## domain-R2-N10

- 文件：`deposits.rs`、`interest.rs`；保护测试：`behavior_tests.rs`。
- owner / 方法：`DepositState::preview_accrual(through, contract)` 与 `validate_withdraw(amount, contract)`。
- 真实 caller：`BankBooks::withdraw_deposit` 调用单合同提取守卫；`BankBooks::accrue_deposit_interest` 使用合同计提预览生成原分录，再调用既有 `apply_accrual`。
- 预览读取自身本金、利率、余数、日期、到期日；期限截断、时间回拨错误字段、零分计提、同日跳过及稳定合同顺序均保留。
- 账套保留总账、事件号、现金可支付检查及客户资金流水；既有 `withdraw`、`apply_accrual`、`settle_accrued` 仍在原过账成功后的顺序应用。
- 测试：`deposit_partial_withdrawal_keeps_residual_and_maturity_cutoff`、`deposit_payment_failure_preserves_residual_flow_and_event`。前者固定部分提取后的累计余数、到期截断与停息后的完整字节不变；后者固定现金支付失败后的完整账套、余数、流水与事件号不变，以及 unknown 优先于零金额。
- 验证状态：root 最终编译/指定短测与独立静态复核已完成。

## domain-R2-N11

- 文件：`ecl.rs`、`lending.rs`；保护测试：`behavior_tests.rs`。
- owner / 方法：`EclPolicy::initial_allowance_target`；单次借用对象 `EclScenariosRef::{validate, allowance_target}`。
- 真实 caller：`BankBooks::new` 经 `EclPolicy::validate` 校验两表；`BankBooks::issue_loan` 经政策计算首日准备；`BankBooks::assess_credit` 先构造受检情景借用，再查贷款、校验阶段、计算目标。
- 借用对象直接持有 `&[EclScenario]` 并拥有权重校验和 checked i128 加权计算，不复制两张政策表、不保存派生金额、没有新增持久状态或服务对象。
- 精确保持 `stage1_default`、`lifetime_default`、`assess` label、逐字段首错顺序、checked 运算、一次 `rhe_div` 舍入和所有错误字段。
- 与候选名称的适配：采用 `EclScenariosRef`，没有宣称构造后永久 validated。原 `issue_loan` 没有再次校验通过 serde 恢复的政策；若强行要求其使用必经 validate 的对象会改变接受集合。因此初始政策路径沿用原启动校验责任，assess 明确通过 `validate` 构造同一借用类型。
- 测试：`ecl_sums_scenarios_before_single_half_even_rounding` 固定多个情景仅累计后舍入（0.5→0、1.5→2）；`ecl_validation_precedes_unknown_loan_and_overflow_does_not_commit` 固定列表首错及 checked overflow 前完整状态不变；`restored_initial_policy_is_not_revalidated_during_issue` 固定原 serde 接受集合和发行行为。
- 验证状态：root 最终编译/指定短测与独立静态复核已完成。

## domain-R2-N12

- 文件：`loans.rs`、`lending.rs`、`interest.rs`、`writeoff.rs`、`ecl.rs`；保护测试：`behavior_tests.rs`。
- owner / 方法：`BankLoanState::new` 接收 day-one allowance；`preview_accrual`、`validate_principal_collection`、`validate_interest_collection`、`validate_assessment`、`preview_write_off`、`validate_recovery`；内部复用 `require_not_written_off`。
- `rate_bp`、`counterparty`、`allowance` 收为 private；借款人访问经只读 `counterparty()`；发行不再构造后直接写 `allowance`。
- `LoanWriteOffSnapshot` 仅承载受检本金、利息与毛额分录事实，不是第二份贷款状态。
- 真实 caller：`BankBooks::issue_loan`、`collect_loan_principal`、`accrue_loan_interest`、`collect_loan_interest`、`write_off`、`recover_written_off`、`assess_credit`；上层生产链仍由 `company/operations/bank.rs` 与 `company/operations/dispatch.rs` 调用既有 BankBooks API。
- BankBooks 保留 validate→post→apply、事件号和跨边界资金流水；没有在 post 后新增可失败守卫，也没有前置原在 apply 时才会发生的算术错误。
- 阶段 1/2 本金基数、阶段 3 `max(gross−allowance, 0)` 基数、核销准备足额规则、核销后零分日期/事件号推进、回收后超额准备待重估转回均按原实现保留。
- 测试：`loan_collection_and_recovery_keep_first_errors_and_exact_limits` 固定未核销回收、超本金、核销态优先于零额、恰好收清利息及恰好全额回收；`loan_same_day_and_written_off_accrual_keep_zero_amount_progress` 固定同日不推进和核销后的零分推进；`recovery_overflow_retains_existing_post_then_partial_apply_order` 固定异常恢复输入下原有过账成功、事件号提交、recoverable 先减少、allowance 加法溢出、未登记资金流水的部分写入顺序。
- 验证状态：root 最终编译/指定短测与独立静态复核已完成。

## 短验证入口

- 新增内联测试 filter：`company::bank::behavior_tests`，九个 case，无额外 feature。
- 构建目标：`engine --lib`；使用上层统一的必要编译阶段，显式多核，编译与测试耗时分开记录。
- 预构建二进制运行：上述 filter，显式 `--test-threads` 多核，并设置整命令 10000ms 进程树 deadline；测试没有 I/O、计时器、长 fixture 或并发共享状态。
- 既有 `packages/engine/tests/bank_accounting` suite 的 gold/failure 用例仅供审查对应原语义；本 worker 未运行产品回归或长验收。

## 语义与范围核对

- 保留银行公司会计和证券投资者账户的边界；存款为负债、贷款为资产、客户现金流为原经营分类；不改变沪深 A 股交易制度。
- `AccountingAmount` 单位为分；`FractionUnits` 在本域为 1/3_650_000 分；利率与 ECL 因子仍为 bp。
- PD/LGD/EAD 仍是显式政策/情景输入，未引入股票跌幅、隐式补钱、透支、罚息、复利或自动转存。
- public API 与 serde 字段/形状原样保留；仅模块内部调用和私有字段访问改变。
- 没有增加依赖；没有编辑既有调查正文；没有运行 Git 写操作。
