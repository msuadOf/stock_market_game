# Sweep 30：Task 33 / 34 / 36 历史独立复核与当前实现核对

## 基线和全文覆盖

- 基线为 `b76ece39b3a1635adde52da07375607f19b56ecc`；沿用 Sweep 12 已核对的 merge HEAD 产品/脚本/文档同基线结论。读取位置为 `.worktree/implementation-reaudit`，不修改产品或 Git 状态。
- 已连续阅读全文：`.omo/evidence/company-information-npc-intentions/task-33-review.md` **26 行**、`task-34-review.md` **24 行**、`task-36-review.md` **57 行**，合计 **107 行**。
- 已遵循根 `AGENTS.md` 与 `docs/principles.md`；本范围未发现另一个更深的 `AGENTS.md`。
- 复核追到当前生产 state/coordinator、React、WASM serializer、共享 observation clock、auction receipt/causal projection，以及诊断聚合和负控测试源码。本轮未运行测试、编译、长验收、浏览器或线上请求，不把历史测试计数作为本轮通过结果。
- **结论：本分配范围没有确认新增独立漏实现。** 历史 nullable、逆序覆盖区间、午休 clock 和旧竞价累计值观察已有当前实现反证；`NpcDecisionTrace` 空 events 仍归既有 **G37**，不重复编号。

## Task 33：全部章节/段落

| 原文行号与章节 | 当前实现证据 | 状态 |
|---|---|---|
| `:3` 至 `:12` 日期、reviewer、初始 verdict、reviewed scope | 原文记录2026-09-12的独立审查；当前对应文件仍为 `apps/web/src/store/company-slice.ts` 与 `apps/web/src/host/company-query-coordinator.ts` | 历史过程和范围，不构成额外产品要求 |
| `:14` Confirmed：原子 baseline、stale fencing、金额字符串、五种状态 | `company-slice.ts:111` 返回全新baseline；`:5` 状态联合；`:129` 保存完整public report；`company-query-coordinator.ts:75`重置generation与请求registry；`:208`检查disposed/generation/ticket | 所述实现存在；没有把精确会计金额转成浮点数 |
| `:15` civil/disclosure不依赖成交、仅刷新受影响公司 | coordinator `:144`接受tick，`:150`独立接受civil；`:163`逐条civil/disclosure事件；`:179`仅对缓存的披露公司刷新 | 已实现；civil推进后刷新已缓存公司也是明确查询策略 |
| `:16` 共用EngineHost、无adapter专属UI/private暴露 | coordinator `:32`只取EngineHost公共报告capability/API；CompanyPanel只收公共DTO；`apps/web/src/host/serde-normalize.ts:19`公共报告入口 | 公共查询架构存在；不是对所有宿主传输边界都无缺陷的背书 |
| `:18` Finding/remediation：逆序空coverage导致游标回退 | coordinator `:188`先拒绝unsafe/negative/reversed；`:192`校验前序cursor；`:196`校验逐事件连续；`:203`校验尾cursor；`:184`通过后才推进 | 修复仍保留；`company-query-coordinator.test.ts:239`保留逆序空range负控 |
| `:21` Residual：旧web suite 219/220 / WASM Map恢复失败 | `serde-normalize.ts:64`prepareSaveForWasm，`:85`至`:94`恢复权威数值账户Map；`serde-normalize.test.ts:123`、`:182`、`:233`覆盖恢复和非法键；生产WASM Worker报告查询也经过normalizer | 历史失败不能直接当当前漏实现；现有输入/输出Map桥接已存在，本轮不宣称旧suite已重跑 |
| `:23` Residual：当时只做primitive结构校验，日期/秒数由Rust契约供给 | `public-report-normalize.ts:61`合法公历日往返校验；`:67`日内秒范围/安全整数；`:141`报告期间、批准/发布顺序 | 当前已比该历史描述更严格；不是仍缺日期语义校验 |
| `:24` Residual：rendering留给Task34 | `components/company/CompanyPanel.tsx:81`至`:103`实际状态、报告列表、四表与附注 | 后续rendering已实现，不把Task33有意分阶段当欠实现 |
| `:26` Final verdict | 历史修复后批准；本轮按当前源码逐条核对 | 保留历史结论，不新增运行证据 |

## Task 34：全部章节

| 原文行号与章节 | 当前实现证据 | 状态 |
|---|---|---|
| `:3` 日期、`:4`scope | 历史范围为React/App/mobile/start-date/production-preview E2E | 历史范围 |
| Independent Findings `:8`：真实WASM的next_cursor undefined / supersedes遗漏阻塞UI | `apps/web-wasm/src/lib.rs:33`public_dto_to_js显式 `serialize_missing_as_null(true)`；`:406`public_report_page实际调用该serializer；`:418`public_report_by_id同样接线；`packages/engine/src/company/query.rs:29`、`:36`这些Option字段未标skip_serializing_if | **历史阻塞已修复**；不能继续把REQUEST_CHANGES当现行缺口 |
| Independent Findings `:9`、`:10`：不伪造报告、完整ready/error/unavailable/empty/loading分支 | `CompanyPanel.tsx:81`至`:103`；`ReportNotes.tsx:20`、`:31`比较期说明；`company-presentation.ts:97`Available/Unavailable分支；`FinancialStatementTable.tsx:9`与`company.css:27`、`:31`局部滚动和sticky subject | 已实现；missing comparison不被当零，会计精确值保留字符串 |
| Independent Findings `:11`：起始日期2000..2099、默认2030、同setup跨宿主 | `components/start-date.ts:17`范围及公历校验，`:32`setupWithStartDate，`:36`DEFAULT_START_DATE；`StartDateInput.tsx:11`、`:22`；`App.tsx:502`、`:647`复用组件；`app/useSessionHostLifecycle.ts:204`用同setup分别创建三宿主 | 已实现；没有另立宿主专属日期状态或存档格式 |
| Verification Observed `:13`至`:20`：241tests/TS/lint/build/5E2E/截图 | 当前E2E `apps/web/e2e/company-information.spec.ts:12`等待实际公共报告ready；`:19`真实WASM四表；`:81`375px局部滚动；`:101`日期变更；`:141`故意注入非法page size形成显式错误 | 这些是历史计数/当时观察；当前源码已有真实ready路径覆盖，未在本轮执行或重新量像素 |
| Verdict `:22`至`:24`：REQUEST_CHANGES仅因上游nullable阻塞，不允许UI workaround | 真实生产WASM page/by-id serializer修复见上；`wasm-worker.ts:258`与`:314`仍使用strict normalizer；`public-report-normalize.ts:29`optional只接受null或合法值 | 采取上游真实修复，未靠UI吞错放宽DTO；历史阻塞不再单列G |

## Task 36：全部章节/段落

| 原文行号与章节 | 当前代码证据 | 状态 |
|---|---|---|
| `:1`至`:4`独立review过程 | 历史两轮read-only reviewer记录 | 历史过程，不是新功能 |
| Fixed `:6`至`:10`：auction maker/taker、双边qty/value、累计fill、restore-window错误、phase/endpoint、budget、decision、replacement/守恒 | `session/pipeline/auction_day_end.rs:1983`按maker_is_buy记录真实maker/taker，side=None；`:1931`用receipt before/after累计值；`diagnostics/causal/aggregate.rs:76`decision前向guard，`:114`fill/累计值guard，`:202`双边execution对账，`:257`总量/两倍市场量守恒；`causal/report.rs:21`RestoredObservation；`aggregate.rs:313`endpoint；`session/causal.rs:16`phase/civil time | 所述因果观测/聚合能力已有，不伪造历史起单或变更存档schema |
| Accepted `:12`至`:15`：后续midpoint仅观察、竞价无aggressor、同股票连续方向、任何depth loss/可能0分钟恢复 | `diagnostics/causal/microstructure.rs:39`每stock连续side；`:81`首个后续合法midpoint；`:97`auction_has_no_aggressor；`:121`depth loss，`:122`包含当前quote寻找50%恢复；`:143`censored reason | 已实现；observational bp不被描述成反事实或完整因果效果 |
| Remaining `:18`至`:21`：原chain clock遗漏午休 | `session/observation_clock.rs:4`共享clock，`:27`7200秒后加5400秒午休；`decision_chain.rs:74`生产capture实际调用；`session/causal.rs:28`同源 | **后续修复已实现**，由本文件Source Clock Repair Re-review明确覆盖旧qualification |
| Remaining `:22`至`:23`：旧竞价gross overflow缺AuctionCompleted，诊断只记abort | 当前 `stock_auction.rs:589`先校验gross，`auction_day_end.rs:1481`成功completion发布AuctionCompleted；当前pipeline使用Result/transaction拒绝非法计算 | 当前已不是旧散落竞价循环；失败交易应显式失败，不能依据历史注记强加“溢出也必须成功完成竞价”事件要求；旧原分支结论不能照搬 |
| Remaining `:24`至`:25`：旧auction settlement filled_value_before全零 | `session/pipeline/stock_auction.rs:584`读取envelope audit，`:597`买方/`:605`卖方都用真实before.filled_value；`:611`累加gross；`auction_day_end.rs:1943`diagnostic用receipt真实value_before/value_after | **旧零值残余已不成立**；实际结算与诊断都不是硬写Money::ZERO |
| Remaining `:26`：first-stock calendar policy | `session.rs:1389`与`:2804`构造/恢复都取setup.stocks[0]；`session/civil_clock.rs:204`明确v1沪深同轨及选所政策 | 既有公开政策；本review未批准改变日历选择，不据此新增漏实现；已登记G15官方coverage问题不受此结论影响 |
| Attribution `:28`至`:30`：decision为提交时执行观察，PlanId父标识，非完整plan-revision causality | `session/causal.rs:75`查询linked_plan_id，`:88`decision_for(account)；`CausalFactKind::Submitted`携带真实OrderOrigin；生产 `adaptive_plan_chain.rs:911`、`:984`、`:1042`实际提交接线 | 满足该文限定归因；**不能据此核销另一个NpcDecisionTrace空events的既有G37** |
| Verdict `:32`至`:33`：clock尚未修时非无条件通过 | 同文件`:35`起的后续re-review明确supersedes | 保留先后历史，不把旧非批准结论冒充当前未修复 |
| Source Clock Repair Re-review `:35`至`:48`：接受clock修复、5401秒跳跃、不改minute/RNG/schema、legacy reconstruction | 当前clock源与生产调用如上；`decision_chain.rs:3358`边界/压缩日，`:3389`午休5401秒断言；`tests/extraction_replay.rs:236`立即恢复权威字段，`:281`seed扰动，`:296`事件顺序扰动 | 修复及验证源码保留；历史5+4测试通过不作为本轮执行结果 |
| Nonblocking `:50`至`:54`：日期rollover/nonzero-opening-no-closing缺专门新assert、constants维护 | 当前共享clock能读取当前civil_date且区分opening/closing；已有两套压缩边界测试；没有文中声明这些未单列case就是已知错误行为 | 验证补强方向，不确认新漏实现；不将“少一个独立断言”等同“功能未实现” |
| Handoff `:56`至`:57`：需primary最终接受后再改任务checkbox | 历史协作门禁，不是生产能力；本轮没有改任何计划checkbox | 不增列产品G |

## DEV、负控、生产与验证工具的区分

- DEV/private隔离已有：`session.rs:11`因果模块受 `simulation-diagnostics` 控制，权威状态collector字段 `:1159`同样受feature控制；`apps/web-wasm/src/lib.rs:451`capability及`:455`导出受feature和debug_assertions双条件；正常发布WASM另有已有脚本检查诊断导出。不能从DEV功能存在推断生产快照暴露私人信息。
- CausalReport真实起单/成交/撤单并非只有手工fixture：`session/causal.rs:65`记录原始OrderId/account/stock/PlanId/当前decision；`adaptive_plan_chain.rs:1096`消费真实execution round的trade和operation quote；`auction_day_end.rs:1915`至`:1999`消费真实receipt/match。诊断只是观察者，不另立交易或收费算法。
- `tests/causal_diagnostics.rs:67`复制submission/改owner、`:192`修改budget或termination qty、`:230`复制fill/改execution price都是**负控输入**，由aggregate拒绝；不是生产自然生成了这些错误的证据。`:220`恢复后显式RestoredObservation是批准的诚实边界，不是要求造出缺失观察历史。
- 会计展示边界当前 `public-report-normalize.ts:57`精确金额语法、`:120`summary/detail/比较期/发布时序/scope一致性校验；`components/company/public-financials.test.ts:50`非法公历日、`:57`更正关系等是输入负控。它们验证共享公共DTO，不代表任务33/34曾承诺在UI再实现完整会计账簿/封账业务。
- 会计核验工具的拒绝案例、历史负控失败及Task36诊断整流不能替代工商/集团/行业生产闭环核对；现有 **G28/G35/G36** 和 **G37** 继续由对应专题负责。此处没有证据核销它们，也不重复列新项。

## 新候选与反证汇总

| 候选 | 当前反证 | 处理 |
|---|---|---|
| Task34仍因Option→undefined无法ready | public DTO显式null serializer已接page与by-id，Option没有skip_serializing_if；Worker严格parse仍保留 | 排除旧阻塞 |
| Task33空逆coverage仍可回退 | assertEventCoverage先拒绝不安全/负/逆序，完整事件覆盖校验后才推进 | 排除 |
| Task33仍不校验日期/秒数 | public-report-normalize已做公历/秒数/批准发布顺序语义校验 | 排除 |
| Task36午休时间仍错或只修了测试wrapper | observation_clock被生产DecisionChainObservation::capture和causal_time共同调用 | 排除 |
| Task36auction结算累计值仍全零 | 生产fill_receipt使用before.filled_value，诊断读真实receipt value链 | 排除 |
| restore不保存因果fact是欠实现 | 明确批准ObservationRestart/RestoredObservation，不改save schema、不补假origin | 范围内显式不支持 |
| 因果报告已有真实OrderId可核销G37 | 另一个 `decision_chain.rs:764`调用record_npc_decision_trace时`:769`仍传`&[]`，`:800`order_ids从events提取 | **既有G37继续保留**，不新增编号 |
| 历史测试失败、负控、hand-off checkbox或未覆盖单个配置即新增代码缺口 | 文中历史结果与当前源码、验证fixture与生产事实不同；没有明确当前功能缺失反例 | 不新增G，保留验收诚实限制 |

本报告只新增审计证据文件，等待总审计独立复核/归并；不宣称完成真实浏览器回归、长模拟验收或交易语义改动。
