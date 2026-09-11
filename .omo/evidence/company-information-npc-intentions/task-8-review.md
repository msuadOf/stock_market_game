VERDICT: APPROVE

# Task 8 Independent Review — 工商经营与营运资金会计（commit 91a8d55）

Reviewer: independent subagent (AGENTS.md 大A语义与独立复核门禁), did NOT implement this change.
Scope reviewed: complete diff of 91a8d55 (28 files, +5109 lines, 0 deletions), branch
`codex/feat/web-ui-polish`, repo root `D:\workplace\stock_market_game`. Worktree at HEAD ==
91a8d55 with only untracked artifacts (`.omo/`, `apps/web/src/types/generated/*.ts`), so the
test runs below exercised exactly the committed code.

---

## 0. Commands re-run by this reviewer (actual exit codes + counts)

| # | Command | Exit code | Result |
|---|---------|-----------|--------|
| 1 | `git show 91a8d55 --stat` | 0 | 28 files, +5109/−0; **no `accounting/journal.rs`** in the diff (frozen zone untouched; task-8 edit ban honored) |
| 2 | `git status --porcelain` + `git diff 91a8d55 --stat` | 0 | tracked tree identical to commit; only untracked artifacts |
| 3 | `cargo test -p engine --test industrial_accounting` | **0** | **29 passed; 0 failed; 0 ignored** (matches task-8-happy.txt) |
| 4 | `cargo test -p engine` (full) | **0** | **614 passed; 0 failed; 4 ignored** across 24 suites (81+25+21+24+21+35+10+10+15+3+31+8+7+3+29+20+21+10+18+33+5+112+72+0; the 4 ignored are the registered session stress gates) — matches task-8-full-suite.txt |
| 5 | `cargo check --workspace` | **0** | no warnings, no errors (cross-crate sanity incl. apps) |

Raw outputs: `%TEMP%\opencode\t8r-1.txt`, `t8r-2.txt`, `t8r-3.txt` (cmd /c redirect per PS 5.1 quirk).

Read in full: `accounting/{inventory,fixed_assets,receivables,tax}.rs`, all 12 files of
`company/industrial/`, `accounting/mod.rs`/`company/mod.rs` wiring diffs, all 8 test files under
`tests/industrial_accounting/`, `accounting/journal.rs` (frozen-zone contract, read-only),
task-8-{happy,failure,full-suite}.txt, `docs/company-accounting.md` §2.1/§2.2/§7,
`tests/fixtures/company-model/policy-sources.json`, notepad learnings/issues §W2-Task 8.

---

## 1. Q1 — 是否符合大 A 语义、依据是否可靠？ **是**

Policy anchors (cross-checked against `docs/company-accounting.md` + `policy-sources.json`):

- **CAS 14（2017）§4/§13**（verified-official，财会〔2017〕22号）→ 赊销履约时点一次确认收入；
  回款只 `Dr 1002 / Cr 1122`，收入科目余额在回款前后被逐断言相等（chain_gold L140/157、
  collection.rs L53–67）——**不重复计收入**。赊销分录不含任何现金科目行——**不动现金**。
- **CAS 22（2017）§63**（verified-official）→ 坏账准备 = 未结应收 × 政策比率的**整个存续期
  ECL 简化法**（`ecl_allowance_target`），差额计提/冲回（Dr/Cr 6701↔1231），核销要求准备
  ≥ 开项余额，全程非现金。与准则条文口径一致，且在模块头注明确"简化选择而非准则缺失"。
- **财会〔2016〕22号**（verified-official）→ 价外税模型：销项 222101 / 进项 222102 分开核算；
  `split_input_vat` 把不可抵扣进项归集进存货成本；应纳 = 销项 − 可抵扣进项，进项富余
  自然留抵 222102 借方（不模拟退税——已登记简化）。净负债口径以进项借方冲减（财会22号
  净额列示）。分录平衡核验：存货(货款+不可抵扣) + 进项(可抵扣) == 现金/应付(货款+全税)。
- **vat-law-current / cit-law-current 均 blocked** → `TaxPolicy` **无默认构造器**，测试用显式
  构造并逐处标注 "Fixture 合成税率，不声称真实参数"（main.rs L55–70）。这是 §7 登记的
  sanctioned workaround，被严格遵守——无任何硬编码"真实"税率。
- **移动加权平均** = `game-assumption-inventory-method`（simulated-game-assumption，CAS 1
  blocked）——实现与登记一致，不虚称准则原文。
- **CAS 4/8/18/31 原文 blocked** → 直线折旧/减值/所得税/现金流分类均为文档化游戏简化，
  头注均声明"不声称其原文依据"；付息还本归筹资、资本开支归投资与 CAS 31 常识口径一致。

语义 spot-checks（全部独立手算复核通过）：

1. **移动加权守恒**：发出成本 = rhe(结存成本×发出量/结存量)，结存按差额结转 →
   Σ发出 + 期末 == Σ入库 分毫不差；末批发出量 == 结存量时精确带走全部剩余。
   手算：3件100分+2件350分 → 发4件 = rhe(450×4/5)=360，余1件90分；再发1件=90，归零。
2. **直线折旧 rhe(剩余基础/剩余月数) 守恒**：成本1000元/残值2分/3月 → 基础99_998分 →
   33333/33332/33333（半偶舍入 tie 两次独立验证），Σ == 99_998，账面 == 残值。减值后
   重定基：999.98−333.33−100 = 566.65 → 28332/28333，Σ折旧+减值 == 成本−残值。末月
   months==1 时 rhe(base/1)==base 精确清零——结构性守恒成立。
3. **赊销不加现金、回款不重复收入**：见上（CAS 14 锚点 + 测试断言）。
4. **价外税销项/进项拆分**：销项 = 不含税基数×税率；进项拆分 deductible +
   non_deductible == full（sub 恒等）；50% 抵扣比例金样 65/65 验证。
5. **亏损 FIFO + DTA 全额确认**（registered simplification）：到期出池（year−origin >
   carryforward）先于弥补；FIFO 按起源年；DTA = 期末池×税率，利润年差额转回。
   手算：亏1000 → DTA 250；盈6000 → 弥补1000、税5000×25%=1250、DTA 转回250；
   全周期净利 = 8000−2000−1000−1250 = 3750 == 测试值。简化可接受性：CAS 18 原文
   blocked + 不设确认门槛/折现已在税模块头注声明，游戏口径自洽且不虚报合规。
6. **ACT/365F FractionUnits 余数守恒**：interest = rhe((cents×bp×days + carried)/3_650_000)，
   remainder = scaled − paid×3_650_000 → Σpaid×3_650_000 + 终余数 == Σ(cents×bp×days)
   恒成立。手算：200000分×400bp×31天 → 679分 + 余1_650_000（2_480_000_000 精确）；
   2月续提 (200000×400×28 + 1_650_000)/3_650_000 → 614分 + 余550_000（跨月余数衔接
   验证）。LOAN-2 六天 → 164分 + 余1_400_000。
7. **PaymentFailed/Overdue 不补钱、不透支、不崩溃**：`map_post_error` 把 K2 批末负现金
   禁令包装为类型化 `PaymentFailed`；guards.rs 验证拒绝后公司继续运行（下一笔合法费用
   照付）、应付保持开项并进入 `payables().overdue()` 逾期面、现金余额逐分断言不变。

---

## 2. Q2 — 是否为需求所必需、保持最小范围？ **是**

- **纯增量 diff**：4 个新共享子账模块 + `accounting/mod.rs` +13 行（mod 声明 + pub use）、
  `company/mod.rs` +4 行（mod industrial + 再导出）、12 个 industrial 文件、10 个测试文件。
  **零删改、零重构、零无关测试改动、零 drive-by**。无 churn。
- **journal.rs 语义冻结区未动**（--stat 无该文件；`BusinessKind` 仅作标签消费，未扩枚举、
  未改过账逻辑——与 issues.md 登记一致）。
- 科目表 v2 = v1 全集 + 工业扩充，不改 v1 语义（任务 6 冻结面）。
- 每个处理器都是任务 8 计划范围内的经营事件（采购/生产/销售/回款/费用/税/借款/计息/
  付息/还本/资本开支/折旧/减值/坏账），无越界功能。

---

## 3. Q3 — 遗漏边界测试 / 跨层语义漂移 / 不必要复杂度？

**强项**
- 负向矩阵完备：15 个 failure 用例，每条拒绝路径类型化 + `assert_eq!(co, before)`
  **字节级状态不变**断言（含 PaymentFailed、InsufficientInventory、OverApplication、
  ItemCleared、InsufficientAllowance、DebtBeyondCreditLine、NoCreditLine、
  AccrualNotForward、NothingAccrued/NothingToPay、PrincipalBeyondOutstanding、
  TaxOverpayment、InvalidRateBp、OpeningSeedMismatch/OpeningDebtMismatch/
  OpeningAccumulatedDepreciation、TaxPolicy/AssetPolicyInvalid、DueBeforeOpen/
  DuplicateItem、非平衡开局行透传 Accounting）。`next_event_id` 拒绝不消耗——由状态
  字节不变隐式锁定。
- 金样断言手算数而非仅借贷平衡：终态现金 20451.21（2_045_121分）/ 净利 3991.57 /
  净负债 9859.64 / 权益滚动 13991.57；还本后现金 12801.21 / 权益 13985.43 / 净负债
  2215.78；31 天利息 679 分 + 余数 1_650_000；现金流三分类逐期间对账（1月经营+6458/
  筹资1993.21/投资0；期初筹资12000）；对手方净额与流水数；授信容量 1000→3000 释放；
  serde 整账套往返相等。**本审全部独立重算通过**。
- 跨层对账：开局存货/资产种子与总账逐科目精确对账（漂移 → 类型化拒绝，不许隐性差）；
  生产后 WIP 清零；VAT/坏账准备/DTA 已入账余额一律读总账 `net_of`，无影子计数器。
- Task-21 教训核查：18 个公开处理器的**主成功路径**全部有金样真实触达；未发现
  "守卫拒绝使成功路径不可达"的结构。
- 无静默吞错：无空 catch（Rust 无 catch）、无 `?? 默认值` 掩盖；`expect(...)` 仅用于
  "前一验证刚确认存在"的内部不变点（与仓库先例一致）；`map_post_error` 透传非
  负现金错误。唯一可议点：`available_credit` 对溢出以 `.ok()?` 归 None——只读派生
  访问器，权威守卫路径 `borrow()` 独立重算并类型化传播，可接受（见观察 O5）。

**次要观察（非阻塞，已按指令 append 到 issues.md）**
- O1 `production.rs` L78–92 零加工费成功分支（Depreciation/NonCash 纯结转标签）无金样
  触达（chain_gold 用 1000 元加工费；inventory.rs 的 produce 全部在过账前被拒）。
- O2 `expenses.rs` L171–187 `accrue_income_tax` 空行 → `Ok(None)` 无分录路径未测
  （两个金样年都有分录）。
- O3 `repayment.rs` L85–108 还本日 == 上次计提日（days==0，单张还本分录）路径未测。
- O4 `interest.rs` L124–135 零金额正天数计提（推进余数与日期、无分录）路径未测。
- O5 `mod.rs` L171–181 `available_credit` 溢出 → None（`borrow()` 权威路径类型化，见上）。
- O6 `rhe_div` 三副本（amount.rs 冻结原件 + inventory 共享 + loans 私有）：带交叉引用
  注释的同算法孪生，冻结区约束下的合理选择；任务 13 统一入口时再议（learnings 已记）。

O1–O4 属二线分支测试增强建议，不影响正确性结论；O5/O6 为登记在案的设计取舍。

---

## 4. Registered deviations adjudication（issues.md §W2-Task 8 逐条裁定）

| # | Deviation | 裁定 | 理由 |
|---|-----------|------|------|
| 1 | company/industrial/ 12 文件 vs 计划字面 7 | **接受** | 250 纯逻辑行天花板是仓库规则；拆分沿"状态/数学半边 vs 处理器半边 vs 数据表/守卫"职责缝（loans=状态+ACT/365F 数学、interest=借入/计提、repayment=付息/还本、chart=科目表数据、config=种子对账守卫、error=统一错误面）；task-7 的 7-vs-5 + error.rs 先例成立；interest 合并版 403 行 / mod 合并版 320 行确实超顶。 |
| 2 | chain_gold.rs 298 纯行（单场景例外） | **接受** | 全链金样的价值恰在"单测试函数内 期初→采购×2→生产→赊销→回款→借款→计息→付息→结应付→还本 + 终态对账"的连续性；跨文件拆同一场景是人为切割。与 task-1/2/21 已登记的单文件测试契约张力同类，issues.md 已给出后续扩样时的拆分预案（按事件段拆 chain_gold/ 子模块）。 |
| 3 | BusinessKind 行业变体欠账：赊购=CreditSale/NonCash、生产结转=Depreciation/NonCash、核销=ReceivableCollection、资本开支=CashExpense+Investing CF | **接受（有后续义务）** | journal.rs 属任务 6 语义冻结区且本轮两度禁改；journal.rs:47 自文档"分类标签，不驱动过账逻辑；行业事件在任务 8–11 扩充此枚举"——本轮以最近通用标签记录是该冻结约束下的唯一合规做法。语义漂移风险被三重缓解：(a) kind 不驱动任何过账；(b) CF 类别承载投资语义；(c) 完整映射表钉在 industrial/mod.rs 头注 + issues.md。欠账已显式登记给任务 9–11/13（含存量存档标签迁移评估——纯数据演算无会计风险）。审阅者提醒：赊购打 CreditSale 标签语义刺眼，任务 9–11 扩枚举时应优先偿还。 |
| 4 | 已登记简化：单事件生产（无 WIP 跨日/制造费用科目）、费用现付（无应付职工薪酬 accrued 循环）、折旧全归 6602、减值后剩余寿命不变、DTA 全额确认、进项富余只留抵、开局累计折旧诚实拒绝 | **接受** | 每条都满足宪章要求"游戏尚未实现的真实规则必须明确标注"：(a) 源码头注 + docs/company-accounting.md 状态列 + issues.md 三处一致登记；(b) blocked 依据（CAS 1/4/8/18 原文未取得）如实标注，不虚称准则原文；(c) 简化内部自洽——如减值不重估寿命在 fixed_assets.rs 头注声明且守恒在新基础下继续成立；开局累计折旧以 OpeningAccumulatedDepreciation 类型化拒绝（诚实边界）而非静默接受或伪造，前史归任务 14 同一处理器生成（语义同一性由构造保证）。 |

issues.md 第 5 条（clippy 既有缺陷 behavior/decision.rs:239，任务 1 起登记归任务 42）按
指令不作为拒绝理由；本轮 `cargo check --workspace` 0 warning 佐证新增代码无新增告警。

---

## 5. Architectural invariants (tasks 6/7) re-verified

- `Books::post_batch` 是唯一过账入口：全部处理器经 `post_with_commit`（validate → post →
  apply；`next_event_id` 仅成功后推进）；拒绝 → 账套与子账字节不变（测试逐一断言）。
- OPENING-DEBT 隐式合同与 2001 贷方余额**精确**对账（不匹配 → OpeningDebtMismatch）；
  授信容量 = 限额 − Σ未偿（含开局），金样 5000−2000−2000=1000 及还后 3000 验证；
  `loan_account_of` 对 OPENING-DEBT 恒 2001（learnings 记录的期限推导误选 2501 已修）。

## 6. Residual risk

无阻塞风险。遗留：(a) O1–O4 分支覆盖增强建议（已 append 到 issues.md）；(b) 任务 9–11/13
的 BusinessKind 枚举扩充与标签迁移欠账（已登记）；(c) blocked 税法取证解除前生产端
无默认税率（设计使然，非缺陷）。
