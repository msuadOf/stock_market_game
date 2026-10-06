# Session 股本与现金接线独立复核

复核日期：2026-10-06。复核者未参与本轮实现。范围包括本轮完整目标 diff 中的 Session 公司行为状态、新 `session/corporate_actions.rs`、Simple 分红账簿新模块及其验证、Account 现金变更、市场除息锚实际调用、Rust/Web 存档恢复与 hash/clone/day-end 路径。依据已读 `AGENTS.md`、`docs/principles.md`、Q14、`docs/company-actions-design.md`、`docs/trading-rules.md`、CommonActionsPlan 及其独立计划复核、ADR-0024/0035 和既有 Simple Finance 复核。未修改实现或自行运行测试；下列 fresh 结果均为 root 提供的日志并由本复核者读取。

## 原发现状态

1. **现金入账现已实现并通过定向短测。** 最新 `Account::credit_cash` 使用 checked add 后才以 `Arc::make_mut` 写入 candidate cash；`.tmp/company-system/session-actions/fresh-0.log` 显示正向到账及零/负/溢出原子性相关账户短测 5/5 通过。Session 只有 credit 成功才记 Paid 和 gross receipt；这关闭先前 TDD red 阶段的资金变更问题。日志范围不代表完整 Session 回归。
2. **未来批准日门禁已动态闭合。** `GameSession::approve_cash_dividend` 拒绝 `plan.approved_on > civil_date`，恢复校验也拒绝超过当前日的批准事实。`future-approved-fresh.log` 显示 `future_approved_date_cash_dividend_is_rejected_atomically` 1/1 通过，验证错误后 business hash 不变且没有保存分红 book。
3. **分红公告仍未接入公开信息库，属于明确未完成范围。** `process_dividends_on_day_end` 只将 book 状态改为 `Announced`，没有 `PublicLibrary`/`AnnouncementRequest`；日终公开派发也没有纳入该 book。因此公告日期目前仅是私有流程事实，不进入公开查询、NPC acquisition 或公开信息事件。实现 owner 已说明 typed public dividend announcement 在下一批处理；在该批复核和验收前，本轮不能宣称公开披露/NPC 获知完成。

## 已核实边界

- 空 `SessionCorporateActions.registries` 是允许的；Session getter 明确说明空表表示完整股份来源事实尚未配置（`session.rs:2356-2358`）。方案受理要求显式匹配名册；registry 校验发行人/证券/总股本及所有账户实际持仓，完整名册总股数由 `ShareRegistry::validate` 守恒校验。没有发现自动补造匿名持有人、残余股份、税务身份或取得日期。
- 每自然日名册结算从真实 `PersonalTradeConfirmation` 汇总账户证券净成交，并通过 `close_day` 连续推进；随后逐账户对比名册股数与 Account 实际持仓（`session/corporate_actions.rs:184-257`）。新开户但零持仓不会被当作股东；非既有 holder 仅凭真实净买入进入登记。无需把该结论扩成未测试的所有成交边界已经覆盖。
- 分红计划对授权上限、按税前每股金额乘非库存股资格股数得到的 gross 总额、Simple 可分配利润及显式注册资本事实分别校验（`session.rs:2361-2420` 与 `finance_dividend.rs`）。未发现把 Simple 展示现金当预算、补充账户资金或伪造回购成交的路径。
- 日终先建立 session checkpoint，登记推进、逐户付款、公司账簿更新与其后的日终处理发生系统错误时，由外层整体回滚（`session.rs` 的 day-end checkpoint）。逐 holder 业务付款失败作为明确 `Failed` 保留未付权利；已成功 holder 被排除于下次重试。账户现金增加与 overflow 原子性由 fresh-0 短测覆盖。
- Rust 状态包含 corporate actions，并接入 save/restore、clone、business hash；账户及外部 gross receipt 与 book 的 Paid amount/date/payment id/plan id/holder 及 `TreatmentNotConfigured` 交叉验证。账户与 External receipt 日期上界在 Rust 恢复校验中按 current date 验证；不将 gross Paid 误称为 TaxPaid。
- 除息路径在 `pipeline::plan_tick` 的 `TickShadow` candidate 内、市场资源/报价快照前准备；同证券同除息日方案先合并税前每股 gross，再一次安装参考价锚。该调用尊重证券交易所日历并保持历史成交价；特殊调整公式显式拒绝。依据记录在 `docs/trading-rules.md`：沪深《交易规则（2026年修订）》相应除权除息条款，自 2026-07-06 生效，来源见 dividend research。该结论仅针对标准纯现金除息基础，不等于整套公司行为已完成。

## 结论与复核门禁

Session 股本/现金接线、批准日期与恢复日期门禁、除息失败原子性和 `SaveSlot` ts-rs 类型在限定范围内 **复核通过**：Account 定向用例 5/5，除息 E2E 1/1，未来批准日 E2E 1/1，到账未来日期篡改 E2E 1/1，ts-rs 导出 135/135；registry/session 2/2、share registry 14/14、CashDividendBook 16/16、Simple finance 分红 4/4、Market 锚 8/8、auction 撤空 1/1 也见所列日志。此次签核不覆盖完整回归或尚未实现的公开分红公告。CommonActionsPlan 的空 registry、完整持有人事实、实际账户净持仓与交易确认、税身份标记和失败付款重试边界在当前路径中有相应校验；除现行官方规则基线外不以未核实的中国结算指南扩张结论，碎股尾差、税务结清、送转/配股/增发及回购仍不能据此宣称已支持。

本记录结合静态 diff 与 root 提供的新鲜定向测试/typegen 日志；未由本复核者独立重跑，也不代表完整回归。公开公告后续接线须由非实施者另行复核；完整回归或交易所规则未覆盖事项均不得借此记录宣称完成。

## Tick 原子性增量

- **finding 已静态和定向动态关闭。** `prepare_cash_ex_references_for_current_date` 由 `pipeline::plan_tick` 在 `TickShadow::capture` 后、`ExpiryShadow` 与 `SealAllocationSnapshot` 前，对 candidate 调用。准备失败或后续阶段失败不会污染 authority；成功时锚和应用组随 CommitTick 共同安装。真实登记/除息日双方案 fixture 注入 `post_shadow_failure`，断言业务 hash、applied groups 和 anchor 未改变，再验证成功仅扣一次及存档恢复。`build-final-review-fixes.log` 编译成功，`review-fix-2.log` 中该 E2E 1/1 通过；之前 fresh-7 也为 1/1。没有依赖 helper 内 clone 冒充整 tick 原子性。

提供的 `build-final-candidate.log` 与 `build-final-review-fixes.log` 编译成功；fresh-6 中 registry/session 2/2，fresh-7/review-fix-2 中除息测试 1/1，fresh-8 auction 撤空/除息活动标记 1/1。fresh 日志证明这些被点名用例结果，不外推完整 engine/web 回归。

## 当前增量状态

- **TS 注解误列已正式 typegen 关闭。** 当前 `report_correction_operations` 恢复 `#[ts(skip)]`，`corporate_actions` 字段使用 `SessionCorporateActions` schema import type，和 Web strict schema 的两个独立字段一致。ts-rs 导出 135/135 通过，生成 SaveSlot 已核对，详见下项。

- **到账日期上界已接入恢复校验并通过真实 E2E 负例。** `SessionCorporateActions::validate` 对账户和具名 External gross receipt 都拒绝 `paid_on > current_date`。真实 approve/settle/restore E2E 将账户 receipt 日期篡改为次日并断言类型化拒绝；`.tmp/company-system/session-actions/review-fix-2.log` 中包含该 E2E 1/1 通过。
- **SaveSlot TS 类型已正式生成并核对。** `.tmp/company-system/session-actions/typegen-review-fix.log` 的 ts-rs 导出 135/135 通过；生成的 `apps/web/src/types/generated/SaveSlot.ts` 包含 `corporate_actions: import("../../save/schema/corporate-actions").SessionCorporateActions`，不再将其误列到 `report_correction_operations`。该注解发现动态关闭。

- **公开公告仍未接线，按当前产品范围未验收。** Book 的 `Announced` 仅保存状态，未生成 `PublicLibrary` announcement，也未进入 NPC acquired information 路径。owner 已告知后续另批实现 typed public dividend announcement；本轮不得宣称公开披露/NPC 获知已完成。
