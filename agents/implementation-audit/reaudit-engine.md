# Engine OOP 重构后的历史缺口复核

- 复核基线：`8cf34a1ce2d893f003e1d4c34d7c2bea170dd4cb`（`refactor(engine): 完成全仓 OOP 状态与行为聚合`）。
- 对比范围：`b89afb3346743a4b4fccf26c9ac9ff108595f696..HEAD`；只审查，不改产品代码，不运行全量测试。
- 需求来源：完整阅读 `agents/implementation-audit/implementation-audit-2026-10-02.md`、`docs/superpowers/plans/2026-09-10-company-information-npc-intentions.md`（700 行）与 `docs/decisions/0026-individual-institution-experience.md`（71 行）；并阅读 `AGENTS.md`、`docs/principles.md`。较新计划及 ADR-0026 约束优先于审计旧判断。
- 后续已核对 `8cf34a1..dddcc31` 的全部engine生产差异：decision_chain仅移除失效注释并修测试冗余借用，institutional_behavior仅修测试Copy使用；decision_snapshot_capture将独立观察副本的Arc封装提前，仍调用legacy observe_position，观察内容、错误顺序和权威提交不变。`ebfb68b`产品代码与dddcc31相同，因此本记录G/Q结论未因这些改动核销；以下原详细行号仍对应8cf34a1，decision_chain注释之后位置缩短3行。
- 审查重点：G06–G09、G16、G28、G35–G38、Q02、Q11；另追踪 R04/R05/R11 涉及的策略、个人信息、经历调用链。OOP 重构后按当前 owner、调用方及实际生产入口判断，不以旧符号移动/消失作为结论。
- 限制：这是当前源码静态复核，非行为运行、编译、A 股官方来源重查或独立交易制度取证；未阅读全部 2026-10-03 OOP 子审查文档，只将相关 experience/operations 审查记录用于交叉检查，不把其范围外结论扩张为本次证明。

## 结论摘要

| 条目 | 当前结论 | 复核依据 |
|---|---|---|
| G06 | 仍缺，非 OOP 重构引入 | 现金流估值的五年显式现金流仍各只折现一次；见下文。 |
| G07 | 仍缺，非 OOP 重构引入 | `InstitutionDecisionRoot` 的五路信号消费只属于机构信念簿；Retail 仍由 `ZiNoiseStrategy` 调用零售行为内核。 |
| G08 | 仍缺（已有局部能力） | 机构真实失败与风险经历的日期/衰减属于 b89afb3 前已存在能力；本次未证明相对 b89afb3 有新修复。散户生产观察/成交仍调用 legacy writer，散户日期与20交易日衰减仍未接入。 |
| G09 | 仍缺 | Root 只把本人新获知年报放入 `new_annual_reports`；`BeliefBook::apply_material` 强制 Annual，季度/半年材料无更新路径。 |
| G16 | 仍缺 | 历史计划簿复制从旧 session 函数移到 `RootReadContext::capture`，仍按 root 批次克隆完整 `PlanBook`；没有证据表明所有权/COW 已消除此复制。 |
| G28 | 仍缺 | 生产披露构造 `ScopeId::Standalone` 并仅对该报告登记；Consolidated 请求仍未进入日终披露。 |
| G35 | 仍缺 | 新 `OperatingDayRun` 仍调用原行业日流；工商日常处理没有折旧、所得税和商业债务支付；利息计提已接入，不能说完全没有利息实现。 |
| G36 | 仍缺（已有局部能力） | 四行业业务委托在 b89afb3 前已存在，本次重构未构成新修复。新游戏装配依旧固定 Industrial；抽样公司冲击未按 CompanyKind 过滤，公告按公司公开。 |
| G37 | 仍缺 | DEV trace 的真实订单事件实参仍为空切片；预算限制仍取终止计划状态，不是分配约束事实。 |
| G38 | 新旧续行/机会分类未接入（独立于经历信心） | Allocation API 有 ExistingPlan/NewOpportunity 优先级，但生产调用只构造 ExistingPlan；不得把 `AllocationExperience::default()` 本身列作新缺口。机构经历影响及信心已在上游分析/报价处理，重复接入通用分配惩罚会造成双重惩罚。 |
| Q02 | 待定边界，当前仍无生产读取记录 | `record_public_history_read` 只见定义和测试调用；当前 root 观察记录行情观察，不记录主动读取公开历史的事件。该项仍保持 Q02 的开放问题性质，不提升为本轮新确认 G。 |
| Q11 | 待定边界，当前仍无生产 cause 分发 | Correction/CreditDefault cause 与 direct update 原语存在；生产 root 只发年报 `NewMaterial` 和 HorizonExpired，未按更正/真实违约公告路由。仍按 Q11 登记，不作为本轮新确认 G。 |

没有把以上历史缺口归因于 OOP 重构；其主要 owner 提取保留了缺口。以下另记到 G36 的跨层遗漏：实际 `CompanySpec` 行业被用于经营委托/参数校验，但随机 CompanyShock 仍可能为银行、保险发布仅适用于工商/地产的中断或减值公告。这是生产披露范围问题，不证明默认工商局以外已发生运行失败。

## 逐项证据

### G06 — 五年权益现金流折现仍缺

`packages/engine/src/strategy/fundamental/valuation.rs:156-169` 先逐期更新 FCFE；第 159–164 行每个年份都将当年 FCFE 除以同一个 `(1+r)`，没有按年份幂次折现；终值才在 168–170 行连续折现五次。由当前实现可确认历史数学问题仍在。本条并非接线缺失：机构候选生成读取个人 `BeliefBook` 估值（`packages/engine/src/session/decision_chain/roots.rs:117-122`），年报更新在 `roots.rs:430-447`。

### G07 — 身份与基本面分析能力仍耦合

当前五路信号构造位于 `packages/engine/src/session/decision_chain/roots.rs:65-98,100-184`，输入来自 `BeliefBook::analysis()` 和条目估值（74、118–122 行），该 root 的生产调用处于机构参与者通道：`PlanPersonalState::take` 从 `belief_participants` 取得个体信念（`packages/engine/src/session/decision_chain/personal_state.rs:68-87`），`InstitutionDecisionRoot::observe` 调用该分析链（`roots.rs:198-221`）。散户生产决策仍在 `packages/engine/src/session/pipeline/npc_decisions.rs:169-199` hydrate `StrategyState` 并执行 `decide_with_experience`；其 `ZiNoiseStrategy` 继续调 `decide_retail_position_with_experience`（`packages/engine/src/strategy/zi_noise.rs:188-207`），没有消费 `AnalysisProfile` 或个人财报估值。诊断 `assessment_debug`（`packages/engine/src/session/decision_chain.rs:1049-1101`）不构成散户生产接线。

### G08 — 散户日期/衰减链仍未接线；机构局部能力非本次修复

散户观察仍通过 `experience.observe_position(&code, price, market_minute)` 更新（`packages/engine/src/session/pipeline/decision_snapshot_capture.rs:343-348`）；散户成功成交仍走 `record_fill_with_order`（`packages/engine/src/session/pipeline/retail_projection.rs:518-528`）。新增 `observe_position_dated` 位于 `packages/engine/src/experience/feedback/lifecycle.rs:335`，本次生产调用搜索未发现调用方。失败衰减方法 `failure_influence` 虽实现 20 交易日一档（`packages/engine/src/experience/feedback/inputs.rs:10-21`），但生产调用仅从 `read_allocation_experience` 委托（`packages/engine/src/plans/allocation/experience.rs:17-34`）；该读取 API 在 session 无调用，分配请求仍填 `AllocationExperience::default()`（`packages/engine/src/session/decision_chain.rs:971-982,1235-1246`）。

机构 ADR-0026 路径是不同契约且已接入：本人 root 在 `packages/engine/src/session/decision_chain/roots.rs:283-303` 观察个人持仓并更新账户风险；真实机构成交投影在 `packages/engine/src/session/pipeline/retail_projection.rs:529-607` 登记费用、成交和获利退出。对比范围起点 `b89afb3` 已具备这些个人日期/失败衰减能力；OOP diff 只是搬移/调整 owner 接缝，不能记为本次新修复。不能用机构实现核销散户 G08，也不能要求重做机构衰减。

### G09 — 个人已知中期报告仍不能更新预期

个人发现会记录新公告/报告（`packages/engine/src/session/decision_chain/roots.rs:344-395`），但只有 `ReportKind::Annual` 才加入 `new_annual_reports`。`BeliefCause::NewMaterial` 在 `packages/engine/src/strategy/fundamental/update.rs:22-35` 语义写作年报，`apply_material` 调 `ensure_annual_for`，后者在 242–260 行拒绝非年报；facts 入口亦为 `extract_annual_facts`。因此发现季报/半年报并不更新财务预期。当前公开排期本身包含这些定期材料，不能把它们当全年报替代。

### G16 — 历史计划复制仍存在

重构将共享 root 输入明确封装为 `RootReadContext`，但 `capture` 仍克隆全部 `session.state.plans`（`packages/engine/src/session/decision_chain/roots.rs:17-29`，第 25 行）。`PlanChainOperationBatch::start_ready_accounts` 每个 root 批次调用一次 capture 并封入 `Arc`（`packages/engine/src/session/plan_chain_candidates.rs:300-330`），避免逐账户各自复制不代表普通 tick 不复制完整历史计划簿。当前静态证据不能证明克隆是性能瓶颈，也不能据类封装宣称目标已经完成；建议保持“所有权优化未完成、性能未测”的原边界。

### G28 — 固定集团报告未进入生产披露

日终生产 `publish_scheduled` 仍构造 `ScopeId::Standalone(member)` 并发布 standalone（`packages/engine/src/session/disclosures.rs:155-200`，尤其 182–190 行）。`ClosingEngine::snapshot_interim` 只创建 standalone（`packages/engine/src/accounting/closing/mod.rs:211-235`）；`generate_consolidated`/报告层是否有算法不改变调用链事实。当前没有在该生产披露路径根据固定集团生成 consolidated scope、对应期间版次及公开索引，故报告算法存在不能核销 G28。

### G35 — 工商期末经营闭环仍不完整

OOP 新 `OperatingDayRun::advance` 只按阶段调用 shock、due、行业 flow、次日利息排队（`packages/engine/src/company/operations/day.rs:79-85`）；工商 `advance_day` 调产销、补货、管理费、坏账准备（`packages/engine/src/company/operations/industrial.rs:85-176`），不见 `apply_depreciation`、所得税计提/支付或商业债务本金/利息支付。利息计提已经由 `CompanyOperations::accrue_interest_for` 按行业分派（`packages/engine/src/company/operations/dispatch.rs:33-56`），所以旧文案应继续区分“计提已有”与“支付/期末处理缺失”。日终会在 `CompanyOperationsClockWiring::run_day_end` 调用自然日经营（`packages/engine/src/session/company_operations.rs:76-87`）；但这不是把被省略的工商业务补全。

### G36 — 四行业会话装配与冲击适用仍有缺口；四路委托非本次修复

新局列出证券时 `assemble_listed_company` 仍为每只证券构造 `CompanyKind::Industrial`（`packages/engine/src/session/company_assembly.rs:331-345`）；即使通用 operations 有 Bank/Insurance/RealEstate 值对象及四分支处理器，也不表示默认 `SessionSetup` 能装配四行业发行人，或定制公司可经会话披露查询闭环。四行业 `IndustryPairView`/flow delegate 在 `b89afb3` 已有，当前 OOP 对它们的提取不是相较该基线的修复。

新抽样 `sample_company_shock` 按统一目录抽取 `ProductionInterruption` 和 `AssetImpairmentSignal`（`packages/engine/src/company/events.rs:209-235`），不接收行业类型；日推进把抽样事件直接加进全部公司的 `economy`（`packages/engine/src/company/operations/day.rs:135-145`）。地产/工业对其中部分效果有处理，银行/保险则可能收到并通过 `session/disclosures.rs:107-124` 形成公开公告。事件目录明确适用范围仅工业/地产生产中断、仅工业固定资产减值（`packages/engine/src/company/events.rs:12-20`）。需要显式过滤或诚实标注只记录；不据此声称发生了错误证券交易或默认游戏已实际触发该事件。

### G37 — DEV trace 仍没有真实订单关联

trace writer 能从给定 `events` 取本账户 OrderAccepted/OrderCanceled ID（`packages/engine/src/session/decision_chain.rs:774-811`），但 `record_plan_root_diagnostics` 的生产调用固定传 `&[]`（同文件 754-770 行），当前新异步 coordinator 仍调用该方法（`packages/engine/src/session/plan_chain_candidates.rs:371-377`）。因此不是订单 ID 符号不存在，而是这条 trace 生成时没有被提供交易事件。`budget_constraints` 仅从 terminal plans 的 status 派生（decision_chain.rs:812-817），也不代表真实 `AllocationConstraint`。保留 DEV 隔离已有事实，不把空 trace 误称未提交的交易事实。

继续核对至 `2247f4f`，上述生产路径未变。`diagnostic_parity.rs` 改为同一会话中重复查询前后save字节不变、trace稳定且有界，并使用交易日与较频繁观察的Growth账户；这验证查询只读，不验证trace包含实际受理订单ID。`causal_diagnostics.rs` 改用三个交易日、显式跨休市日的真实成交fixture；`diagnostics.rs` 测试改为逐次报告核对seed、统计及成交数量守恒，不再断言自由调度整局相等。生产报告注释明确seed不含实际受理轨迹；微结构 `DirectionPersistenceAccumulator::consume` 将嵌套匹配等价合并，仍只消费Execution中有side的事实，逐股方向及pairs/same计数不变。没有补上诊断订单关联，不能据此核销G37；本轮未执行这些Rust测试。

### G38 — 续行与新机会预算分类未进入生产请求

预算排序实现 `ExistingPlan` 优先于 `NewOpportunity`（`packages/engine/src/plans/allocation.rs:115-126`），但当前生产提交点 `decision_chain.rs:971-982,1235-1246` 都标 `ExistingPlan`。全仓目前没有 `AllocationClass::NewOpportunity` 生产构造调用（只见优先级分支定义），故续行与新机会竞争的分类仍未进入分配器。这是同一账户计划分类，不应泛化到不同账户撮合。相同调用点的 `AllocationExperience::default()` 不列为缺口：机构真实失败/信心已在其上游分析与计划决策中消费；把它再传给通用 allocator 会重复施加惩罚，不是 G38 的实现要求。

### Q02 — 开放边界核对：主动读取公开历史仍无生产经历记录

`PersonalPriceMemory::record_public_history_read` 当前定义在 `packages/engine/src/experience/price_memory.rs:164-180`；生产调用搜索未见其调用点，仅同文件测试调用（约 209–237 行）及 `watchlist.rs` 的测试。根观察链只调用 `observe_price`（`packages/engine/src/session/decision_chain/roots.rs:321-337`）。因此读取发生记录仍未连接；实际公开技术数据消费不能自动等同于 NPC 曾主动读过历史。

### Q11 — 开放边界核对：更正与违约的直接重估没有生产 cause 分发

`BeliefCause::Correction` 与 `CreditDefault`、`apply_credit_default`、`apply_material(... direct)` 已在 `packages/engine/src/strategy/fundamental/update.rs:22-35,91-130` 实现；更正/违约事件 ID 有可承载原语。生产 root 仍把本人刚获得的新年报一概传为 `NewMaterial`（`packages/engine/src/session/decision_chain/roots.rs:430-447`），其余生产 cause 为 HorizonExpired（449–460 行）；这里未见把更正报告版次映射 `Correction`，或将明确 `CreditDefault` 公告映射专用 cause。因此 correction 走普通 λ 路径、CreditDefault API 无实际入口的历史判断仍成立。信用恶化 `CreditDeterioration` 是不同公告语义，不能无依据升级成违约。此项继续作为 Q11 待定边界，不提升为新增 G 缺口。

### G28 补充边界 — 抵销申报未校验实际余额上界

G28 生产集团披露未接通之外，原审计登记的 Task12 F6 上界约束仍缺：`precheck_balance`（`packages/engine/src/accounting/consolidation/eliminate.rs:84-105`）只验证成员、自指、科目及现金；配对 `balance_entry` 在 122–126 行验证两侧申报金额彼此相等，未读取相应成员的实际科目余额。`ConsolidationRequest` 将账簿与 `intercompany_balances` 分开输入，执行路径只把申报交给 `build_worksheet`（`packages/engine/src/accounting/consolidation/mod.rs:84-111`）。相等申报仍可能超过双方账面余额；不据此声称当前会话已发布或默认游戏已发生超额抵销，因为 G28 的集团生产接线尚缺。

### G36 补充边界 — 非工商 `books_mut` 的 unreachable 仍在

`IndustryBooks::books_mut` 仍只接受 Industrial，对 Bank/Insurance/RealEstate 执行 `unreachable!`（`packages/engine/src/company/operations/config.rs:36-46`）。当前会话封账明确仅对上市工商公司可达，且历史范围不要求任意非工商强行走此接口；故它证明四行业会话封账接线仍缺，但在该生产前置条件下不单独判作默认运行时新 panic，也不误报为 OOP 新增。

## R04 / R05 / R11 与重构新增遗漏核对

- **大 A 执行语义：** 重构中的 `RetailDecisionContext` 将散户追势判定提取成临时对象（`packages/engine/src/strategy/retail.rs:139-230`）；仍读取本人 `sellable_qty`，`StopLoss/TakeProfit` 形成卖单时受可卖数量约束，T+1 不可卖仍返回无单，买单仍走 `affordable_buy_qty`。当前代码和其 `ZiNoiseStrategy` 调用保留金额分、数量股、`LimitPrice::Highest/Lowest` 的既有执行口径。未发现此次提取改变交易所规则或添加强制止损/虚拟成交；这只是静态 diff 判断，官方规则仍按文档适用日期，不声称本轮重新核验法源。
- **Retail 参数及随机调用：** `ZiNoiseStrategy::strategy_data`（`packages/engine/src/strategy/zi_noise.rs:40-52`）集中映射全部策略参数；`retail.rs` 先在调用方抽取追势门，再执行纯分类。其文件新增边界测试静态断言 RNG 消耗，但本轮未运行测试；当前完整差分未见参数缺映射。
- **个人认识：** RootReadContext 仍只为机构 DecisionRoot构造 `NpcObservationContext` 并按 accepted 候选执行 `record_acquisition`（`roots.rs:344-405`）；交易行情观察单独走个人价格记忆。没有看到 OOP 改成全体共享读取；同时 Q02 的主动历史读取专门事件仍未接入。
- **经历：** 本次读到的独立 `experience-review.md` 记载其 reviewer 已复查新增 owner writer 与机构观察接线，并明确表示未找到公开历史读取生产 writer，与本次 Q02 结论一致。该历史 reviewer 对 G08 散户日衰减和所有 session producer 未作全量核销；本结论来自当前 caller 搜索及上列源码，不把 reviewer 的狭窄审查范围扩成全链证明。
- **OOP 新 owner 核对：** `RootReadContext`、`InstitutionDecisionRoot`、`PlanRootCoordinator`、`StockRouteCoordination`、`RestatementRegister`、`OperatingDayRun` 等新 owner 均追到调用方及持久化/对账边界。没有把“提取成功”当行为核销。可确认本批并未修复上列缺口；本次没有发现足够证据证明这些抽取本身改变 A 股价格/数量/投资者现金语义。对未运行的保护测试及全量测试不作通过声明。
