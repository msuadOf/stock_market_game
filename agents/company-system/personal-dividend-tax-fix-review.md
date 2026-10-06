# 个人现金分红税门禁修复批次复审（非作者，2026-10-06）

复审人：未参与实现的独立 reviewer（门禁复审）。复审对象：提交 f1fc21f6 门禁复核 10 项 finding 的修复批次，即当前工作区未提交 diff（11 个修改文件 + 2 个新增 ts-rs generated 文件；`packages/engine/src/company/mod.rs` 的 1 行 re-export 为既定保留项）。方法：只读逐行审阅 `git diff` 全量、抽读修复后源码上下文、对照财税〔2012〕85号 / 财税〔2015〕101号官方原文（WebSearch/网页原文）、独立复跑修复者声称的短测命令。未修改任何文件。

## 结论

**通过（pass）**。10 项 finding 全部被正确修复，无 blocker/major 残留；另有 1 项 minor（M4 文档引用精度）与 4 项 note，均不阻塞提交。门禁复核另发现的既有缺陷（`collect_due` 幂等比较）亦已修复。

## 逐条核验

- **M1（历史税额加回 / 资金上限虚增）— 通过**。`settle_dividend_tax` 中 `collected_tax` 折叠与 `base_available_cash` 加回已整体删除，`collect_due` 直接收 `accounts.get(&account).cash()` 真实现金；`collect_due` 幂等重放比较已改为 `existing.available_cash == available_cash`（即门禁另发现的既有缺陷，随本批修复）。会话用例 `insufficient_cash_partial_collection_and_next_day_end_chase` 真实构造了"第一日历史收缴 200 分 + 第二日应纳 10 分但现金仅 3 分"场景：断言部分收缴 3、`available_cash == 3`、`outstanding == 7`、现金归零且不为负，次日补足后追缴 7、现金 13。红→绿证据（`gate-fix-red-session.log` 18:47 以修复前实现复现红）与本人复跑均确认。
- **M2（部分收缴语义统一 + typed 查询面 + 文档不实）— 通过**。部分收缴 + 后续日终追缴已统一（`min(应纳税额, 真实现金)`）；新增 `DividendTaxOutstandingCause`/`DividendTaxOutstandingView`（`#[ts(export)]`，`deny_unknown_fields`）与 `SessionCorporateActions::dividend_tax_outstanding_views()` / `GameSession::dividend_tax_outstanding_views()` 只读查询面；Web 侧新增 `parseDividendTaxOutstandingView` 严格 parser，含 `needs_funds`/`cause` 与余额一致性双向校验；generated 文件为 ts-rs 正规输出（文件头与 import 形态核对），`export_bindings` 143 项含两个新导出（本人复跑确认 `export_bindings_dividendtaxoutstandingcause/view ... ok`）。`AccountId` 经 `canonical_u64_decimal` 序列化为十进制字符串，与 web parser 及 generated `AccountId = string` 一致，无跨层漂移。两份文档不实表述已修正：`personal-dividend-tax-review.md` 第 9 行重写并附 2026-10-06 修正注记（原文"余额不足显式失败"与"按卖出净额计税"两处不实均消除）；`current-handoff.md` 未完成边界段落重写为三档补税 + 部分收缴追缴 + typed 查询面，并显式登记"UI 呈现归后续批次"。依据可靠：85号第二条第二款确有"个人应在资金账户留足资金……资金暂无或不足的，及时通知个人补足资金，并划扣税款"（本人对照原文）。
- **M3（月末边界卡死）— 通过**。`personal_cash_dividend_rate` 改经 `clamped_anniversary`：目标月无对应日时钳制到目标月最后一日（逐例手算验证 1-31→2-28/29、8-31→9-30、2-29→平年 2-28），`NeedHoldingPeriodBoundaryEvidence` 变体整体删除；`>1` 年卖出不再报错。单元测试覆盖 1 月 31 日/闰年 1 月 31 日/8 月 31 日/2 月 29 日跨平年及超年边界共 16 组；会话级 `month_end_acquired_lot_sells_with_clamped_boundary_rates`（8-31 取得、10% 档、5 分）通过。`docs/trading-rules.md` 已登记口径与依据（民法典第二百零一、二百零二条 + 85号第八条），并诚实标注为实现口径。语义核验：85号第八条"持股一个月是指从上月某日至本月同日的前一日连续持股"+ 持股期限"至转让交割之日前一日"，与实现的 `disposed_on <= 周年日` 恰好等价（卖出日=同日 ⇒ 持有恰满 1 个月 ⇒ 含 ⇒ 上一档），边界语义正确。
- **M4（trading-rules 新章节）— 通过，附 1 项 minor 引用精度问题**。章节九项内容（三档税率、持股期限、月末钳制、派发不预扣转让时扣收、账户单位 FIFO、限售股、亚分处理、部分收缴追缴、显式不支持 + 装配期限制）齐全，每条标注依据与适用日期；本人对照 85号/101号官方原文逐条核验：三档税率（85号第一条 + 101号第一条改超 1 年免税）、账户单位与每日日终净增（减）数 + FIFO（85号第三条原文一致）、限售股解禁前 10% / 解禁日起算 / 167号 70号依据（85号第四条原文一致，含第五条基金身份显式不支持）、"暂不扣缴、待转让时按持股期限计算、从资金账户扣收"（101号第二条逐字一致）、施行日期（85号第十条 2013-01-01、101号第五条 2015-09-08）均正确；月末钳制与亚分处理如实标注为实现口径且官方明文未取得。**minor**：三处引用精度 —— ①持股期限"取得之日至转让交割之日前一日"的定义实际位于 85号第一条第二款，第八条是"年（月）指自然年（月）"及 1 个月/1 年的定义（文档把两者都归于第八条，内容本身无误）；②"派发不预扣"的依据只能是 101号第二条（85号第二条时代实际是派发时统一暂按 25% 计入即 5% 预扣、转让时补差，85号第二条贡献的是"从资金账户扣收 + 不足通知补足并划扣"机制）；③85号超链接指向 fgk.chinatax.gov.cn 首页而非条文页（原文页为 fgk.chinatax.gov.cn/zcfgk/c102416/c5203902/content.html）。建议下次触碰该文档时修正条号与链接；不影响已实现语义，不阻塞本批。
- **m5（零税回执膨胀）— 通过**。`outstanding == 0 && !has_payment_on(settled_on) && !has_disposition_on(settled_on)` 时跳过 `collect_due` 与回执落账；有付款/处置日照常留痕。`quiet_days_produce_no_zero_tax_collection_receipts` 断言付款日后 7 个安静日零新增回执、付款日恰好 1 张；既有 `repeated_session_ticks_on_cash_ex_date_do_not_subtract_dividend_twice` 断言 7→1 属 finding 明确要求的变更，且补强了回执日期断言（2030-01-08），不属弱化。同日幂等重放由 `collect_due` 对既有 `event_id` 的事实比较满足，`has_payment_on` 在重放时仍为真故不会误跳过。
- **m6（亚分卡死）— 通过**。按 finding 指定方案 (a)：`assessed_cents_through` 每笔分红内部精确分数求值、按笔 `round_half_up_cents`（(2n+d)/(2d) 下取整，half-up 数学正确，含 u128 转换与溢出守卫、负分子显式报错）、再按笔汇总为整分；`outstanding()` 恒为整分且负值经 `ExactDividendTaxAmount::new` 显式拒绝；`NeedRoundingEvidence` 变体删除。测试覆盖 4.2→4、0.5→1、1/30→0 三向及 restore 往返。trading-rules 登记口径并诚实标注"85号/101号未规定、官方明文未取得"。原 `exact_subcent_tax_is_not_rounded_or_erased` 断言的是被 finding 明确废弃的旧行为，替换合规。
- **m7（configure 守卫）— 通过**。`configure_cash_dividend_tax_book` 在名册已有日结回执或该证券已有登记分红时显式拒绝并说明装配期原因；`GameSession::configure_cash_dividend_tax_book` doc 注明仅装配期调用及原因。测试 `tax_book_configuration_is_rejected_after_registry_history` 覆盖登记前（仅回执）与登记后（回执+分红）两种拒绝，并确认未落任何新税账；红→绿证据在案。
- **m8（四个会话级测试）— 通过**。`cross_tier_dispositions_apply_statutory_rates_per_lot`（0%+10%+20% 混合 = 6 分，三批次跨档）、`restored_session_still_collects_tax_on_real_sale`（restore 哈希一致 + 二次 save/restore 哈希一致 + 真实卖出收 10 分）、`insufficient_cash_partial_collection_and_next_day_end_chase`（M1）、`month_end_acquired_lot_sells_with_clamped_boundary_rates`（M3）全部真实存在。断言有效：卖出经真实限价单在盘中成交（断言 `Trade` 事件 + `PersonalTradeConfirmation` + 玩家 intent 被消费），税额、现金勾稽、outstanding、视图逐项断言；非 fixture 捷径触发的伪测试。
- **n9（Web parser 两处对齐）— 通过**。①`parseExactAmount` 从"拒可整除非最简"改为 `greatestCommonDivisor(numerator, denominator) !== 1n` 完整约简检查——与 Rust `TryFrom<ExactAmountState>` 的"已约简非负分数"（`new` 归一化后比对不等即拒）语义一致，含 0/N 边界（两侧均拒 0/2、收 0/1）；②税账日结改为 `addCivilDays(dayCursor, 1) !== day.day`（首日即要求紧邻开账日下一自然日）——与 Rust `validate` 的 `days_since(previous_day) != 1`（`previous_day` 自 `opened_on` 起）逐条一致。两处均有正负例（`税账首个日结必须紧邻开账日下一自然日`、`精确税额分数必须完整约简（gcd=1）`），红→绿证据在案。
- **既有缺陷（幂等比较 `remaining_cash`→`available_cash`）— 通过**。`fifo_partial_sale_assesses_only_paid_dividend_and_collection_is_atomic` 与 `later_same_day_payment_does_not_rewrite_earlier_collection` 在 `cash_dividend_tax::tests` 14/14 中转绿（本人复跑确认）。

## 整体三问

1. **大 A 语义**：实现与现行（101号之后）口径一致——派发时不预扣、转让时按持股期限从资金账户扣收、三档 20%/10%/0%、账户单位 + 每日日终净额 + FIFO、限售股解禁前 10% 且解禁日起算；边界日语义（卖出日=周年日仍属上一档）经与 85号第八条 + 持股期限定义原文比对恰好等价。资金不足"部分收缴 + 后续日终追缴"是 85号第二条第二款"通知补足并划扣"的已登记游戏简化。月末钳制与亚分取整官方无明文，均如实登记为实现口径并给出民法典等依据。依据可靠性：本人已对照 85号/101号原文逐条核验，实质无误；仅 M4 所列条号/链接精度问题（minor）。
2. **必要且最小**：diff 严格限于 10 项 finding 及其支撑（测试、文档、typed 查询面）；fixture 泛化 `session_with_approved_cash_dividend_and_player_lots` 对既有调用方保持逐字节等价（默认 5 股、eligible 70 分不变），是四个新会话测试的必要基础设施而非无关重构。`packages/engine/src/company/mod.rs` 的 1 行 `pub use` 为任务说明中的既定保留项，但当前仓库无任何消费方（note，见下）。未发现夹带功能或顺手重构。
3. **遗漏边界 / 跨层漂移 / 复杂度**：边界覆盖充分（闰日、月末、跨档混合、half-cent 双向、gcd 负例、restore 往返、二次落档哈希）；未发现跨层漂移（AccountId/StockCode 序列化形态、generated 类型与 parser 三方一致）。三点 note：①`dividend_tax_outstanding_views` 的 `needs_funds = outstanding > 0` 等价性依赖"付款只在与收缴同一日终事务内登记"这一不变量（代码注释已说明），无专门测试锚定，未来引入日内付款时需扩 `cause` 枚举；②`ExactDividendTaxFraction` 与 `ExactDividendTaxAmount` 形态重复但注释已声明契约一致性，属可接受权衡；③一笔分红多次付款时"按笔合计后取整"与逐笔取整存在可预期差异，符合登记口径，无该场景专项测试（低风险）。

## 测试抽查结果（本人独立复跑，均通过）

| 命令 | 结果 | 耗时 |
| --- | --- | --- |
| `cargo test -p engine --lib cash_dividend_tax::tests -- --test-threads=16` | 14 passed / 0 failed | 0.00s |
| `cargo test -p engine --lib session::company_simple_session_tests -- --test-threads=8` | 23 passed / 0 failed | 2.53s |
| `cargo test -p engine export_bindings` | 143 passed / 0 failed（含 `export_bindings_dividendtaxoutstandingcause/view`） | ~1.1s |
| `node --test --test-timeout=10000 apps/web/src/save/corporate-actions-schema.test.ts` | 16 passed / 0 failed | 0.12s |

单 case 与单命令均在 10s 纪律内。`.tmp/company-system/session-actions/gate-fix-*.log` 红绿证据齐全（红日志以修复前实现复现，含旧 `NeedHoldingPeriodBoundaryEvidence`/`NeedRoundingEvidence` 与 `min(due, base)` 症状，与 finding 描述吻合）。完整 `cargo test -p engine --lib`（--test-threads=32，17.5s）的 257 项失败经抽查全部位于 stock_auction / pipeline / shared_ingress / phase_timing / player_candidates 等与本批无关模块，失败清单中无任何 dividend/company/tax 相关用例，与"既有失败、失败集合与 HEAD 一致"的登记相符（HEAD 基线未独立复验，采信修复者的 worktree 对照记录）。

## 附带观察（note，不阻塞）

1. **M4 引用精度**（同上 minor）：持股期限定义应引 85号第一条第二款；"不预扣"应仅引 101号第二条；85号链接应指向条文页。
2. `packages/engine/src/company/mod.rs` 新增 `pub use cash_dividend_tax::{CashDividendTaxBook, DividendTaxProfile}` 在仓库内无消费方（既定保留项；建议后续补消费方或移除）。
3. 提交卫生：`git status` 中的未跟踪 `.claude/worktrees/…` 目录属工作流产物，随批提交时须排除；仅需提交 11 个修改文件 + 2 个 generated TS 文件。
4. `personal-dividend-tax-review.md` 中 web 全量"4 项既有失败"与 `gate-fix-web-full.log` 尾行"8 Web test shard failed"的计数口径（项 vs shard）不完全对应，后续登记建议写明确切失败用例清单。
