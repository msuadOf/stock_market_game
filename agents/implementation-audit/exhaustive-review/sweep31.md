# Sweep31：Task4/7/8 独立复核记录与当前生产调用链

日期：2026-10-03。审查产品基线 `b76ece3`；当前合并工作树涉及 company/accounting/calendar/session 源码对该基线无差异。已沿用本 agent 读完的根 AGENTS/principles，全文连续读取 `.omo/evidence/company-information-npc-intentions/task-4-review.md` 22 行、`task-7-review.md` 19 行、`task-8-review.md` 162 行，共 203 行。三份实际 verdict 均为 APPROVE，没有 REJECT；不能虚构“历史拒绝已修复”。只新增本记录，无产品/Git 写操作，无编译、测试或长验收。

## Task4：逐条核对（22 行全文）

| 原文行号与要求 | 当前状态与证据 |
|---|---|
| 3–6：独立 reviewer、历史 commit、10 项 calendar 测试 | 历史证据，未在本轮复跑；不据此宣称当前测试通过。 |
| 9：CivilDate 闰年、星期和整数日期算法 | 当前 `packages/engine/src/calendar/date.rs` 保留 CivilDate 与 Hinnant 整数换算；`:328` 开始 days_from_civil。未发现代码被替换为墙钟/网络日期。 |
| 10：1998/1999 共 504 个 prehistory 交易日 | 历史已验收断言；现行 frozen policy 仍覆盖 init-only 1998，不能把旧数量直接用于新政策。 |
| 11：HKO 表与来源 | `calendar/data/mod.rs:23` 记载双语取证，`:52` 保存缺陷说明，`:254` 2051 农历 anchors。原 reviewer 的在线取证是历史事实，本轮没有重新联网。 |
| 12：restore-wins 与三个 digest | `calendar/policy/validation.rs:14` 先内层 fallback.validate，再 outer digest，`:24` 重新从 spec 装配；`session.rs:2801` 附近重建存档 calendar。不能仅用当前默认表覆盖已冻政策。 |
| 13：fallback 规则、调休周末仍休市 | `calendar/holidays.rs` 先周末判断，`:94` simulated_holiday_kind；政策表是日历输入，不把调休周末当交易日。 |
| 14：运行窗、init-only floor、四种 year labels、无 notice 不标 Official | `calendar/policy/mod.rs:148` year label，`:167` NoticeTextUnverified；`policy/validation.rs:27` bounds、`:47` coverage、`:68` facts 覆盖。生产保留显式 policy 校验。 |
| 15：纯逻辑、无依赖/网络、当时 scope | 历史 diff 的限制，不外推当前整个模块永久 ≤250 行或永远无 expect。未发现 calendar 的运行网络依赖。 |
| 18：1970-01-00 注释 typo | 仍在 `calendar/date.rs:328`，文漂，非日期算法错误。 |
| 19：2051 defect prose 不一致 | 仍在：`calendar/data/mod.rs:23` 说英文文件独有，`:52` 常量说 both en and tc；星期列不消费，不能升级为交易日算错。 |
| 20：`.ok()?` 的 gap year 理论风险 | `holidays.rs:97` 仍在，`policy/validation.rs:68–75` 对整个允许窗先检查 facts；就该合法装配调用链，不登记为静默坏日历。 |
| 22：APPROVE | 当前逐项以上边界；历史批准不等同于本轮重新实测。 |

## Task7：逐条核对（19 行全文）

| 原文行号与条款 | 当前状态与生产证据 |
|---|---|
| 3–5：独立 reviewer 与历史 commit | 历史元数据，保留。 |
| 8：公司资金/投资者资金隔离；opening 外部 Lender 仅状态 | `company/mod.rs:96` Company 的 Books/counterparties/contracts/budget；`:119` opening 仅 voucher post。`session/company_assembly.rs:67–102` 分别装配 registry/operations，不向 AccountBook 打款；日终 `session.rs:2105` 经营与投资者账户分层。 |
| 9：issuer 精确股数，不按股价造股本 | `session/company_assembly.rs:70–86` 股票精确匹配及 issuer_pairs；`company/mod.rs:222` validate_issuer_mapping 校验股本。当前 StockSpec total_shares 已是 u64，与 review 的 8,928,571,429 股单位相容。 |
| 10：voucher→post_batch、opening 拒绝原子、event 1 | `company/mod.rs:118–125` to_voucher/post_batch/map OpeningPost；工业 `industrial/mod.rs:78–117` 独立开局校验及 next_event_id=2。Company 不依赖 Session。 |
| 11：数据/授信/恰好满额/拒绝无变更 | `company/mod.rs:142–172` 校验合同及 per-counterparty 借款合计；`company/contracts.rs:125–136` 指定贷款人过滤，`:158–193` 多贷款人 credit_lines 显式建模。工业实现的跨贷款人漂移见新候选 N31-1。 |
| 12：历史测试、文件结构和纯逻辑约束 | 历史测试结果未复跑；不能据此核销下游 caller。 |
| O1，15：CompanyRegistry serde 重新 validate_set | 通用 Company/Registry 仍 derive Deserialize（`company/mod.rs:95` 与 registry 结构），但当前产品 SaveSlot 恢复并不直接接受该 registry：`session.rs:2716` restore 经 validate_save_slot，新建局重建 registry，`session/company_assembly.rs:79` 调 CompanyRegistry::new，`:86` issuer mapping；`session.rs:2929–2940` 校验经营公司集合。原公共 serde 边界建议仍保留，不能据此声称产品现行 registry 已绕过 validate_set。 |
| O2，16：开局 2001 没有合同 | **工业域已修正**：`industrial/mod.rs:89–103` opening reconciliation，`industrial/config.rs:160–197` 精确对账 2001、构造 OPENING-DEBT 并登记 LoanPortfolio，包含计息起点与 maturity。不能重开“所有开局债完全无合同”；借款付息还本生产调度仍 G35。注册表壳本身不负责真实经营借款。 |
| O3，17：first-match issuer find | `company/mod.rs:222` 保留映射查询，上游 setup/CompanySpec::validate_set 维护唯一；未发现新的合法重复 issuer 调用链。 |
| 19：APPROVE | 不外推工业经营闭环已经完整。 |

## Task8：按全部章节核对（162 行全文）

| 原文章节/行号 | 当前实现与结论 |
|---|---|
| Verdict/Scope，1–9；Commands，13–29 | 历史 APPROVE、当时完整 diff 与 29/614 测试等为历史证据。本轮未复跑。journal 当时冻结不是当前工业语义标签债已偿还的证明。 |
| Q1 CAS14，37–39、64 | `industrial/sales.rs` sell_credit 确认收入；collect 用 BANK/AR，生产 `operations/industrial.rs:118` sell_credit、`operations/dispatch.rs:76–84` 到期回款。真实 caller 在，不重复把回款计收入。 |
| CAS22 ECL，40–42 | `industrial/sales.rs:177–208` 用 open total 目标准备与 ledger 已提余额差额；`:214–250` 核销 guard+清应收。生产 `operations/industrial.rs:174–176` 调 allowance；“坏账核销标签是回款”仍有后续债 N31-2。 |
| VAT，43–46、65–66 | `industrial/purchasing.rs:75–99` full/deductible/non_deductible 守恒且不可抵扣入存货；`accounting/tax.rs:107–115` split 两步明确。不是收入/现金混用。 |
| 税法 blocked，无默认构造，47–49 | TaxPolicy 无 Default 并显式校验。现行生产 `session/company_assembly.rs:262–275` **已有显式版本化 13%/25% 游戏假设**，历史“生产无税率”不再成立；源码同时称“现行标准税率”，在法源 blocked 仍应与 policy 状态一致，不把取证债当无生产参数。 |
| 移动加权、折旧等游戏简化，50–53 | `accounting/inventory.rs:102–145` checked 比例发出及差额结存；`fixed_assets.rs:36–60` 剩余基础/寿命与半偶舍入；所得税 `tax.rs:153–218`，无真实合规宣称。零折旧的行业过账新边界见 N31-3。 |
| 手算库存守恒，57–59 | 最后一批 preview_issue 分配剩余成本，随后 quantity/total_cost 减相同值；无重新从浮点均价推算。 |
| 手算折旧/减值，60–63 | FixedAssetEntry 基础=cost-salvage-累折-减值，剩余月数=life-depreciated；减值共同残值下限 `fixed_assets.rs:64–93`。纯子账公式成立，但不能证明 IndustrialBooks 零金额过账成功。 |
| FIFO/DTA，67–71 | `tax.rs:161–205` 先过期、按 origin 排序、弥补、剩余亏损计算。`industrial/expenses.rs:127–192` 预览→必要分录→提交 loss_pool；重复年度调用的现有非幂等行为已在 ownership_tests 明示，不因为有新 owner 就核销（生产调度 G35 待接）。 |
| ACT/365F 余数，72–76 | `industrial/loans.rs` 状态/preview_accrual；`industrial/interest.rs:106–131` 预览全合同→post→apply；零金额正天数 items 仍推进日期/余数。 |
| PaymentFailed/Overdue，77–79 | `operations/industrial.rs:61` 只把 PaymentFailed 变业务记录；`:99`、`:156`、`:166` 分别处理生产费/补货/管理费，其他错误上抛。无补钱/透支。应付支付公共处理器存在，但默认流现仅现金采购，不凭测试开放完整信用采购调度。 |
| Q2 scope，83–92 | 当前拆分为 owners 不改变 post_batch、分行业 Books；历史冻结与文件数量不是当前功能完成标准。 |
| Q3 failure/success gold，98–115 | 源码 validate→post→apply 一般路径保留（`industrial/mod.rs:189–196` post_with_commit）；不能用历史金样覆盖未测的新增候选。 |
| Q3 显式错误/available_credit，116–119 | `.ok()?` 在 `industrial/mod.rs:178–181` 仍为只读现存设计债；borrow 权威算术错误显式传播，**但 per-lender 算法不同于旧 O5 的溢出→None 问题**，见 N31-1。 |
| O1，122–123：零加工费成功覆盖 | 生产 `industrial/production.rs:78–92` 分支存在，零加工费不添现金行；后续都采用零加工费配套标签为 Depreciation，正确过账与标签债分开。未运行分支测试，不能宣称通过。 |
| O2，124–125：income_tax 空行覆盖 | `industrial/expenses.rs:174` 空行不造 JournalEntry；`industrial/ownership_tests.rs:199–207` 已补 zero_line_tax_still_commits_expired_loss_pool。状态提交的实际代码存在，不只看测试。 |
| O3，126：同日还本 | `industrial/repayment.rs:85` preview_repayment_accrual 返回无应计项则只还本；`industrial/ownership_tests.rs:249` 同日还本测试已补。 |
| O4，127：零金额正天数利息 | `industrial/interest.rs:116` 仅正金额形成分录，`:128–131` 所有 items 仍 apply。因此零应计额路径实际修正/保留，不误报欠实现。 |
| O5/O6，128–130；132：只读溢出/同算法舍入副本 | 历史非阻塞取舍保留。源文件有重复 helper 不等于新会计错误；本轮不强制无需求重构。 |
| Deviation 1/2，140–141：文件拆分、金样长度 | 历史接受的工程例外，不重开功能缺口。 |
| Deviation 3，142：BusinessKind 行业标签后续义务 | 后续新增银行/保险/地产枚举，但工商赊购/生产/核销/资本开支仍用错义标签；N31-2 有当前生产凭证 caller。 |
| Deviation 4，143：注册简化 | 单事件生产、费用现付、折旧 ADMIN、DTA 全确认等保留已登记游戏简化；不因真实制度差异自动开新 implementation 待办。 |
| 145–146：历史 clippy 警告 | 旧版本工具链记录，未复跑不宣称现在通过/失败。 |
| Architectural invariants，150–156 | post_batch 唯一过账实际存在；opening debt 精确对账修复可见 `industrial/config.rs:173–197`，`:183` OPENING-DEBT；LoanAccount 固定开局 ST 借款由工业 repayment 对该 contract 特判，不以 maturity 静默改为 LT。 |
| Residual，158–162 | 历史分支测试有些已补，标签债仍在，税率已有显式生产假设；残余结论不能照抄。 |

## 工商期末生产 caller：G35 保留

完整生产主线：`session.rs:2105` 的自然日日结推进 CompanyOperations，`company/operations/day.rs:79–85` 依次冲击、dispatch_due、行业日流、次日计息；`:249` 实际分派工业。`operations/industrial.rs:85–176` 执行生产、赊销、现金补货、管理费、坏账目标；`operations/dispatch.rs:41–42` 接利息计提，`:76–84` 接 AR 到期回款。`day.rs:198–201` 继续安排滚动利息。

全源码搜索 depreciate_month/accrue_income_tax/pay_interest/repay_principal，除定义及 tests 没有工商 CompanyOperations 的生产调用。期末封账/披露有 owner 不能代替折旧/所得税/商业债付息还本，因此原 G35 未修复，不能新增重复整项。开局固定资产、隐式合同与真实计息已有，也不能反过来说“没有任何计息/公司经营实现”。

## 新候选与反证

### N31-1：工业 per-lender 授信被全部贷款人的未偿余额污染

原文 Task7 `:11` 要合同/credit surface 数据完整；当前明确契约 `company/contracts.rs:140–144` CreditLine(lender,limit)，`:158–183` 可接受多个不同贷款人，`:191` 指定贷款人上限；Company 通用路径 `company/mod.rs:151–153` 用 outstanding_borrowings(counterparty)。工业 `industrial/interest.rs:56–58` 却取 `loans.outstanding_total()` 与指定 lender.credit_line 比较；`industrial/mod.rs:178–181` available_credit 同样总贷款余额。`industrial/loans.rs:99–103` 没有 lender 过滤。

静态最小例：贷款人 A/B 各限额 1000，先向 A 借 1000，然后向 B 借 1；合法 independent credit line 被 IndustrialBooks 拒绝，B 可用额度也误报 0。无显式公司总负债上限字段/约定支持此合并。此项不同于 O5 的 checked 溢出→None，也不同于 G35 生产调度缺失。

反证：默认局目前一个外部 lender 且日流不主动 borrow，所以不声称默认玩法已遇到跨 lender 拒绝。公共 IndustrialBooks API 与合法配置边界仍确有错误，应由总审计独立确认并补按合同 counterparty 归集当前 LoanState.outstanding，而非直接使用未核销 contract 原本金。

### N31-2：Task8 明确后续义务的工商 BusinessKind 标签债仍未偿还

历史 `task-8-review.md:142` 接受借用通用标签但要求后续行业枚举与存量迁移评估；`:160–161` 明确残余债。当前 `accounting/journal.rs:61` 已含银行/保险/地产专用变体，工业仍如下：赊购 `industrial/purchasing.rs:89–92` CreditSale；生产 WIP→成品 `production.rs:103–107` Depreciation（默认日流真实调用）；ECL 准备/固定资产减值 `sales.rs:201`/`capex.rs:115` Depreciation（默认日流真实 allowance/冲击 caller）；核销 `sales.rs:242` ReceivableCollection；资本开支 `capex.rs:47` CashExpense。

反证：kind 本身不驱动过账，现金流 Investing/NonCash 及正确借贷仍在，所以不声称账目金额或现金流已经算错。缺口是审计凭证业务分类与后续义务遗漏。默认日流实际产生 Production/ECL 的错误分类，非只有无人调用的方法；资本开支/核销部分仍为公共 handler 边界。建议总账新增或并入已有凭证分类债，不回退银行/保险正确分类，不扩大真实制度支持。

### N31-3：合法零月折旧金额导致行业过账拒绝，月份无法推进

原文 Task8 `:60–63` 要剩余寿命半偶舍入守恒，`:143` 允许登记简化；公共资产政策 `accounting/fixed_assets.rs:148–162` 正成本且残值可等于成本，算法 `:46` 半偶舍入也允许 0。例如成本1分、残值0、寿命3月，首月 rhe(1/3)=0；或成本=残值任意正额。

工业 `industrial/capex.rs:66–84` 只判断 remaining_months>0，无条件给 0 amount 生成两行 JournalEntry；`accounting/journal.rs:222–228` 拒 NonPositiveLine。`capex.rs:92` post 失败，`:94` apply_depreciation 不执行，于是合法登记资产的月寿命不能推进。正常处理应保留零金额月份的状态推进而无零金额分录；不能弱化 JournalEntry 正额铁律。

反证：纯 FixedAssetRegister::apply_depreciation `fixed_assets.rs:55–60` 支持0且推进月数；普通大额金样全是正额，证明不了工业封装合法。当前 G35 尚没生产月调度，因此本轮不声称默认局已经崩溃；该公共工业 handler 缺陷独立于“调度没有接线”。查到的 assets_gold/subledgers 测试只覆盖正额/非法政策，未见零额工业折旧用例；未执行测试。

## 边界说明

税法 blocked 是取证债；现行显式游戏税率已存在，不恢复“没有生产税率”的历史结论。通用注册表 serde 建议不得冒充当前 SaveSlot 接线已坏。Task4 两项 doc typo/prose 不升级为交易日功能缺陷。Task8 重复年度所得税未幂等已有 ownership test 明示，接期末时必须明确执行次数；本轮只挂在 G35 边界，没有依据把直接重复公共调用必然提升为独立新产品缺口。

本批确认 G35 仍缺、Task7 O2 工业开局债合同已修正、O2/O3 部分分支覆盖已补；提交三个新候选 N31-1/N31-2/N31-3 给总审计复核去重。
