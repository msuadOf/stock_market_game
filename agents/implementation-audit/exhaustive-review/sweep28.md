# 历史实现穷尽复核 sweep28：Task27 存档及 Task28 场景复核

## 全文范围及结论

- 产品基线 `b76ece3`；当前 merge 的 `git diff --name-only b76ece3 HEAD -- packages apps` 无输出。已承接本轮根 AGENTS/principles 读取，不修改产品或Git。
- 连续全文读取 `.omo/evidence/company-information-npc-intentions/task-27-manual-continuation.md` **25行**、`task-27-review.md` **54行**、`task-28-review.md` **89行**，共 **168行**，无跳章。
- 三份历史文件主要是存档/测试审查证据。公开查询、WASM、DEV 并非其中独立新增承诺；按父任务要求额外追当前跨层生产边界。
- 未确认总账之外的独立产品 G。旧 Task28 REJECT 多项已有后续代码修正；不能照抄为当前缺功能。部分历史端到端验收仍需证据，尤其 feature开关经济行为对照、跨年长验收、same-day真实买入后再卖以及跨股卖单未成交的完整受控场景。**未运行测试**，不把测试源码判定写成本轮通过。

## Task27 manual continuation：逐条复核

| 原文条款与行号 | 当前状态 | 当前代码与核销依据 |
| --- | --- | --- |
| 5–11：26 NPC、先2日、恢复后3日，每tick事件与日结存档字节比较 | 历史证据保留；现行短fixture已缩小，恢复链仍存在 | `packages/engine/tests/save_contract/main.rs:264` 当前连续性测试用265行每交易日1tick，270行日结，273行decode/restore，279行比较事件、287行比较日结档、293行检查下一日首tick；不声称当前仍执行历史原规模/天数 |
| 13–16：历史13测试通过 | 历史运行记录，不外推当前计数或通过 | 当前save_contract有新的quiet-point测试 `main.rs:188` 和严格负例 `failures.rs`；本轮不重新跑命令 |
| 18–21：100000 pending queue产生一个ResourceLimit并保留进度 | **已被后续容量决定核销**，不要求恢复旧门槛/类型 | ADR0019 `docs/decisions/0019-draft-market-scope-and-capacity.md:58` 删除仅证明旧固定容量拒绝的测试；现行 `session/execution/records.rs:52`、99行直接追加pending facts，全仓无 `pending_plan_event_capacity`/MAX_PENDING_PLAN_EVENTS；`session.rs:752` 的SessionError::ResourceLimit及 `persistence.rs:965` 解码字节上限不是旧订单/计划配额 |
| 23–25：formatter、生成目录清理及RuntimeResource删除 | 历史owner/cleanup证据，不成为永久禁止生成要求 | 生成TS后续属于Task29/绑定脚本；不能要求当前生成目录保持2026-09-12 HEAD字节或恢复已撤销RuntimeResource |

## Task27 independent review：逐章复核

| 章节、原文行号 | 当前状态及代码依据 |
| --- | --- |
| 标题/verdict，1–5 | 2026-09-12 APPROVE只对当时改动；不代替本轮生产与边界复核 |
| Final Closure，7–23 | 空生成diff、formatter、encoding、963测试等均为当次记录；不是当前测试数量保证，也不要求今天删除后续合法生成类型 |
| Retained Behavior Evidence，25–37 | 历史命令结果保留，源码已演进。旧extraction replay/固定容量工具已经后续正式清理，`docs/test-cleanup-checklist.md:60`、81–91行登记，不能因旧测试不存在列漏实现 |
| strict K7，39–42 | 权威字段、未知/缺字段拒绝、候选先校验仍有代码：`session/persistence.rs:258`验证schema和policy，936–939行公司/披露/个人/计划校验；963行decode先字节门禁、971行schema header、972行serde。`session.rs:2926` 后续恢复直接覆盖公司与个人权威状态，不前史重放或默认重建；当前存档/恢复continuity有短测试，但历史五日字节证据不当本轮运行结论 |
| ResourceLimit，44–45 | 后续ADR0019已撤销任意计划事件数量限额，核销；不把没有旧ResourceLimit event当新缺口 |
| A股不变，46–47 | 本轮仅工作记录，无交易代码变更；T+1、申报单位、价格时间规则仍须现行生产验证，不以历史APPROVE自动代替 |
| Residual Risks，49–54 | rust-analyzer timeout、旧clippy位置、boulder排除均为当时工具/归属背景；没有事实可把2026-09-12失败自动标成当前缺产品 |

## Task28 review：覆盖表及 R1–R5 的当前去向

| 原文条款/行号 | 最新代码事实 | 分类与反证 |
| --- | --- | --- |
| 导语7–12、覆盖表18：真实session/closed-day | `company_scenarios/lifecycle.rs:8` 使用真实GameSession，18行运行交易日，34行断言closed-day不推进tick、39行RNG不变；该测试自身不证明完整填单链 | 保留该用例的有限范围，不把非空publication数当belief/plan/fill全链证据 |
| 覆盖表19、R3 47–51：跨年仅日期/pending | **已修测试源码**：`lifecycle.rs:52` 年末session；93–99行检查annual closing versions增长，102行公开query，116–128行沿实际civil日推进至2030年报公开，123行验证休市不交易，133行检查disclosure cursor，143–144行确认报告期间和正资产 | 历史“只查日期”已过时；51行明确ignore long validation，本轮未运行，不宣称跨年长验收完成 |
| 覆盖表20、R1同消息不同先验，34–39 | **已改真实session**：`controlled.rs:8` 以合法已获知历史建立个人先验，90行restore；92行query公开年报；115–118行实际day/civil披露；144行场景在152–155行step/日结/step，经真实跨tick观察，159–171行断言二人取得同report且个人估值变化不同 | fixture允许预设过去，但当前report acquisition和belief revision确实走session；不再是旧participant.rs两次纯函数调用 |
| 覆盖表21、R1同P&L不同经历 | `controlled_experience.rs:4` 对照经save恢复的个人历史，53–77行构造 dated买卖/受挫经历，86–91行真实session运行，95行现金相同、99行retail decisions不同 | 已补生产消费接线；仍是受控历史fixture，不等于本人真实完整历史交易或机构allocation全部差分证明。G08 dated生产writer缺口不能被这测试核销 |
| 覆盖表22、R1 cheap-but-withdraw | 旧participant.rs已不存在；当前 `session/decision_chain.rs:2354` 受控session撤单测试在2383–2386行检查真实OrderCanceled、无新OrderAccepted，2399/2404行覆盖中性与负弱信号撤买 | 现行真实撤单能力和测试已存在；“便宜同时撤单”的完整受控基本面信号组合不据此自动宣称已验收。没有产品缺撤单证据，不新增G |
| 覆盖表23、R1跨股卖未成交不买 | 当前 `decision_chain.rs:485` `plan_review_resources` 在493行只用own.cash，494–496行扣实际reserved_cash； `decision_resources_tests.rs:210` 覆盖mixed books不把pending_plan_events当可用cash | 纯allocation guard与生产现金输入都有，未定位同一完整session跨股票待售委托/实际无成交/买单拒绝的精确场景；记录验收待证，不从测试缺口直接推导生产错误。G38机会分类独立保留 |
| 覆盖表24–25、R4 53–57：partial fill/restore断开 | 后续生产测试 `pipeline/continuous_tick_transaction_tests.rs:521` 保存真实部分成交计划（524行filled100），恢复相同child（533行remaining300），539–551行两实例入队真实100股卖单并step，554行真实Trade，560行filled200，562行同child，567行remaining200，574行reserve减少，590–594行比较现金/预留/持仓 | 历史“无连接”已修。该用例对账而非逐字event/full-save oracle；自由并发不应以同seed强制总序。旧手工即时execute_plan_observation已退役，不恢复旧接口。`docs/test-cleanup-checklist.md:84` 后续脚手架删除/生产路径迁移 |
| 覆盖表26、R2 41–45：T+1测试只有零股 | 旧constraints.rs不存在；真实撮合买入锁定由 `continuous_trade_acceptance_tests.rs:20`、341行helper、406行t1_locked断言覆盖；账户validator负例 `account_validation_tests.rs:514` 涉及T+1；末tick结算后解锁由 `continuous_tick_finalizer_tests.rs:376` 覆盖 | 当前T+1功能不能据旧零持仓用例报缺；仍不声称本轮找到了完全符合R2“同一session真实买入→立即卖拒绝”的独立公司scenario，作为验收待证。已有分层证据不是历史标签原地修复 |
| 覆盖表27：malformed fixture | 原constraints.rs已删；当前typed/scalar错误由save负例与query输入guard承接 | 不要求保留某个历史测试文件名。存档结构负例不同于session全链 |
| 覆盖表28：future/unbalanced atomically rejected | `lifecycle.rs:157` 使用真实save，199行篡改future个人读日期，204–206行decode后restore拒绝；212–214行非平账报告decode拒绝；215–218行原会话save不变 | 现行源码仍覆盖所述负候选；本轮未执行，不宣布绿 |
| 覆盖表29：generic恢复 | `company_scenarios/restore.rs:4` 11–15行真实save/decode/restore，16行立即重存档字节相同，23–28行续行事件字节比较，39行最终档比较；live partial场景另见上文 | 不将普通同seed twin compare扩展为自由调度整局恒同保证；当前并发受理轨迹问题仍沿总账G39处理 |
| 覆盖表30、R5 59–63：diagnostics只是空feature | **历史事实已失效**：`Cargo.toml:20`、24、28行diagnostic tests required-features；`session.rs:1156`等真实cfg域；`decision_chain.rs:753` trace writer受feature保护 | Cargo feature值 `[]` 仅指没有额外依赖，不表示feature无行为。`diagnostic_parity.rs:57` 当前检测同一已提交session查询前后save不变、trace重复稳定和有界（68–82行）；不能把这种只读证明称为两个feature构建经济行为逐字相同 |
| Evidence Integrity，65–70 | 当时真实Trade/候选拒绝有效，目录归属仅历史scope；现行已删matching/participant/constraints、新增controlled系列及生产pipeline测试 | 不重新制造旧dirty-tree归属，不把后续类型变更当Task28原改动；保持测试路径演变证据 |
| Reproduced Verification，72–85 | 963 passed、clippy/LSP、12名字parity均历史结果 | 本轮只读源码，未运行命令，完整长矩阵与feature双构建output比较仍不可从旧日志推出 |
| Cleanup Receipt，87–89 | 历史review未改产品，与本轮仅新增审计相符 | 不把不实施修复的review文件当所有P1已核销，逐条采用当前代码证据 |

## 公开查询、WASM、DEV 的当前生产边界

- 公开报告：`session.rs:2026` /2039行只读查询，2030/2043行用本session civil date；`information/queries.rs:26` page guard、32行限制合法page_size、50–60行要求cursor属目标公司且当时可见、64行按as_of筛选；85行ID查询经过公开发布时间检查。DTO由 `company/query.rs` 产出，非整份GameSession。`tests/company_query_contract.rs:20` 检查pagination与无私有字段，154行测试未公开/非法query拒绝，206行session当前civil date。没有发现新增前视或私有泄漏候选；集团/四行业生产披露遗漏仍归G28/G36。
- WASM：`apps/web-wasm/src/lib.rs:406` page入口先serde解析query，410行调统一session API，412行public DTO serialization；418行by-ID调用同session public API。生产查询不是仅测试专用helper。公共API不需要simulation-diagnostics开关，NPC私有trace单独出口。
- DEV边界：WASM `lib.rs:455` trace仅 `all(feature="simulation-diagnostics", debug_assertions)`导出，451行capability同guard；Web `wasm-worker.ts:111` 仅DEV可载diagnostics WASM，283行实际capability/函数存在检查；`dev-inspector-main.tsx:4` 限制DEV入口。Server `routes.rs:567` 禁用或release返回404，575行再验证session凭据；Tauri `lib.rs:193` 同guard显式拒绝，198行校验generation后调用actor。不将历史“空feature”恢复成当前G。
- DEV真实订单关联仍缺：`session/decision_chain.rs:764` `record_npc_decision_trace` 在769行传 `&[]`。当前只读、有界trace测试不能修复随后真实order ID/plan变化关联，这项已列 **G37**；不另编号。
- 公共保存与内部quiet-point区分：Task27“每tick可save”是engine测试/验证快照；用户宿主日终 `ProtocolSession::save` 由 `session/protocol/civil/session.rs:175` 只返回最近day_end_save。不得把内部验证可保存判成ADR0025用户存档违规，也不得以旧continuity测试否定现行日终契约。

## 候选、反证及范围

| 候选 | 最终状态 |
| --- | --- |
| 重开旧100000队列ResourceLimit | ADR0019已正式撤销，不列G |
| 同report/跨年/partial restore完全没接线 | 当前controlled/lifecycle/pipeline已修，排除旧结论；长验收与精确覆盖差异分别记录 |
| Feature `[]` 证明没有diagnostics | 真实cfg/required-features/三宿主guard反证，排除 |
| 没有diagnostics双构建权威字节对照 | 保留验收待证；当前只读测试不等价；自由调度轨迹不能用同seed代替，不新增产品G |
| 无T+1功能、不能partial restore、没有公开query/WASM | 当前生产caller与分层测试反证，排除 |
| 本人日期衰减、预算机会分类、DEV订单关联漏接 | 仍由G08/G38/G37覆盖；受控fixture/只读查询不能核销生产writer缺口 |

本文件全部结论基于静态读取，无新测试运行结果；本批不变更交易规则、权限、存档或A股单位。父任务需统一独立复核工作记录diff。
