# Session 股本与现金接线独立复核

复核日期：2026-10-06。复核者未参与本轮实现。范围包括本轮完整目标 diff 中的 Session 公司行为状态、新 `session/corporate_actions.rs`、Simple 分红账簿新模块及其验证、Account 现金变更、市场除息锚实际调用、Rust/Web 存档恢复与 hash/clone/day-end 路径。依据已读 `AGENTS.md`、`docs/principles.md`、Q14、`docs/company-actions-design.md`、`docs/trading-rules.md`、CommonActionsPlan 及其独立计划复核、ADR-0024/0035 和既有 Simple Finance 复核。未修改实现或自行运行测试；下列 fresh 结果均为 root 提供的日志并由本复核者读取。

## 原发现状态

1. **现金入账现已实现并通过定向短测。** 最新 `Account::credit_cash` 使用 checked add 后才以 `Arc::make_mut` 写入 candidate cash；`.tmp/company-system/session-actions/fresh-0.log` 显示正向到账及零/负/溢出原子性相关账户短测 5/5 通过。Session 只有 credit 成功才记 Paid 和 gross receipt；这关闭先前 TDD red 阶段的资金变更问题。日志范围不代表完整 Session 回归。
2. **未来批准日门禁已动态闭合。** `GameSession::approve_cash_dividend` 拒绝 `plan.approved_on > civil_date`，恢复校验也拒绝超过当前日的批准事实。`future-approved-fresh.log` 显示 `future_approved_date_cash_dividend_is_rejected_atomically` 1/1 通过，验证错误后 business hash 不变且没有保存分红 book。
3. **（前一批次历史发现，已由当前公告接线状态关闭。）** 当时 `process_dividends_on_day_end` 只推进 `CashDividendBook` 状态，未进入 `PublicLibrary`、NPC acquisition 或公开披露事件；后续补入 typed announcement 并经 Session 定向测试和独立复核后关闭。该历史描述不再代表当前实现状态。

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

- **公开公告 typed 接线已限定复核通过。** `AnnouncementContent::Shock(AnnouncedEvent)` 与 `AnnouncementContent::CashDividend(CashDividendAnnouncement)` 明确区分经营冲击与分红公告；现金分红公告承载完整冻结 `CashDividendPlan` 和实际税前批准 `total_gross`。`distributable_amount` 仅是授权上限，公布总额从 `CompanySystem::dividend_plan_facts` 的已批准 `total_gross` 取值，不读尚未登记时为零的 `CashDividendBook::total_gross`。实现涉及 `PublicLibrary` 插入/恢复校验、Simple 日终派发、Session restore 勾稽，以及旧 Shock/PaymentFailure consumer 迁移。
- **发布时间和 NPC 获知边界明确。** Session 仅在计划 `announced_on` 与结算日一致且 Book 已进入 `Announced` 后，于当天 18:00 disclosure phase 发布；公告日期校验要求 `plan.announced_on == occurred_on`，通用时序守卫要求同日 18:00。该 18:00 是游戏约定，不是交易所/公司法规定的分红公布时点。NPC 继续通过既有 `discovery_candidates` 和 `record_acquisition` 本人获知；CashDividend 不伪造成 Shock、不产生 `CreditDefault`，也不引入估值或交易因果。
- **restore 跨域勾稽。** 每条现金分红公告必须绑定唯一 Session `CashDividendBook`，完整 plan 相同且生命周期已离开 `Approved`；公告 `total_gross` 必须等于 Simple 已批准财务事实中的精确金额。生命周期已开始的每个分红 Book 恰有一条公告，未公告的 `Approved` Book 不得有公告；重复公告、缺 Book/finance fact、plan 或批准金额错配均显式拒绝。该检查属于 Session save/restore 边界，不改变 `PublicLibrary` 独立公开面的内容校验职责。
- **领域依据及范围。** 分红宣告、股东登记、税前 gross 和后续支付保持分层；依据 `docs/company-actions-design.md` §2.2、§4 及官方《公司法》第210、212条核验记录。官方依据支持分配条件/决议及支付时限边界，不支持将游戏 18:00 phase 宣称为法定/交易所时间。本次不调整现金股息除息参考价、登记名册、持有人税务或公司账务；完整交易所/中国结算分红逐类流程也不因此视为支持。
- **独立复核与定向验收。** 未参与实现的 Luna reviewer 独立核对 typed announcement diff，限定 PASS：批准总额与授权 cap 语义正确、公告时点绑定正确、CashDividend 不进入 Shock/违约因果，改动范围必要且聚焦。reviewer 建议补测 `PublicLibrary` 对非正/超授权 gross 的拒绝，以及另一种跨域篡改（plan/status/重复公告）；这些是未覆盖建议而非已发现漏洞。本轮 root 提供的 `.tmp/company-system/session-actions/final-announcements.log` 中 `company_simple_session_tests` 为 16/16（1.17s），覆盖真实日终发布、本人 acquisition、restore 保留及在授权范围内篡改 `total_gross` 后由跨域恢复守卫拒绝。较早 `latest-session-announcement-green.log` 的 15/16 是旧 fixture 缺 baseline belief 的失败证据，不代表最终状态。未运行完整 Engine/Web/native 回归。

## Engine 信息链最终批次复核

复核日期：2026-10-06。由未参与实现者独立通读本批当前全部 23 个 `packages/engine/**` 未提交文件完整 diff，并核对 `AGENTS.md`、`docs/principles.md`、ADR-0016、Q14 设计蓝图、公司行为设计及既有缺年报审计/旧回滚函数快照。未修改产品代码、未运行 Cargo、未提交改动。`.tmp/company-system/session-actions/final-announcements.log`、`legal-monthly-final-green.log` 与 `final-interim.log` 是 root 提供并由本复核者亲读的日志，不是本复核者运行结果。

- **领域语义与证据：通过。** ADR-0016 要求公开信息与本人获知隔离、只用本人信息集；中期报告缺本人同 scope 年报时不混范围或年化，资料不足应显式不可用。新 `ForecastBasis::AnnualBaselineUnavailable` / `ValuationUnavailable::AnnualBaselineNotOwnKnown` 保留原因与既有分析方法，不构造估值；有效报告引用、owner、公司、scope 等损坏仍传播错误。旧日终回滚测试现在确实构造 NPC 已获取 Annual 报告、随后从 PublicLibrary 删除该 `PublicationId` 的损坏事实引用；错误检查及重试原子性因此针对真实损坏状态。CashDividend 被类型化为独立内容，既不伪作 Shock，也不生成 `CreditDefault`。分红公告的 `total_gross` 是 Simple 已批准的税前总额，`distributable_amount` 保持授权上限；没有字段或消费者将 gross 冒称税后到账。18:00 是游戏 disclosure phase，不是交易所/法定公布时点。此批不更改沪深交易制度，Q14 与 ADR 是产品契约而非法源，现行 A 股依据不被扩大声称。
- **恢复与日终勾稽：通过。** 日终仅在公告日与计划 `announced_on` 同日且 book 已转为 `Announced` 时发布。Session restore 对公告与唯一 dividend book、完整 plan、Simple 批准财务事实及批准 gross 做交叉校验；重复、缺项或篡改均明确拒绝。NPC 按其既有个人订阅/发现流程获知，CashDividend 获取不会污染 Shock/违约因果。月报测试通过真实 `step()` 后日终流程，未靠改写 clock 模拟。税务配置和后续公司行为规则未被本批宣称实现。
- **范围发现已关闭：** 收窄后的原始 diff 中，`session.rs` 仅新增必要的 `dividends` 日终 disclosure context 注入；`decision_chain.rs` 仅将既有 Shock 测试构造迁移到 `AnnouncementContent::Shock`。未见残留无关格式 hunk，两处均为本批接口/类型变更所需。
- **验证与后续边界：** root 提供的 `final-announcements.log` 显示公司 Session 用例 16/16，`legal-monthly-final-green.log` 显示 notices 10/10，`final-interim.log` 显示 interim 1/1；只证明这些定向用例，不代表完整回归。`CreditDefault` 在本人无 Annual 时仍可能返回 `NoOwnAnnualMaterial` 并阻断该信息路径；既有审计已明确承认这是独立后续工作，本批未关闭，也不作为本批已修复项。另有 PublicLibrary 非正/超 cap gross 拒绝及替代跨域篡改 fixture 可继续补强，但静态校验和本批 restore 对批准总额的实际勾稽未显示当前缺陷。

**结论：Engine 信息链批次 PASS（限定当前 23 个 Engine 文件，不代表完整回归或所有宿主已完成）。** 两个收窄文件最终 diff 已复核；`CreditDefault` 缺 Annual 仍是明确后续边界，不计作已修复。Root 另行发现的 Web save/schema 新枚举接线缺口由独立 reviewer 处理，在其关闭前不可据本 Engine 签核声称跨层批次全部完成。

### Annual baseline recovery 增量复核

2026-10-06 再次完整通读 `strategy/fundamental/update.rs`、`tests/fundamental_beliefs/interim.rs`、`tests/fundamental_beliefs/main.rs` 的新增最终 diff，并核对 ADR-0016 的个人获知/同 scope 年报规则及上述 Q14 缺资料应明确不可用约定。未运行 Cargo；`.tmp/company-system/session-actions/annual-baseline-recovery-final.log` 与 95274 全 targets 检查结果由 root 提供，本复核者仅亲读日志。

- 原先 `AnnualBaselineUnavailable` 的合法状态在 NPC 后续**实际获知** Annual 后不再卡在无预测/无估值状态。一般 `apply_material` annual 路径只在旧 basis 是该 unavailable marker 时重新 `initial_forecast` 并按比较资料可用性形成初始信心；已有正常 baseline 的 ordinary Annual 仍走既有 λ 修订且保留信心。测试保留并断言 ordinary interim λ 的既有结果，也分别覆盖 acquired Annual 有/无 prior-year comparative 的恢复。
- unavailable belief 到期时若本人已获知同 scope Annual，按该 Annual 建立初始 forecast、重估并更新所用报告；horizon confidence 保持到期前值，因果仍记录真实 `HorizonExpired`。最终测试覆盖 basis、confidence、used report 及 `last_cause`。本人仍无同 scope Annual 时继续推进 unavailable anchor/cause，既有覆盖保证不会因资料缺失崩掉。所有查找仍经过 `NpcObservationContext::acquired_reports()`；没有公开库扫描代替个人获取。
- no-prior-year fixture 通过修改有效 `PublicLibrarySave` 中报告比较列并 `PublicLibrary::from_parts` 重建，明确是报表资料 fixture，不伪称 Simple producer 生成；原真实已发布 interim 与其 PublicationId 在 clone 中保持，以防 fixture ID 混淆。`Scenario: Clone` 是构造多账户独立情景的必要测试支撑。源码新增量与恢复场景相称。
- root 提供的 `annual-baseline-recovery-final.log` 为 interim 用例 1/1 通过；整体全 targets 检查通过亦为 root 报告，本复核者未重跑。初次 RED 曾因比较期 fixture 缺误导项而在 `AnnualBaselineUnavailable` 与预期 `InitialWithoutHistory` 比较处失败，随后 root 修正有效比较列 fixture；失败日志不是最终回归证据。

**该恢复增量复核通过。** 合并后的 Engine 结论仍限于以上静态范围和所列定向结果；Web schema 跨层解析由独立复核处理，`CreditDefault` 缺 Annual 仍未解决。
