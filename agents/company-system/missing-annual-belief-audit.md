# 缺少年报时的信念重估审计

## 范围与结论

先只读检查 `beliefs.rs`、`strategy/fundamental/{update,facts,valuation}.rs`、`information` 获取视图与 `session/notices.rs`，并对照 ADR-0016、ADR-0035、Simple 公司目标记录；随后按 root 授权实施最小的中期材料缺年报修复及相应测试。工作区有大量并行改动；没有运行 Cargo 或操作 index。

确认存在真实缺口：一个参与者合法获知其本人尚未获知年报的 Monthly、Quarter 或 HalfYear 报告时，`apply_interim_material` 在 `update.rs` 约 226 行把缺少同 scope 年报映射为 `BeliefError::NoOwnAnnualMaterial`。`notices.rs` 的日终编排把所有此类 `BeliefError` 包装为 `SessionError::InvalidSave`。因此当受关注股票的日终信息检查获知中期报告，而本人此前没有该 scope 的 Annual acquisition 时，日终整个事务失败；信息检查及后续日结不能继续。用户描述的季度/公告路径与此一致。

该状态本身是合法的：ADR-0016 约定估值材料只从本人已获知集合选择，中期报告按同 scope 年报基数作补充，不可混用 scope 或年化。公开的 Monthly/Quarter/HalfYear 新材料，与特定 NPC 尚未获知任何 Annual 完全可以并存。ADR-0035 的 Simple 目标允许按月、季度、半年或年结算并披露；披露频率不意味着每位 NPC 已经读过年报。

## 错误分类

- `ValuationUnavailable` 已可序列化并可存入 `BeliefEntry.valuation`，明确写有“不抛弃整个 NPC；其他信号路径照常运转”。估值方法禁用、合并现金流未归母、非正净利/权益等已经走这一类。
- `NoOwnAnnualMaterial` 是编排错误，不是估值输入不可用；它被 `BeliefError` 单独定义，并从 interim、credit default、horizon expiry 的查找分支返回。
- 真正损坏的来源引用仍应报错：`NpcObservationContext::report` 对 acquisition 中不存在的 PublicationId 返回 `AcquisitionError::NotAcquired`；owner 不符、公司归属不符、未来披露、材料种类不符、scope 错配及坏版本也不应降级成 unavailable。不能用“忽略获取失败”方式吞掉引用损坏。

## 现有回滚测试解释

`notices.rs::information_revaluation_failure_rolls_back_entire_day_end` 删除 `PublicLibrary` 中全部 Annual 报告，再在 2032-01-31 对关注者交付当日新材料，断言 `InvalidSave`、事务回滚，恢复原库后同日重试只结算一次。它确实是人工删改公共库制造的状态，不是“库中尚未有 Annual、NPC 也尚未获知 Annual”的原样合法场景；但当前断言实际只核验 `NoOwnAnnualMaterial`，没有命中缺失 acquisition ID、错误 owner、scope 不匹配等完整性故障。因而它**不能作为 NoOwnAnnualMaterial 必须 fatal 的证据**。用户要求不要偷改/弱化断言是正确的：保留专门的 corruption fatal 用例，并把现有测试改成断言其真实损坏前置条件（如删掉被 acquisition 引用的报告并显式核验引用完整性失败），另加全新合法资料不足用例。不得把旧用例简单改成成功日结而丢弃 rollback/重试覆盖。

`agents/company-system/session-review.md` 曾把该 fixture 解释为“损坏既有信息不变量”，但源码里显式 `retain` 仅删除公开 Annual 报告；当前断言收到的却是纯 `NoOwnAnnualMaterial`。源码所示 fixture 并未显式破坏某个既存 acquisition 引用。因此这段旧解释证据不足，需按 fixture 实际断言与状态构造重新界定。

日志已在 `.tmp/company-system/session-actions/source-and-announcement-red-2.log` 与 `-3.log` 找到。两者都是 `company_simple_session_tests` 中公开分红公告测试，执行 Session 信息交付时 `apply_credit_default` 因没有本人已知 Annual 报 `NoOwnAnnualMaterial`，并非 restore validation 失败。它们证明**另一个公告触发器**也把估值基线不足提升成日终错误；不替代本审计 Quarter/Monthly 路径。现金分红公告的 credit-default 语义还需判断：如果该公告只造成调整而信用事件并不代表违约，不应借此扩张 `CreditDefault`；本轮修复不应顺带改变公司行动模型。公告 fixture 目前应先按其约定给 NPC 获取已公开 Annual，避免掩盖真正的分红/恢复断言。

## 实施更新

1. `apply_interim_material` 在报告已由 `NpcObservationContext` 验证、事实抽取成功之后，按精确同 `ScopeId` 查询本人已获知 Annual。缺失时现在写 `ForecastBasis::AnnualBaselineUnavailable` 与可序列化 `ValuationUnavailable::AnnualBaselineNotOwnKnown`，保留当前 analysis method，估值不生成，`used_report_ids` 只含触发材料，`last_cause` 记真实触发。没有切 scope、年化中期资料、改模型或读私有材料。事实抽取中的引用、公司/报告类型和算术错误仍显式传播。
2. 该 unavailable belief 后续到期时，只有在本人仍未获得该 scope Annual 的条件下推进 horizon anchor 与 `HorizonExpired` cause，保留 unavailable 状态和材料 id；如本人期间已获取年度材料，则仍走原年度重估。其他 ForecastBasis 到期而缺少其所需 Annual 仍 fatal。
3. `notices.rs` 原完整回滚/重试测试已改成真正 dangling acquisition fixture：先登记一个有效 Annual acquisition，再从 PublicLibrary 删除该 id；错误断言仍要求日终失败，且完整 business hash/save 原样不变，恢复 PublicLibrary 后月度 summary、tax、report/Closing 只生效一次，重复日结被拒且状态不变。历史函数快照保留于 `reference/information-revaluation-failure-rolls-back-entire-day-end.rs.txt`。新增合法资料不足 Session case 设置真实 Monthly schedule 与无 Annual 的有效 PublicLibrary，NPC 首次观察获取本人 Monthly 后日终继续完成，并断言存档 reason/method/cause/used id 正确及 restore/resave 一致。该 fixture 将 `ticks_per_day=1` 并真实调用 `step()` 后再日结，没有手工调整 clock。
4. TDD 红测由 root 运行：首个 no-annual interim Red 已确认目标运行时错误 `Err(NoOwnAnnualMaterial)`；首次报告被非法 Consolidated report ROE 元数据拦截，不作红证据，随后替换为真实生成的 HalfYear standalone report。新增 horizon Red 也已确认缺 Annual belief 到期返回 `Err(NoOwnAnnualMaterial)`。最终 `final-interim.log` 由 root 使用新 binary 签核 1/1（含到期边界）；`legal-monthly-final-green.log` Notices 10/10（含新增合法 Monthly 日终及 corruption rollback/retry），实际验证 1 tick 完成交易日、首次本人获取公开月报和保存恢复续行。日志均位于 `.tmp/company-system/session-actions/`。本 agent 未自行运行 Cargo。
5. `Correction` 中的 Monthly/Quarter/HalfYear 仍走 `apply_interim_material`，因此缺基准时同样记为 typed unavailable；年度更正本身即是本人已知 Annual，可继续原路径。`CreditDefault` 在合法无 Annual 时仍会由 `apply_credit_default` 返回 `NoOwnAnnualMaterial`，可能阻断信息交付。Root 明确将其留作后续独立批次：另行决定如何只记录不可用与真实公告 cause，不借本批定义信用估值经济效果。不能宣称所有合法“无 Annual”触发器已解决。
6. Web 存档契约同步识别 `ForecastBasis::AnnualBaselineUnavailable` 与 `ValuationUnavailable::AnnualBaselineNotOwnKnown`；`beliefs.test.ts` 验证这两个 Rust serde unit variant 的字符串形式可严格 round-trip，并验证未知 tagged variant 仍被拒绝。红测在旧 parser 上因合法 basis 字符串被当作对象而失败，加入 union/parser 分支后短测全绿。该测试只证明 parser 契约，不是完整合法缺年报存档的跨层验收；没有据此声称 Engine 生成档案已端到端验证。
7. 后续边界审计发现，`AnnualBaselineUnavailable` 条目首次取得本人同范围 Annual 时，普通材料路径仍会将 `WithoutHistory` 观察交给 `revise_forecast`，后者按“缺历史不修订”保留 unavailable basis；即使有 `TwoYear` 也会以旧不可用状态参与 λ 修订。到期路径也只更新估值、未从本人新获知 Annual 建立 forecast。新增真实 acquisition 测试先确认合法 RED：`annual-baseline-recovery-valid-red.log` 在首个本人 Annual 后仍得到 `AnnualBaselineUnavailable`，预期分别为 `InitialWithoutHistory` / `InitialTwoYear`。修复后 `annual-baseline-recovery-final.log` 与 `annual-baseline-final-cause-green.log` 均为该定向 case 1/1 通过，覆盖本人 Annual 通过真实 acquisition 可见、无先前收入比较项与有两年比较项、NewMaterial 与到期恢复。NewMaterial 首次形成 Annual 基线按 `initial_forecast` 和初始信心（无历史 3000bp、有两年历史 6000bp）；horizon 首次恢复也形成初始 forecast，但继续保留到期前信心，并推进 Annual ID、anchor 和 `HorizonExpired` cause。已有真实 forecast 的普通 λ 修订仍沿旧 `revise_forecast` 路径，既有信心断言不变。此处的无先前收入比较项是明确隔离的合法事实 fixture，经 `PublicLibrary::from_parts` 校验后再由 NPC acquisition 读取；不声称会计生产者实际生成该无历史报告。审计没有改变预测参数、估值方法或读取策略。

## 独立复核

非作者 Luna 最终增量复核 PASS。初始缺年报状态仅在确实获取本人同范围 Annual 后形成首个年度 forecast；`WithoutHistory` 和 `TwoYear` 都按初始预测规则处理。到期时发现新获知 Annual 同样建立 forecast，并保留 horizon 原有信心策略、更新 cause/anchor/真实 Annual ID；已形成有效 forecast 的普通材料仍使用 λ 修订且不重置信心。最终 reviewer 特别确认 horizon 恢复 cause 断言与首次 baseline 语义最小且必要。仅在 `AnnualBaselineUnavailable` 状态且确实没有本人同范围年报时，horizon expiry 会推进 anchor/cause 并保留 unavailable valuation 与 interim id。普通有估值条目缺少基准仍为 `NoOwnAnnualMaterial`。

Session 测试复核确认 corruption case 先记录合法 Annual acquisition，再删除公共库中该 PublicationId；`InvalidSave(no publication)`、完整 business hash/save 不变、恢复后 summary/tax/monthly report 恰一条及重复同日推进状态不变断言均保留。合法 case 删除 Annual 时本人 acquisition 仍为空，设置实际月报排期并通过 `step()` 与 `end_civil_day()`，检查实际本人获知 Monthly、无 Annual、belief method/cause/id/unavailable 与 restore/resave。修除非法 report fixture 未弱化 PublicLibrary 校验。

Correction 的中期报告走相同 unavailable 分支，Annual Correction 本身提供基准；`CreditDefault` 仍可能因无 Annual 而致日终失败，按本批范围记录为后续缺口。最终运行记录另确认 Notices 10/10（`.tmp/company-system/session-actions/information-final-notices.log`）、Announcements 16/16（`.tmp/company-system/session-actions/information-final-announcements.log`）；这是相邻回归，不扩张 CreditDefault 覆盖范围。独立 reviewer 未运行 Cargo，动态结果以 root 提供的日志为证。

范围只涉及个人信息集、估值可用性与日终错误分类，不改变 A 股制度、Simple 财务生成或公司经济模型。
