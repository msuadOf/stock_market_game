# 个人公开市场股息税独立复核记录

本批为未实施者复核记录。多轮新起与复用 Luna reviewer 均在平台侧 `thinking_signature_invalid` 处中断，未执行文件修改或得出代码结论；该失败与仓库 diff 无关，不能作为复核通过证据。

主 agent 随后按只读门禁复核本批相关源码：

- 税务身份只接受 `IndividualPublicMarket`，企业、基金、非居民与未配置身份显式保留不支持或 `TreatmentNotConfigured`，不从账户类型或策略风格推断。
- 税账以分红登记与持有 lot 为事实源，只在真实 `PersonalTradeConfirmation` 卖出时按 FIFO 消耗；未成交挂单不触发收缴，也不伪造成交。
- 持有期按自然日计算，不足一个月的卖出按已到账分红对应处置股份计 20% 补税（不是按卖出净额计税），不虚构税后现金流。收缴使用受控 `debit_cash`，资金不足时按 `min(应纳税额, 账户真实现金)` 部分收缴并在后续日终继续追缴（财税〔2012〕85号第二条），不给投资者自动补钱。（2026-10-06 修正：原文“余额不足显式失败”与实现不符，见下方门禁复核记录。）
- Rust restore、Web strict parser 和三份 current fixtures 对同一 `dividend_tax_books` 契约建模；fixtures 相对 HEAD 仅添加空数组，并保持原单行 JSON 格式。

验证范围仅为本批定向短测：两个完整模块路径的 Engine 测试各 1 项通过；Web 公司行为 schema 13 项、当前存档契约 33 项和 `apps/web` TypeScript 检查通过。未运行完整回归。宿主/UI 配置入口、其他税务身份和其他股本行为仍不在完成范围。

## 非作者门禁复核（2026-10-06）

对提交 f1fc21f6（feat(engine): 接通个人现金分红税补缴）的非作者门禁复核发现以下问题，本批按 TDD 修复。原文照录如下（含严重度与依据）：

- **【M1｜高｜corporate_actions.rs settle_dividend_tax】** `base_available_cash = cash + Σ历史collected` 把历史已收缴税额加回可用现金，资金上限虚增；当 outstanding > 真实现金但 ≤ base 时 `debit_cash` 触发 `InsufficientCash` 令整个日终失败、回滚后每日重复同样失败。修复：移除加回项，直接以账户真实现金作为 `collect_due` 的 `available_cash`（同日幂等重放由 `collect_due` 的 existing 比较天然满足，不另造机制）。实测复核补充：`debit_cash` 仅在溢出时返回 `InsufficientCash`，本缺陷在该路径的实际表现是**按虚增上限多收税款并把账户现金扣成负数**（探测到 `available_cash=203 = 现金3 + 历史200`，收缴 10 后现金 −7），比原 finding 描述更严重。
- **【M2｜高｜同文件 + 文档】** 现状无历史收缴时按 `min(due,cash)` 部分收缴，`needs_funds` 全仓库无消费方、session 层不检查不展示；而本文件第 9 行与 current-handoff.md 声称“余额不足显式失败”——不实。按官方口径（财税〔2012〕85号第二条：从资金账户扣收，不足的通知补足并划收）统一语义：部分收缴 + 后续日终继续追缴 + 把 outstanding/needs_funds（含原因）通过既有 typed 公司行为查询面显式暴露（engine 查询 DTO → Web 严格 parser → ts-rs 正规 typegen，禁止手写 generated）。UI 呈现归后续批次，在 current-handoff.md 登记该边界。
- **【M3｜高｜cash_dividend_tax.rs personal_cash_dividend_rate】** 月末取得的 lot（29/30/31 日取得，及 2/29）一旦有已到账分红登记，任何卖出都在对月边界计算 `from_ymd(下月,31)` 等不存在日期时触发 `NeedHoldingPeriodBoundaryEvidence`，日结被卡死——包括远超一年、税率无歧义（应为 0%）的卖出。修复：对月对日无对应日时把边界钳制到目标月最后一日（依据民法典第二百零一条/第二百零二条期间计算惯例 + 财税〔2012〕85号第八条），不只对歧义区间报错。docs/trading-rules.md 登记该口径。
- **【M4｜文档｜docs/trading-rules.md】** 新增“个人流通股股息红利差别化个人所得税”章节，完整登记本批已接通语义（三档税率、持股期限、派发不预扣转让时扣收、账户单位 FIFO、限售股、月末钳制、亚分处理、部分收缴追缴），每条标注依据文件与适用日期。
- **【m5｜中｜corporate_actions.rs】** outstanding 为 0 且当日无新付款/处置时跳过 `collect_due` 与 `TaxCollectionReceipt` 落账，消除逐日零税回执的存档膨胀；有事件日照常留痕。
- **【m6｜中｜cash_dividend_tax.rs】** 亚分税额 `NeedRoundingEvidence` 卡死常见组合（如每股 7 分×3 股×20%=4.2 分）。选择 **(a)**：定义“按持有人每笔分红合计应纳税额四舍五入（half-up）到分”的规则并实现，登记 trading-rules.md。理由：与 `ExactDividendTaxAmount` 精确分数结构衔接干净——每笔分红内部仍精确求值，仅按笔汇总后取整为整数分，`outstanding()` 恒为整分，`NeedRoundingEvidence` 变体整体移除；无结构冲突，且 (b) 会把最常见的整数股×整数分×20% 组合（乘 20% 后天然出现 0.2 分尾差）留在不可用状态。
- **【m7｜中｜session.rs configure_cash_dividend_tax_book】** 加前置守卫：名册已有历史日结回执或已登记分红时拒绝配置，防误用后日终永久卡死；doc 注明仅装配期调用。
- **【m8｜测试｜company_simple_session_tests.rs】** 补四个会话级短测：跨档位卖出、restore 后再真实卖出补税、现金不足部分收缴+追缴、月末取得批次卖出正常计税。
- **【n9｜中｜apps/web/src/save/schema/corporate-actions.ts】** Web parser 两处对齐 Rust：`ExactDividendTaxAmount` 分数校验从“拒可整除非最简”改为完整 gcd=1 检查；首个税日要求紧邻开账日下一自然日（对齐 Rust `record_net_day`/validate 的 `addCivilDays(opened_on,1)`）。

**门禁复核另行发现的既有缺陷（f1fc21f6 提交时已红）**：`collect_due` 同日幂等重放的比较写成 `existing.remaining_cash == available_cash`，正确应为 `existing.available_cash == available_cash`，导致 `fifo_partial_sale_assesses_only_paid_dividend_and_collection_is_atomic` 与 `later_same_day_payment_does_not_rewrite_earlier_collection` 两条已提交测试在 HEAD 即失败（独立 worktree 验证）。本批随 M1 一并修复。

### 修复方式与测试证据

| Finding | 修复 | 红→绿测试 |
| --- | --- | --- |
| M1 | 移除 `collected_tax` 加回，`collect_due` 直接收账户真实现金 | `session::company_simple_session_tests::insufficient_cash_partial_collection_and_next_day_end_chase`（红：收缴 10≠3 且现金被扣成 −7；绿：部分收缴 3、outstanding 7、次日追缴 7） |
| M2 | 新增 `DividendTaxOutstandingView`/`DividendTaxOutstandingCause` typed 查询 DTO（`#[ts(export)]`）与 `GameSession::dividend_tax_outstanding_views()`；Web 新增 `parseDividendTaxOutstandingView` 严格 parser；修正两份文档不实表述；current-handoff 登记 UI 边界 | 同上会话用例内嵌视图断言 + web `股息税未划收查询视图按严格 parser 校验余额与原因一致性`；typegen 143 项含新导出 |
| M3 | `personal_cash_dividend_rate` 边界经 `clamped_anniversary` 钳制到目标月最后一日；移除 `NeedHoldingPeriodBoundaryEvidence` 变体 | `cash_dividend_tax::tests::month_end_acquisition_clamps_period_boundary_to_target_month_end`、`leap_day_acquisition_clamps_year_boundary_across_non_leap_years`、会话级 `month_end_acquired_lot_sells_with_clamped_boundary_rates`（红：日结被 `NeedHoldingPeriodBoundaryEvidence` 卡死） |
| M4 | docs/trading-rules.md 新章节（见上） | 文档变更，无独立测试；口径由 M3/m6/M2 测试锚定 |
| m5 | settle_dividend_tax 在 outstanding=0 且当日无新付款/处置时跳过收缴落账 | `quiet_days_produce_no_zero_tax_collection_receipts`（红：7 张零回执；绿：仅付款日 1 张）；既有 `repeated_session_ticks_on_cash_ex_date_do_not_subtract_dividend_twice` 断言由 7 改为 1（付款日照常留痕） |
| m6 | `assessed_cents_through` 按笔求值后 `round_half_up_cents` 取整；`outstanding()` 恒为整分；移除 `NeedRoundingEvidence` | `per_dividend_tax_rounds_half_up_to_whole_cents_at_collection`（红：21/5 ≠ 4/1）、`half_cent_per_dividend_tax_rounds_up_and_sub_half_cent_rounds_to_zero`；替换原 `exact_subcent_tax_is_not_rounded_or_erased`（其断言的旧行为按 finding 明确变更） |
| m7 | `configure_cash_dividend_tax_book` 装配期守卫（名册无历史回执且无已登记分红）；session.rs doc 注明 | `tax_book_configuration_is_rejected_after_registry_history`（红：晚配置被接受；绿：拒绝并说明装配期） |
| m8 | 四个会话级短测 | `cross_tier_dispositions_apply_statutory_rates_per_lot`（0%+10%+20% 混合=6 分）、`restored_session_still_collects_tax_on_real_sale`、M1 用例（部分收缴+追缴）、`month_end_acquired_lot_sells_with_clamped_boundary_rates`（5 分=10% 档） |
| n9 | web parser gcd=1 完整约简检查 + 首个税日紧邻开账日 | web `税账首个日结必须紧邻开账日下一自然日`、`精确税额分数必须完整约简（gcd=1）`（两侧各含负例，红→绿） |
| 既有缺陷 | `collect_due` 幂等比较改回 `available_cash` | HEAD 即红的 `fifo_partial_sale…`、`later_same_day_payment…` 转绿 |

**验证证据**（命令与 exit code 见 `.tmp/company-system/session-actions/gate-fix-*.log`）：

- `cargo test -p engine --lib cash_dividend_tax::tests`：14 项通过（红 6 失败 → 绿）。
- `cargo test -p engine --lib session::company_simple_session_tests`：23 项通过（红 3 失败 → 绿）。
- `cargo test -p engine export_bindings`：143 项通过，含新导出 `DividendTaxOutstandingView`/`DividendTaxOutstandingCause`；`node scripts/check-generated-types.mjs` 仅报告两个新类型文件未跟踪（待随批提交），既有 generated 无漂移。
- `node --test …corporate-actions-schema.test.ts`：16 项通过（红 3 失败 → 绿）。
- `cargo check -p engine`：通过，触达文件无新警告。
- 完整 `cargo test -p engine --lib`（--test-threads=32）与 `node scripts/run-web-tests.mjs` 在本机 HEAD f1fc21f6 即分别有 259/257 量级与 4 项与本批无关的既有失败（独立 worktree 基线对照）；本批改动后失败集合与 HEAD 完全一致（仅减少两条本批修复的既有失败），未引入新失败。既有失败不在本批范围，未宣称完整回归通过。
