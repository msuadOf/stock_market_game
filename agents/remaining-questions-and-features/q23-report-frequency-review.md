# Q23 月报排期独立复核

本次复核由未实施月报改动的 reviewer 完成，针对 main 共享工作区当前未提交的月报排期及披露接线。完整阅读 `monthly_schedule`、`schedule`、`publication`、`public_view`、`prehistory`、`disclosures`、`intraday_disclosures`、`company_groups`、`company_assembly` 及 `report_frequency` 测试；读取 `session`、`persistence`、`candidate_commit` 的相关完整 diff，并沿实际查询、`observation_clock`、`notices`、`fundamental/update` 继续核验。其他任务的税务级联、多人、Q13 与 Q22 改动不归本项。

## 领域与范围

月报被明确登记为游戏额外报告，保留 `ScheduledReportKind::ALL` 四类定期报告，没有把 A 股公司错误描述成具有法定月报义务；正式文档区分法定披露窗口与游戏排期。预设／自定义日期时刻、可选 seed／公司／期间绑定延迟，是用户要求的必要范围。新增 `Monthly { schedule }` 严格契约，无旧档兼容或自动补字段。单体前史真实封月、live 集团月报成员封账检查、日内 candidate 原子提交路径在静态结构上符合需求。两项初审 P1 和增量发现的一项 P2 均已根修，当前月报范围的静态独立复核通过。

## 有效发现

1. **P1：真实 NPC 获知月报会进入不支持的年度材料路径。** `notices::deliver_public_information` 将任意 `AcquiredKind::Report` 作为新估值材料，`preferred_own_report` 按最新会计期间选择材料，因而可选中 `Monthly`。`fundamental/update::apply_material` 仅将 `Quarter`／`HalfYear` 路由到中期处理，其余调用 `ensure_annual_for`；月报不是 `Annual`，会显式报错并导致日内提交或自然日日结失败。现有日内测试把全部 NPC 设为零，没有覆盖这个生产路径。最小修复需明确定义月报在本人估值链中的处理：支持同范围单月同比材料而不把单月利润简单年化，或者明确月报仅用于公开／获知并从当前估值材料候选排除；不能将月报冒充年度材料。至少补一个真实 NPC 关注发行人后，月报公开、本人获知与处理结果的短测。

   修复增量已将 `Monthly` 接入 `extract_interim_growth` 与 `apply_interim_material`，读取同范围单月收入及上年单月比较项，并继续使用本人年度基准，没有乘12年化利润；年度更正再消费最新中期材料的分支也包含月报。真实 `Inst` 测试按本次 `CompanyDisclosurePublished` 的实际 `PublicationId` 核验本人获知时点、`used_report_ids` 和 `NewMaterial`，不再误选同一时刻获知的旧前史报告。该根修静态通过；实际执行结果由 root 记录。

2. **P1：最后交易 tick 后玩家报告查询时间回退。** `candidate_commit` 为最后 tick 特判月报公布截止为当日15:00，但查询仍使用 `observation_civil_instant`。交易日日终推进 `day` 后，该函数的 `tick - day * ticks_per_day` 回到零，同一个尚未自然日日结的 `civil_date` 被解读为9:15。已于10:30等时点公开的报告会在最后 tick 后重新不可见，按 ID 查询也会触发前视拒绝。应使用统一且单调的实际观察时点，或在当前自然日已完成市场 session 时返回15:00；补公布后贯穿最后 tick、自然日日结之前仍可列表／按 ID 读取的短测。

   修复增量仅在 `day_tick == 0`、已完成至少一个 session、`CivilPhase::IntradayTrading` 且已完成当前自然日应有 session 时返回15:00；新自然日未开盘不会误判为昨收盘。日内测试推进完120 ticks 后仍按实际报告 ID 读取，并按 DTO 原有完整期末日期 `2029-12-31` 断言，没有修改生产契约。该根修静态通过。

3. **P2：机构普通观察入口漏掉 Monthly 材料分类。** `decision_chain/roots::observe_personal` 的 `is_report` 仍只包含 `Annual`／`Quarter`／`HalfYear`。机构在普通观察中首次获取月报时会写入本人 acquisition，却不登记 `new_reports` 或对应月报更正；后续订阅送达因已有 acquisition 又跳过，可能永久遗漏该材料的更新。需将现已支持的 `Monthly` 加入同一报告分类，并以并非公布时订阅投递、而是本人之后首次观察获知的路径补一个代表性短测。散户观察路径读取任意已获知报告，无相同硬编码遗漏。

   最终增量仅为生产分类增加 `Monthly`，未增加特殊旁路。新增真实 `InstitutionDecisionRoot::observe_personal` 用例先清空关注列表，公布月报后确认本人未获知，再于本人观察中获取并核验实际新月报 ID、获知时点、`used_report_ids` 和 `NewMaterial`。用例同时明确前史 ID0 是真实 `Monthly` 报告且不是公告，固定原来漏分类误走 `credit_default_cause` 的 `UnknownPublication` 根因，而非通过删材料掩盖它。该修复静态通过；作者提供的 root27 短测通过结果由 root 保存原始执行证据。

## 验证边界

`report_frequency` 当前测试覆盖配置严格形状、默认季度四报告保留、可选延迟确定性／范围、10:30公布及下一 tick 不重复、最后 tick 查询、真实机构获知及使用报告。新增21:00月报在元旦休市日派发，明确 tick 不推进，游标达到23:59:59，恢复后推进下一交易日验证不重复公布、查询只有一份目标月报。故障 hook 已移至 private candidate 月报公开与本人更新完成之后、commit 之前，核验权威 library／个人 information 与 belief／业务 hash 不变；cursor 与 closing 属于同一可丢弃 candidate。机构普通观察入口已有上述独立用例。集团代表性测试先确认成员尚未封账时明确拒绝且 library 为空，再对母公司与子公司真实 `close_month`，按自定义9:50公布并核验 `Monthly`、`Consolidated(root)`、公开与批准时点；没有把单体报表冒充合并报表。无需完整回归。

本 reviewer 未执行 Cargo、编译、测试或网络访问，不将静态代码存在当成验证通过。两项 P1 与一项 P2 的修复均经增量静态复核通过；月报范围符合上述大 A 语义、需求必要性及原子边界。实际短测执行证据由 root 记录。用户另行提出的同期间报告种类排序正在独立 TDD 批次，未纳入本次月报 gate，不将其尚未实现误报为月报回归。
