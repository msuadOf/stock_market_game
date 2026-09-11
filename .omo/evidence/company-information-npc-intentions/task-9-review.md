VERDICT: APPROVE

# W2-Task 9 独立复核：银行合同会计与报表映射（commit 3bb5096）

复核人：独立 subagent（未参与实现）。分支 `codex/feat/web-ui-polish`，主仓
D:\workplace\stock_market_game。按 AGENTS.md「大 A 语义与独立复核门禁」执行。

## 0. 隔离执行记录（全部命令 + 退出码 + 计数）

主仓树有并发 sibling agent 未提交工作（`tests/real_estate_accounting/` 未跟踪、
正在写入），按指令**未在主仓执行任何 cargo**。隔离环境：
`git worktree add C:\Users\msuad\AppData\Local\Temp\opencode\wt-review-9b 3bb5096`
（exit 0，detached HEAD = 3bb5096，内容与提交一致）。

| # | 命令（均在隔离 worktree） | 退出码 | 结果 |
|---|---|---|---|
| 1 | `git show 3bb5096 --stat`（主仓只读） | 0 | 22 文件 +2893 行，与计划文件集一致 |
| 2 | `cargo test -p engine --test bank_accounting` | **0** | **11 passed / 0 failed / 0 ignored** |
| 3 | `cargo test -p engine`（全量） | **0** | 26 套件，**660 passed / 0 failed / 4 ignored**（112 套件含 4 ignored） |
| 4 | `cargo check -p engine -p server -p web-wasm -p engine-gpu` | **0** | Finished，无 error/warning |
| 5 | `cargo clippy -p engine --all-targets` | **0** | 见 §5 裁定 |

计数对账：worker 在共享树 02:50 记录的 660/0/4（task-9-full-suite.txt，823 行）
与本人干净树重跑**完全一致**，无差异。660 = 基线 614 + 本任务 11 + 并行任务 19
的 33 + 任务 17/20 增量（worker issues.md 第 8 条如实登记，成立）。Tauri 桌面壳
按指令跳过（gitignored web/dist 未生成）。

原始输出：`C:\Users\msuad\AppData\Local\Temp\opencode\t9r-{1,1b,2,2b,3,3b,4,4b}.txt`。

## 1. 问题一：大 A / 会计语义符合性与依据是否可靠

**结论：符合，依据可靠。**

- **政策锚核验**：docs/company-accounting.md §2.3 银行行（CAS 22（2017）§16/§17/
  §21/§38/§39/§46–48/§57/§58/§60/§61 + CAS 30（2026）§45–47/§16）与
  tests/fixtures/company-model/policy-sources.json 中
  `cas-22-financial-instruments-2017`、`cas-30-presentation-2026` 的
  verified-official 银行行一一对应；§6 明确列结构化衍生品与 FVTPL/FVOCI 为
  显式不支持——与 `BankProductKind::{StructuredDerivative,FvtplInstrument,
  FvociInstrument}` 的类型化拒绝吻合。
- **K3 红线逐条验证**（代码 + 测试双重）：
  - 存入 1000 → Dr 1003 / Cr 2011（deposits.rs），gold 断言 `ST_DEPOSIT = -1000`
    且 `INTEREST_INCOME = 0`。存款是负债不是收入 ✓
  - 发放贷款 600 → Dr 1301 / Cr 1003（lending.rs），无费用科目参与；gold 断言
    `LOAN_PRINCIPAL = 600`、`CASH = 2405`。贷款是资产不是费用 ✓
  - 存款付息：计提 Dr 6411 / Cr 2231（费用+负债），支付 Dr 2231 / Cr 1003——
    本金不动，gold 终态 `ST_DEPOSIT = -100_000` 未变 ✓
  - 利息应计：ACT/365F 整数计提 + `FractionUnits`（1/3,650,000 分）合同累计
    余数，落分守恒；平价固定利率 ⇒ 合同利率=实际利率（文档化等价，登记简化）✓
  - 逾期贷款：按合同利率续计息（无罚息，登记简化）；阶段三转净额法计息
    （CAS 22 已发生信用减值金融资产），基数 = max(账面−准备, 0) ✓
  - 三阶段 ECL：`ecl_allowance_target = rhe(Σ(权重×PD×LGD×gross)/10^12)`，
    PD/LGD/EAD 全部为显式情景参数（`EclScenario` 构造期 + 逐次重估双守卫：
    字段域 [0,10000]bp、Σ权重恰为 10000bp）；**全模块零市场/价格数据读取**
    （grep 验证 company/bank 无 session/market/strategy/observation/account import）——
    绝无从股票跌幅认定信用损失 ✓
  - 核销先耗准备：`InsufficientAllowance` 守卫（准备 ≥ 账面）+ 分录 Dr 1303 /
    Cr 1301 / Cr 1131；回收 Dr 1003 / Cr 1303（贷记准备），回收后同阶段重估
    转回超额准备（gold 钉死 10,000 分转回）✓
  - 现金约束：提款/放贷超可支付现金 → `BatchAborted(NegativeCashProhibited)`
    映射为类型化 `PaymentFailed`（error.rs `map_post_error`，不吞错、其余
    Accounting 错误原样透传），guards 测试断言失败后账套字节不变、银行继续
    运行（小额提取成功）——无负现金、无透支、无自动补钱 ✓
- **报表分类层**（reports/bank.rs）：纯读投影（`&Ledger` → 行组合），不生成
  报表、不结账、不触现金（4103 仅入科目表供任务 13）；净利息收入 = 6011 贷方
  − 6411 借方，信用减值损失净贷方如实为负（转回利得）。现金流分类：存/贷/
  收息/付息/手续费全记 Operating——CAS 30 (2026) §45–47「向客户提供融资为
  主要业务活动」的归类选择，与工商业借款记 Financing 的差异已在 learnings
  钉死防漂移。
- **金样手工复算**（全部独立重算，非照抄注释）：
  - 贷款 27 天：60000×600×27 = 972,000,000 / 3,650,000 = 266 余 1,100,000 ✓
  - 存款 30 天：450,000,000 / 3,650,000 = 123 余 1,050,000 ✓
  - 阶段 1→2 目标：60266×(10000×800×5000)/10^12 = 2410.64 → rhe 2411，
    补提 2411−300 = 2111 ✓（日终 ECL：60000×0.005 = 300 ✓）
  - 阶段 2→3：收息 266 后 gross 60000，目标 2400 → 转回 11 ✓
  - 阶段 3 净额 28 天：(57600×600×28 + 1,100,000) = 968,780,000 → 265 余
    1,530,000 ✓；存款 28 天：421,050,000 → 115 余 1,300,000 ✓
  - 100% 计提 60,265 / 核销 60,265 / 回收 10,000 / 转回 10,000 ✓
  - 终态：NI = 531−238+500−50,265 = −49,472；权益滚动 150,528 = 资产
    250,528 − 存款 100,000 ✓（会计恒等式在测试内成立）
  - 到期停息：10 天期 100000×150×10 = 41 余 → 41 分，二次计提为空 ✓
  - 余数守恒不变量（Σ已提×3,650,000 + 终余数 = Σ(基数×bp×天数)）跨阶段
    转换（毛额→净额基数）保持——余数属合同计提流而非特定基数，全局守恒成立。
- **存档兼容**：`BusinessKind` 新增 12 个变体（任务书说 13，实为 12——
  CustomerDeposit/Withdrawal、LoanIssued/PrincipalCollected/InterestAccrued/
  InterestCollected、DepositInterestAccrued/Paid、FeeAndCommissionEarned、
  CreditImpairment、LoanWriteOff、WriteOffRecovery），serde 外部名称标记
  （按变体名序列化）→ 旧存档反序列化不受影响，无判别值/序列化破坏；
  journal.rs 是任务 6 语义冻结区但其自身文档预留任务 8–11 行业扩展（learnings
  task-8 条目原文），本扩展纯加标签、零过账逻辑改动 ✓。`BankBooks` 整体
  serde 往返在 gold 内断言等价 ✓。

## 2. 问题二：必要性与最小范围

**结论：必要且最小。**

- 22 文件全部落在计划文件集内（company/bank/ 12 文件 + accounting/reports/
  {mod,bank}.rs + journal.rs +25 纯标签 + 两处 +1 行模块声明 + tests/
  bank_accounting/ 6 文件）。无越界改动、无无关重构、无投机抽象。
- 共享文件接线均为 1 行：accounting/mod.rs `pub mod reports;`、company/mod.rs
  `pub mod bank;`。后者是任务指令 staging 清单外的 +1（worker issues.md 第 1
  条已登记偏离）——**裁定：必要**（模块不声明无法进 crate），与前者完全同构，
  按 lib.rs 共享文件协议执行，无契约变化。接受。
- 依赖方向正确：accounting/reports/bank.rs 只 import accounting::{amount,
  error,ledger}（不 import company）→ 科目代码单一真源 `codes` 放 reports 侧、
  company/bank/chart 引用同一份，两层无漂移面 ✓（架构铁律：company →
  accounting 单向）。
- 科目表 v3 是新函数，不改 v1 语义/不触碰 industrial v2（diff 零涉及）。与
  industrial 码位重叠仅 2231（应付利息，语义一致）/4001/4103（实收资本/本年
  利润，语义一致）——各 Books 实例持独立科目表，码位按账套命名空间隔离，
  无串扰面。1003（存放中央银行款项）与工业 1001/1002 区分 ✓。
- `post_with_commit`/validate→post→apply/拒绝零状态变化（含事件 id 不消耗）
  ——完整复用任务 6/7/8 已复核不变量，`assert_eq!(bank, before)` 逐失败路径
  断言（next_event_id 是字段，字节相等 ⇒ id 未消耗）✓。

## 3. 问题三：遗漏边界测试 / 跨层语义漂移 / 不必要复杂度

**结论：无阻塞项；4 个次要覆盖缺口（非阻塞，登记供后续任务顺手偿还，同
task-8 复核先例处理）。**

覆盖矩阵（计划 K3 银行行验收 vs 测试）：存入非收入 ✓ / 贷放非费用 ✓ /
付息费用口径 ✓ / 应计利息+守恒（手工复算）✓ / 逾期处理（部分，见 F3）/
三阶段 ECL 显式情景 ✓（非法 PD/LGD/权重和双守卫测试）/ 核销先耗准备 ✓ /
回收贷记准备+转回 ✓ / 现金约束 PaymentFailed ✓（提款+放贷双路径）/ 报表
分类层 ✓（11 个列报行逐行断言）/ UnsupportedContract ✓（StructuredDerivative
+Fvtpl+Fvoci 三类、断言 kind 字段）/ 到期停息 ✓ / 无默认银行股 ✓（恰 5 只
股票、C-TEST-BANK 未上市、发行映射校验）/ 跨边界资金流 ✓（稳定 ID +
Inbound/Outbound，NonCash ECL 不记流）/ serde 往返 ✓。

次要缺口（发现，均非阻塞）：

- **F1（最实质）**：`collect_loan_principal` **成功路径**无任何测试触达
  （gold 的 L1 本金走核销；guards 只测超额拒绝）。任务 21 教训明文「每个
  处理器成功路径可达且被行使」。过账行（Dr 1003/Cr 1301）+ 子账本金减少 +
  对手方流三面均未钉死。对比：`withdraw_deposit` 成功路径有测（guards.rs
  L35–38）。建议任务 10–14 或 13 补一条 2–3 行断言。
- **F2**：`assess_credit` 差额为零分支（`Ok(None)`、空批过账）未测。已核
  `post_batch` 显式接受空批（mod.rs L75–79「空批为合法 no-op」）——路径
  正确，仅缺测试钉死。
- **F3**：贷款**越过合同到期日**继续按合同利率计息（逾期续计息的登记简化）
  无显式测试（gold 全程在到期前；存款侧有到期停息对照测试）。行为是刻意的
  不对称（存款停息/贷款续息），值得一条测试固定该语义选择。
- **F4**：`deposit_account` 365 天整边界（≤365 → 2011）未测（现有 181 天→2011、
  546 天→2601 两点）。

复杂度评估：11 文件布局与任务 8 industrial 同构、职责切分清晰（状态/数学/
处理器/计量分文件）；interest.rs 232 纯行、gold.rs 284 纯行均在登记例外
（warning band / 单场景金样，同 task-8 chain_gold 298 先例）。ACT/365F 第四
副本为结构性必然（industrial 原件 pub(super) 私有 + 本任务禁改 industrial，
learnings「直接复用」指令与「禁改」冲突，worker 按 rhe_div 孪生先例复制并
交叉引用注释 + issues 登记）——接受，任务 13 统一入口再议。无跨层语义漂移：
代码/测试/文档/issues 四处的科目码、单位（分）、ECL 口径一致。

## 4. 登记偏离裁定（worker issues.md W2-Task 9 第 1–8 条）

| # | 偏离 | 裁定 |
|---|---|---|
| 1 | company/mod.rs +1（staging 清单外） | 接受——必要最小接线，同构先例 |
| 2 | 简化清单（ECL 不折现、EAD=账面、单利、逾期合同利率、提前提取原利率、到期停息、平价⇒合同=实际利率、银行税未接、手续费仅收入侧） | 接受——逐条有 CAS 锚 + issues/learnings/ecl.rs 头注三处登记；「不折现」显式注明不虚构贴现假设（防御式：缺参数即不造假） |
| 3 | interest.rs 232 纯行 | 接受（<250 天花板内，且登记了拆分预案） |
| 4 | gold.rs 284 纯行单测试 | 接受（同 task-8 例外；跨文件拆同一场景是人为切割） |
| 5 | ACT/365F 第四副本 | 接受（结构性，见 §3） |
| 6 | 未复用任务 8 子账代码本体（自建 DepositState/BankLoanState） | 接受——存贷开项语义确与贸易开项不同；复用的是模式（不变量）而非代码，边界诚实 |
| 7 | clippy 新代码 0 警告 | **属实**（见 §5） |
| 8 | 全量 660 含并行任务增量 | 属实（§0 对账一致） |

另：worker 自行勾选 plan checkbox 早于本复核——按门禁规定以本文件为准，
不影响裁定。

## 5. clippy 裁定（4 个 doc_lazy_continuation 传闻）

隔离树 `cargo clippy -p engine --all-targets`（exit 0）实际输出：
- lib：1 警告 = `behavior/decision.rs:239` unnecessary_filter_map——任务 1 起
  登记、task 42 所有，**既有缺陷，不裁给本提交**（指令明示）。
- tests/analysis_profiles/invariants.rs：3 警告 unusual_byte_groupings——
  任务 17 文件（本提交文件集之外），既有，不裁给本提交。
- **company/bank/{ecl,loans}.rs 及本提交全部文件：0 警告**。

裁定：task-19 worker 02:50 在共享树观察到的 4 个 doc_lazy_continuation 警告
属其中间未提交状态（提交在 02:53）；提交内代码无此警告，worker 自己的
「新代码 clippy 0 警告」主张对提交状态**属实**。无需修复、无需后续动作。
（analysis_profiles 3 警告为本次复核新观察到的既有问题，归属任务 17 文件，
不属本提交，如实记录备查。）

## 6. 证据链

- 红（02:21，实现前）：task-9-red.txt——模块不存在，10 个编译错误 ✓ TDD 成立
- 绿：task-9-happy.txt 11/11；失败面：task-9-failure.txt 7 过 + 4 filtered
- 全量：task-9-full-suite.txt（共享树 660/0/4）= 本人干净树 660/0/4 ✓
- 交叉验证：task-9-19-verify.txt（technical_memory 33/33 独立复跑一致）
- learnings.md W2-Task 9 条目（模块布局/金样锚/已修 bug：到期停息
  last_accrual 推进到有效截止）与代码一致；issues.md 8 条登记完整。

## 7. 残余风险与后续

- F1–F4 覆盖缺口已按 task-8 复核先例追加登记至 issues.md（非阻塞，供任务
  10–14/13 顺手偿还）。
- 会话接线（任务 26）与经营前史生成（任务 14）不在本提交范围，`BankBooks`
  未接 session——架构方向正确。
- 隔离 worktree 保留于 `C:\Users\msuad\AppData\Local\Temp\opencode\wt-review-9b`
  供追查；主仓零写入（除本复核产物）。

**最终裁定：APPROVE。**
