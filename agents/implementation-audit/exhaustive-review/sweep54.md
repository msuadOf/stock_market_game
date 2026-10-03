# S54：diagnostics 与 experience OOP 后续记录全文复核

## 范围与全文记录

2026-10-03，产品基线 `b76ece3`（审计 merge worktree）。全文连续读取 `agents/oop-refactor-implementation/domain/diagnostics-result.md` 138 行、`diagnostics-review.md` 120 行、`experience-result.md` 53 行，共 311 行。初次组合输出截断后，单独重新连续读取 diagnostics-review 全部 120 行，未用REJECT/APPROVE搜索替代正文。根 AGENTS/principles 已读；本轮只新增工作记录，未改产品/未执行 Git 写操作/未跑测试或构建。

工作记录是当时 OOP owner/caller 迁移与复核证据，不改变正式公司计划 K5/K7、ADR0013/0017/0026 和现行受理语义。原文引用的 `agents/oop-refactor-audit/challenge-2026-10-03/action-index.md` 在当前树不存在：无法据缺失权威正文扩大动作范围；本次按三文中可定位的具体动作与正式计划核对。链接缺席是历史记录导航债，未视为缺游戏功能。

## diagnostics-result 全章条款矩阵

| 原文位置/动作 | 当前 caller → owner → consumer 证据 | 状态 |
|---|---|---|
| :3–8 基线/六文件/N41 caller/无运行 | 后文:136追加root定向短测结果；源码当前有后续提交 | 当时状态与后续状态同时保留，不能仅截初始“未运行”说一直没测。 |
| :10–20 阅读/金额/股数/双边量/事实序号 | 诊断Event/receipt输入仍Money分/qty股；`aggregate.rs:202–254`分别检查maker/taker与真实market volume | 领域语义方向保持；本轮不是新增法源核验。 |
| :22–36 N30 RetailOrderLedger | `diagnostics.rs:792` run_one_seed→:816 record_retail_order→RetailOrderLedger（:935）→finish_run/原RetailExecutionRunReport | 真实runner caller存在；unknown/overfill/部分写入保护在:1723/1751。非权威私有投影失败面不自动升级成交易账务事务要求。 |
| :38–54 N31 Seed/StockRun | run_one_seed:800→SeedDiagnostics（:401）/StockRunDiagnostics（:394）→observe_committed_events/finish_run；Participant accumulator:824 | owner实际拥有原四map状态及参与量。step→retail决策→retail订单→events顺序见:804–819；未伪造交易或另造账户。 |
| :56–76 N32 CausalReportBuilder | `session/causal.rs:12` causal_diagnostics→`CausalReport::from_facts`（aggregate:10）→builder:15→build_report:257 | 私有owner真实服务公共facade；Sequence→Budget→fill→execution先后及显式错误见:38–73；报告缺样本/删失保留。 |
| :64–71 N32 两个microstructure owner | build_report→`microstructure::analyze:166`→DirectionPersistenceAccumulator:22 / QuoteResponseAccumulator:58→finish报告 | analyze:169逐fact先direction后quote。Execution后index+1扫quote（:81）；深度下降后从当前index扫恢复（:122）。正式计划的“观测冲击”语义保持，不称因果估计。 |
| :78–93 N39 PhaseTimingLedger | production dispatcher `pipeline/authoritative_tick.rs:24/30/49`→phase capture/PhaseTimingLedger:252→CommittedPhaseTiming:392 | P9前校验、成功提交后mark，实际caller存在。`current_runnable_threads:405`调用rayon::current_num_threads仅是配置容量；外部runner /proc采样不同字段另行报告。 |
| :95–114 N41 Collector private+receivers | plan_chain_candidates:376→decision_chain:754/763→record_plan_root（causal.rs:129）；execution/records:17→record_continuous_fill:141；auction_day_end:1937→record_auction_fill:168 | 三条生产caller闭合，decision_for/facts只读consumer存在；采集为feature诊断，不入save。 |
| :116–134 scoped格式/复核/文件清单 | 当前六个文件及相关collector/phase测试存在；本轮仅读取 | 格式检查不等于类型/行为通过。review冻结SHA只能用于相同内容；当前两个文件另有后续修订，见下节。 |
| :136–138 后续root短测21例/本簇无残余/非全回归 | `domain/final-summary.md:13–19`给build07/check08与两批证据入口 | 已有明确历史验收登记；本轮未重新运行或把21例扩张为任意长诊断验收。 |

## diagnostics-review 全章条款矩阵

| 原文位置 | 当前核对 | 状态 |
|---|---|---|
| :3–10 基线/身份/限制 | reviewer独立身份与实施文档后续追加短测不矛盾 | 历史复核记录，不是当前源字节永久批准。 |
| :12–25 方法/六文件/Session片段/不运行 | 当前对应owner/实际caller以上逐条读取 | 片段复核不冒充整个Session diff，本轮也不以历史静态审查证明类型成功。 |
| :27–51 SHA冻结/接线SHA | sha256sum核对causal.rs、aggregate.rs、phase_timing.rs、phase_timing_tests.rs与表一致；diagnostics.rs/microstructure.rs不同 | 四文件可直接绑定相同内容；两个差异查历史后核销，不能盲用旧SHA。 |
| :53–70 三门核对 | 同账户/股票/真实fill/双边量等源由正式路径产生；owner未持久化 | 抽取范围方向成立，G37不能因此核销。 |
| :72–94关键错误与运算次序 | aggregate:134–144 checked逐笔value；microstructure:169–174组合顺序；phase:405容量；collector:129–180原错误次序 | owner迁移实际完成；原失败面保护不是授权删除正式边界验证。 |
| :96–113 D-INT-01首次旧私有字段call→再次修复 | `decision_chain.rs:760–769`取time→record_plan_root→record_npc_decision_trace；`session/causal.rs:8/88`facts()/decision_for()；连续/竞价receiver见上表 | 已修当前call，不能把初次跨组privacy问题重列。 |
| :115–120静态结论/完整运行限制 | result:136、final-summary:13–19追加定向构建/短测；当前全面验收另看后续总记录 | 历史未运行已经有后续；本轮不依据旧限制抹掉后来证据，也不宣称本基线重新验收。 |

两个SHA差异的具体反证：`git show 2247f4f` 显示 diagnostics.rs仅首行“可重放”改为“基于实际成交与行为”；microstructure.rs仅把嵌套 `Execution { side }`/`if let Some` 合成 `Execution { side: Some(direction) }`，保留按股方向map、pairs/same运算。`a40b127`另修diagnostics.rs“同输入报告必须完全相同”的旧注释，明确seed不包含并发实际受理轨迹。没有据这两个变化确认遗漏的owner、改变运算公式或要求恢复自由调度逐字等价。

## experience-result 全章条款矩阵

| 原文位置/动作 | 当前 caller → owner → consumer | 状态 |
|---|---|---|
| :3–13 基线/动作/无持久字段/规则假设 | owner属于纯engine；ADR0026冻结机构阈值，无股东派钱或交易制度改变 | 过程与语义边界；不要求所有散户套机构模型。 |
| :19 N01 KDJ | `indicators.rs:218/271`两入口→KdjAccumulator:180→push_rsv/finish指标DTO→三个host指标查询 | owner真实调用；九窗/seed/空态不因抽取改变。 |
| :20/26–28 N07双map writer+机构观察/stale | `feedback/lifecycle.rs:43/46/148/169/202/221/305/361`→PositionExperienceTransition（position_transition:11）；机构 `institutional_behavior.rs:23`→observe_institution_position_dated；session.rs:1582→clear_stale_institutional_holding | 6writer及两状态转换不是空结构，实际caller存在。Retail生产仍G08；dated内核存在不等于散户链已接。 |
| :21 N28仓位context | `behavior/decision.rs:213/214/297/329/354/378/384/397/564/565`→RetailPositionDecisionContext（heuristics:56）→PositionDecision | target_for与apply_experience_confidence实际参与原风格判断，desired/executable/T+1没有揉成一个量。 |
| :22 N34价格记忆条目 | `PersonalPriceMemory::observe_price/record_public_history_read`→StockPriceMemory私有方法→集合状态 | 抽取已做；Q02正式“实际历史读取留痕”caller仍缺，不能因owner抽取核销。 |
| :23 N35 RetentionCandidates | watchlist:140 / price_memory:188→RetentionCandidates:7/26→两容器自行retain | 共享选择器确实被两集合方法调用，但Session只call watchlist.prune；S03-N2生产price memory修剪仍缺，与OOP动作局部完成并不矛盾。 |
| :24 N40 EMA | macd:144/145/161→EmaSmoother:116→fast/slow/DEA序列 | 三个owner实际call，DEA固定0.2/0.8，不产生新成交价。 |
| :30–47保护/TDD事实/filter/建议runner/限制 | 指定tests在indicators:301、retention:46、feedback lifecycle/institutional_transition_tests等存在；final-summary有后续统一结果 | 文档先承认未独立跑、后追加root通过，时点一致；推荐integration suite不自动意味着当时承诺全回归。 |
| :49–53独立复审/14例/root最终验证/无本簇残余 | 最终summary:13–19登记104个domain case与8进程/Rayon4/10s边界 | 当前源存在对应14例族；本轮不重跑或把原报告转成本基线全量验证。 |

## 候选反证与残余归属

- **D-INT-01继续存在**：被当前receiver caller反证，核销历史privacy缺口；不能重开。
- **N41已接，所以DEV trace真实订单ID齐全**：反证。`decision_chain.rs:763`只接collector，后续:769仍传空events给decision trace。这是总账G37，与私有字段迁移不是同一consumer。
- **N35共享选择器已做，所以有限个人记忆完整实现**：反证。共享纯选择方法有call，完整Session price-memory prune没有call；沿用S03-N2，不再新编号。也不能用清理stale持仓epoch替代价格记忆修剪。
- **N07六dated writer已做，所以Retail失败日期/衰减已接**：反证。`pipeline/retail_projection.rs:519`的Retail模式仍 `record_fill_with_order`，dated机构收据在:597；总账G08保持。此记录没有承诺在纯OOP批次扩大到策略功能修复。
- **runnable字段=真实OS runnable**：原文result:85–87/review:64已明确拒绝该解释；`phase_timing.rs:405`为Rayon配置容量，`scripts/simulation/escrow-performance-harness.mjs:159–199`另从Linux/proc记录真实线程状态。这是口径限制，不是缺少任何真实线程采样工具。
- **Collector连续value溢出静默导致成功完整报告**：`causal.rs:162`保持旧累计但已经追加gross事实；`aggregate.rs:141–144`重建同一订单累积时checked_add返回CausalError::Overflow。没有依据说被截断的成功完整报告已经输出；工作记录明确此为旧失败面保持，不顺手要求重写采集事务。
- **run_one_seed支持自然日公司全验收**：反证。`diagnostics.rs:801–820`仅step固定tick循环；总账:186已有“tick-only不能证明完整自然日经营/披露”的范围登记。OOP记录N31明确保持runner行为，没有新增调用civil day的承诺。
- **初期review未运行代表最终没有构建短测**：result:136及experience:53、final-summary:13–19已追加root结果；本轮仅核对记录和入口，不代跑/认证该历史证据文件所有内容。

本轮未确认独立于原总账及S03候选的新产品漏实现。三文声明的OOP owner迁移/caller集成已存在；正式功能残余G08/G37/Q02、S03-N2与长期/三宿主诊断验收仍保留。四个冻结SHA当前相同，两个后续改动已经逐diff核销；历史失效链接单列导航债，未把它升级成产品需求。
