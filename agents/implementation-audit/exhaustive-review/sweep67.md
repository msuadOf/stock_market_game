# Sweep67：hosts fixtures2 / performance / production-entry caller 后续复核

## 范围与口径

完整连续阅读 `agents/oop-refactor-implementation/hosts/review-fixtures2.md` 71行、`review-performance.md` 76行、`review-production-entry-caller.md` 22行，共169行，包含全部门禁、初审发现、修后复核、保留旧边界与运行限制。已读根AGENTS/principles；本轮仅静态核查当前owner、实际caller、测试源码与后续status，不运行测试、编译、真实性能或Git写操作。源码基线b76ece3，读取HEAD4ad5a2e；所引用performance-harness/market-ui-report/production-entry/session/plans/belief fixtures相对指定基线diff为空。

## 条款矩阵

| 原文位置/承诺 | 当前状态及代码证据 |
|---|---|
| fixtures2 1–20行：12文件完整diff、静态批准、待root运行、大A依据与日终持久边界 | **静态批准不是全回归通过，后续代表性验证已登记。** `hosts/fixtures2/status.md`末节以status.json final_validation替代旧“root待跑”；`status.json:3847`实际选定R254/R255各10秒门禁通过记录。未选suite/性能/E2E不因此变通过。本轮未重新运行，也未重新核验官方资料。 |
| fixtures2 22–35行：T+1/FIFO/零股/个人获知/总发行股/归母重述、2030模拟春节、6个stateful owner、190原测试不删/断言不弱化 | **当前owner与关键语义存在。** `civil_clock.rs:86,119`session与own due id；`fundamental_beliefs/main.rs:107,113,135,157`显式kind/issued_shares、本人独立state/book与短借用ctx；`session.rs:336`测试检查envelope/cursor/预留。未来日历仍按明确模拟日历，非新增官方休市宣称；内部SaveSlot测试不扩大ADR0025用户持久入口。历史190名称和分类统计为原审计结果，本轮未重新给全suite计数背书。 |
| fixtures2 37–41行 N05 civil-clock | **已迁移。** `tests/civil_clock.rs:86`SpringFestivalScenario实际持有GameSession/self dues，119行own_due按id过滤；测试163/185/264行真实civil/session调用与save观测，未建第二份权威市场。 |
| fixtures2 42行 N06 belief issuer/context | **已迁移。** `fundamental_beliefs/main.rs:107`发行人输入，122行new不自动acquire，142行显式本人record_acquisition，157行ctx owner校验先执行，163行BeliefInputs使用total_issued_shares。失败源码`failures/mod.rs:66,102,180`仍断言NoOwnAnnualMaterial/NotAcquired/MethodDisabled；不以fixture封装宣称总账G06/G09基本面遗漏已修复。 |
| fixtures2 43行 N08 correction/restate | **已迁移，报告简称路径需展开。** 实际文件为`tests/industry_reports/correction_restatement.rs:28`，保存Books/ClosingEngine/member与v1 bytes，115/124行调用生产close_month/year，155行原v1字节保护。不是根tests/correction_restatement.rs缺文件或漏实现。 |
| fixtures2 44行 N10 PlanScenario | **已迁移。** `tests/plans.rs:51,61,71`book/id成对拥有，accept_and_fill先真实ChildOrderAccepted再ChildOrderFilled；getters不把受理当成交。负例仍直接PlanBook调用。 |
| fixtures2 45行 N11 publication fixture | **已迁移。** `tests/publications/failures/mod.rs:36,86`合法Q1与request显式构造，139/141行caller主动改坏字段；fixture不自动修复拒绝输入。 |
| fixtures2 46行 N12 TestOrderSaveFixture | **已迁移，局限于合法测试装配。** `tests/session.rs:93,187`仅持一个SaveSlot；238行构造live envelope，266行sort，268行max(existing cursor,highest+1)保历史gap，277行只调用restore一次，279行逐account核对预留。测试336/388行明确cursor42不回退。合法fixture内部费用构造不是用户坏档自动修复caller。 |
| fixtures2 48–59行：F1中文注释修复、issuer参数收敛、TradingPlan坏status JSON编辑 | **原审查已关闭，当前接口一致。** main issuer结构保持显式kind/发行股；`plans/state.rs:202,214,258`只读返回原Copy字段。测试负例只改status后deserialize，不施加额外PlanEvent来“治好”坏输入。当前合法fixture校正cursor不流入生产restore validator。 |
| fixtures2 61–71行：diff-check、190→191/12文件统计、没有Cargo、root短测后才宣布完成 | **后续状态已收口为代表性验证。** `fixtures2/status.json:3849,3858`引用root rust-short-increment-02-result，selected case通过；`status.json:3867–3872`明确未全量/E2E/性能。旧“新增case尚未执行”历史limit不是今天仍必定未跑；也不因build/check通过宣称191个case全部重跑。 |
| performance 1–10行：6文件完整diff/4action工具owner、已修3个发现、无交易算法改变 | **当前修后owner存在。** `escrow-performance-harness.mjs:325,481`ProcessSampleRun/PerformanceComparisonRun，`market-ui-report.mjs:234`MarketUiReportRun；资源金额仍分/股、守恒由原contracts格式校验。没有新交易制度或真实性能结果可由类提取推导。 |
| performance 12–24行 P2-01：公开appendSample伪造可复用PASS | **已关闭。** 492行#measureSide只通过source-before→runSample→source-after→validateMeasuredSample；507行#appendSample私有且structuredClone；511/517行唯一正常测量追加；521/523行failed/incomplete拒report。生产`runPerformanceHarness:602–604`先measure再report；未测run不能借append旁路生成PASS。测试`harness.test.mjs:441,445`保禁止公共追加。 |
| performance 26–36行 P2-02：返回report共享#samples可变别名 | **已关闭。** `harness.mjs:533`完整report structuredClone，含sample/config/environment；507行接收sample也clone。测试450–461行修改嵌套histogram/数组/workload/command/environment后再report不受影响。 |
| performance 38–46行 P3-01：facade同步抛错破坏Promise拒绝 | **已关闭。** `harness.mjs:418`恢复async facade，无默认options吞错；constructor341行仍必需参数。测试466–467行固定缺少/null options rejected Promise。 |
| performance 48–54行：cash分/shares股、无上界canonical BigInt、P0/P1/journal/positiveFill/T+1、无跨envelope序、stateful resources | **owner提取保留既有契约。** `escrow-verification-contracts.mjs`仍为Resource/EnvelopeConservation局部state；current-run调用是否完整不能由owner存在推导。旧helper只有tests caller已由test-cleanup12批准KEEP；总账G39跨worker完整artifact比较仍须另修，不能以本次BigInt/journal等价迁移核销。 |
| performance 56–60行：sampler失败延后、UI cleanup截断/Linux仅SIGTERM、reuse未逐样本重验 | **明确保留的当前残余边界，详见下表。** 本轮核对真实main/测量caller仍会到达这些路径，未把“review通过”写成这些行为已修复。也没有把actions明确排除的行为升级为本次重构未履约。 |
| performance 62–76行：初审内存fixture、35旧测试非真实bench、修后3项关闭、精准7短测/完整diff-check通过 | **历史精准验证记录，不是本轮运行。** `performance/status.md`保留初版35和修后7不同批次；末节root编译/精选证据只覆盖其明示范围，未重跑真实性能/浏览器。当前代码对应三项修复仍在。 |
| caller 1–6行：example整文件两行diff/sha与三个getter | **当前caller已接。** `production_entry_performance.rs:197`contains(plan.account())，198行active_child_order_id() OR filled_qty()>0，202/214/258 getter直接返回原字段；旧文件hash是当时冻结来源，不将本轮未hash重新算成通过。 |
| caller 8–12行：account身份/真实累计股数/存在子单OR成交、仅getter迁移、无default/mutation | **当前条件未漂移。** 197–200行计满足条件的plan数，不把计数解释为股数或OrderAccepted成交；没有更改交易制度或资金单位。 |
| caller 14–22行：JSON schema/各来源活动、timer/save统计在计时外、seed/RNG/100股/自然日/worker/error不变、无运行验收 | **当前真实生产入口存在，性能仍需实测。** 125行end_civil_day、138行enqueue玩家100股，151行step_with_phase_timing；189行run计时结束后190行save和191行plan汇总，203–209行三来源活动不足显式报错。类getter适配不证明完整tick加速或benchmark通过；总账R15已限定此结论。 |

## 真实 caller 上仍保留的旧边界

| 残余 | 当前证据与分类 |
|---|---|
| sampler rejection不能及时终止存活child | `harness.mjs:391`启动sampler，395行先await child close，400行才await sampler；真实`runPerformanceHarness:597/603`默认runProcessSample会到达该路径。sampler先失败时没有在此处分支先kill child。原review-performance56行、performance/actions.json的ProcessSampleRun动作明确排除行为修复；这是已知工具失败生命周期边界，未被三项ownership修复关闭。外部deadline约束仍需调用方保障；本轮不运行存活child反例。 |
| Market UI清理一步失败截断后续、Linux只SIGTERM无等待 | `market-ui-report.mjs:412`main finally调用run.close；306–311行client.close→stop browser→stop server→rm(profile)无独立错误收集或继续清理；224–231行Linux只发送SIGTERM。原review明确不增加幂等、聚合、树等待。属于已知排除行为边界，不报为本批类提取新缺口，也不宣称资源完整回收已保证。 |
| reusable PASS验证不逐样本重验 | `harness.mjs:565`仍按schema/status/config/environment/endpoint/count/comparison核对，588行只验sample数量；未逐个重复validateMeasuredSample。原review59行保留旧合同。新buildReport所有权修复保护本run，不能据此把外部既有report全量证据真实性写成已核验；若要扩大reuse合同须独立决定。 |

## 候选反证

1. **未测PerformanceComparisonRun可公开追加伪PASS：已修复，不能重开。** 私有append与完整measured校验、failed/incomplete guard有实际caller及回归源码。
2. **消费者改report反向篡改run：已修复。** 双向clone隔离完整结构，不只是clone数组外壳。
3. **options缺失facade同步抛错：已修复。** async wrapper保Promise rejection，不以默认参数掩盖非法输入。
4. **TestOrderSaveFixture自动修坏档扩大用户入口：反证。** tests限定合法装配，生产GameSession::restore仍直接validator；坏档case绕过消费式fixture restore。合法cursor max保历史间隙，不是生产重编号。
5. **fixtures2仍绝对未运行新增case：后续记录核销。** final_validation明示R254/R255通过；旧review没有宣称当时已跑。未选全suite仍未验证，不扩大历史通过范围。
6. **计划活动指标等同成交股数：反证。** contains账户与active child OR真实filled_qty条件计plan数，字段名plan_children_or_fills与当前表达一致。
7. **对象化通过等于sampler/UI清理/reuse全部修完：反证。** 上表三残余在当前真实caller保持，原action/review已明示排除；本轮如实保留，避免漏掉已知未修行为，也不制造新需求。

本轮新增确认的本批承诺漏实现为0。三项performance有效发现现已关闭；三项旧工具行为边界仍保留，代表性root验证与未执行suite/真实matrix/性能/E2E分别记录。仅新增工作报告，主审需独立复核完整diff。
