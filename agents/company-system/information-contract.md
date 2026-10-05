# Simple 公开材料与本人估值接线

公司模型以 `CompanySystem` 为共同入口。最新公司设计要求 `Simple` 与未来 `Simulation` 提供同一上层财务与股本功能；简化的是具体客户、工厂、研发等内部经营过程，不是报告或结算能力。`Simple` 按营收及开支推算利润，复用唯一会计算法形成相互勾稽的完整报告。

## 共同材料

`PublishedReport` 沿用唯一 `PublicationId`、公开库、公布时点、会计政策、完整 `ReportSet` 和本人获知登记。新增必填的 `PublicationSource`：`SimpleGenerated` 说明汇总生成来源，`SimulationAccounting` 说明经营仿真来源。来源不改变财务字段、报告期间、范围、更正或估值方法；缺少来源的旧存档明确拒绝，不做兼容。

公开 DTO 沿用完整 `accounting` 和 `financials`，仅增加同一来源字段，不按模式提供不同业务结构。所有会计金额为十进制元字符串，日期为 ISO 自然日期。更正理由、版本关系和报告窗口继续由既有共同报表结构承载。

## 公开与个人获知

发布沿用唯一 `ReportFrequency` 的年报、半年报、季度报告和可选月报排期。前史以开局前一日为公开边界，不提前发布未来材料。更正关联原公布 ID，不覆写历史材料。汇总财务的公开必须来自真实定稿登记，不能根据少量指标手工伪造完整三表。

玩家查询只见当前时点已公开材料。NPC 仍须通过本人 `NpcInformationState` 和 `NpcObservationContext` 获取材料，公共缓存生成或其他 NPC 阅读不增加本人经历。同期间保留 `Annual > HalfYear > Quarter > Monthly`，报告范围与版本顺序沿用已批准规则。

盈利、权益 `ROE` 和现金流分析继续消费同一已获知报告；亏损、缺历史、非正分母及不确定的融资拆分按既有类型化不可用处理，不自动更换分析方法、不偷读未公开私账。来源标签不是禁止 `Simple` 使用某种财务功能的开关。

## 资金与验收范围

用户进一步明确 `Simple` 只是账面展示，不追踪公司真实资金流向，不设置每月 `funding` 金额。营收及开支按月度百分比变化，财务结果必须标明汇总生成来源，不冒称真实客户付款、融资来源或经营经历。不得把展示指标直接写入投资者交易账户；股本行为仍通过共同规则完成。蓝图中拟议的 `SyntheticFunding` 网络不作为当前接线方案。

此前仅包含少量指标、无现金流的 `SimpleReportFacts` 和不同报告 payload 方案已取消，相关实施 hunk 已精确撤回。本记录描述当前实施契约，不代表定稿生成、公开接线或完整股本结算已经完成。验收以真实短单测和未实施者完整 diff review 为依据，不做复杂回归测试。

信息边界的真实 TDD 证据由 root 统一编译实际 `engine`：先将原始 `source_tests.rs` 链接到生产产物，`host55-information-source-red.log` 中三项为一绿两红，明确暴露跨来源更正和恢复缺失更正目标；`host58-information-red.log` 中六项为四绿两红，前两项根因已关闭，仍暴露公开来源隐藏更正版本及未接通的披露 helper。随后实现共同版本形状守卫和只读定稿材料的原子披露，并新增到期公开、提前不可见及重复扫描幂等的短测。fresh `engine-eb8b0dcc57b8995e` 的 `host60-information.log` 实际执行七项，全部通过，耗时不足一秒，无跳过；该证据只覆盖来源、月度披露基础与更正边界，不表示任意生成周期或完整股本功能通过。

长期生成周期的短报告缺失必须明确区分“尚未生成”与真实零值，不把长周期结果均分为虚假的月度事实。共同 `PublicReportAvailabilityQuery` 明确指定公司、自然月末、报告种类和 `scope`；只从该时点已公开的同范围材料返回 `Available`。未公开时只返回 `BeforeOpening`、`NotYetSettled`、`PeriodNotRepresented`、`NotYetPublished`、`NotScheduled` 或 `ScopeNotRepresented` 原因，不返回内部指标或假报告。即使模型内部已经结算，历史查询时点尚未结束的期间仍按 `NotYetSettled` 表示。

新增可用性 TDD 在 `host63-information.log` 中实际执行十项，八绿两红；两项业务红明确来自尚未接通的查询 stub，日期和范围拒绝控制没有被冒称为业务红。随后接入共同公司系统的真实期间可用性，并保留实际公布、提前读取与报告范围守卫。fresh `host64-short-tests.log` 的信息组实际十项全部通过，耗时 0.02 秒，无跳过；其中包括年度生成周期不伪造月报及未公布数据不返回完整报告。该结论限定当前共同查询与信息边界，不表示整个公司或股本系统全部完成。
