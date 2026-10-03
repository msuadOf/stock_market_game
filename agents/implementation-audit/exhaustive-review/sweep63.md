# Sweep 63：Hosts caller、actors 与协调收口记录

## 全文与基线

- 基线：`b76ece39b3a1635adde52da07375607f19b56ecc`，读取 merge worktree 的同基线产品源码；仅新增本审计工作文件。
- 全文连续读取三篇：`agents/oop-refactor-implementation/hosts/account-callers.md` **44 行**、`agents/oop-refactor-implementation/hosts/actors/status.md` **66 行**、`agents/oop-refactor-implementation/hosts/coordination.md` **36 行**，共 **146 行**。分派中的 `actors/status.md` 按“同 hosts”解析为实际存在的 `hosts/actors/status.md`，不是不存在的顶层 `actors/status.md`。
- 沿用已读 `AGENTS.md`、`docs/principles.md`；以正式 ADR-0010、ADR-0017 及现行领域规则为语义依据，工作记录“等价保留旧行为”不自动豁免正式需求。
- 本轮只读源码和已有 JSON 验证记录，未运行 Rust/Node 测试、编译、doctest、E2E、性能矩阵或 Git 写操作。下述历史通过只属于记录对应批次。

## account-callers：逐章条款矩阵

| 原文行号 / 条款 | 当前源码证据 | 状态 |
|---|---|---|
| 范围与结果 `:3`、`:5`：Position字面量与Account/Position getter、溢出fixture | `packages/engine/tests/account.rs:82`等用 `Position::from_restored_parts`；`:253`真实grant_position；`packages/engine/src/account.rs:124`cash getter、`:132`positions getter、`:609`私有Position字段、`:622`完整四值构造、`:636`各getter | caller迁移存在；测试重建既有非法边界不等于产品新增非法状态写口 |
| `:6`：StoredStrategy getter | `packages/engine/tests/strategy_state.rs:66`调用strategy；`account.rs:128`直接借用StoredStrategy | 已实现 |
| `:7`：公司开局资金隔离按户/全档断言 | `tests/company_opening/isolation.rs:67`读取cash getter，原场景仍检查投资者资金与存档 | 已迁移；未放宽公司/投资者资金隔离 |
| `:8`：same-account收益对照 | `tests/company_scenarios/controlled_experience.rs:95`、`:96`读取cash getter | 已实现 |
| `:10`：StrategyData/SelfView/PositionView与公开PositionSnap无需迁移；tests/account_book不存在 | `tests/strategy.rs:81`、`:127`、`:131`仍为公开观察view字面量；Account/Position权威私有类型与DTO不同；真正account_book是 `src/session/account_book.rs` | 不是遗漏caller；工作记录已将不存在路径明确限定tests |
| `:12`：只等价访问/构造，分/股/成本/费用/T+1/原子性不变 | Account/Position getter直接返原字段；`from_restored_parts`完整返回qty/t1_locked/invested/recovered；`tests/account.rs:260`仍检查溢出不扣cash | 未确认因caller迁移导致新语义缺口；不把getter存在视为所有财务生产链都已完成 |
| 验证及交接 `:14`至`:30`：只格式/文本检查，root短filter、可能超时要说明 | 已有 `hosts/caller-migrations.json`记录该四文件复核及check08，R256–R260选定结果；`hosts/review-account.md:15`记录完整断言等价检查 | 初始“worker未运行”是过程记录，后续有代表性验证；未选场景/全suite仍不冒称通过，不列代码G |
| Market后续迁移 `:32`、`:34`：故意非法last_close不能从正常public路径复原 | `packages/engine/src/market.rs:274`fixture_set_last_close仅cfg(test)；`:473`内部模块容纳非法状态case | 已实施；没有为fixture扩大production API |
| `:36`及`:38`至`:44`：三个完整case迁入engine unit，移除旧integration重复 | `market.rs:503`symbolic、`:546`overflow、`:553`cage三case；`tests/market/price_limits.rs`不再定义同名三个函数；caller-migrations记录R030–R032各一项 | 迁移闭环存在，不能把原路径删除误报成删除测试；本轮未再运行 |

## actors/status：逐章条款矩阵

| 原文行号 / 章节 | 当前代码和真实caller | 状态 |
|---|---|---|
| `:3`：四动作源码迁移、root统一验证 | 后文`:64`起明确final_validation替代初始待验证状态 | 历史先后须合读，不将初始段落当当前未实现 |
| hosts-01-A01 `:5`至`:13`：Desktop SessionHandles私有sender、公有command、保留方法 | `apps/desktop/src-tauri/src/actor.rs:328`compile_fail，`:334`私有cmd_tx；`:240`pub SessionCommand；`apps/desktop/src-tauri/src/lib.rs:292`等通过handles方法；actor外无正常raw cmd_tx caller | 封装已实现；工作记录`:11`保留fire-and-forget与正式ADR确认契约有待复核，见S63-01 |
| hosts-01-A02 `:15`至`:23`：Server私有cmd/event、subscribe_events、速度确认/订阅 | `apps/server/src/actor.rs:595`两个compile_fail，`:607`两个私有sender，`:624`subscribe_events；`:815`校验/oneshot；`routes.rs:1271`初订阅、`:1416`Resync换订阅；`tests/actor.rs:306`等与`protocol_updates.rs:14`改方法 | 实施/caller闭环；内部sender仍归actor，不因它仍存在误报封装未做 |
| hosts-N01 `:25`、`:29`至`:31`：按值私有Pacing，状态归属/方法、manager/run/command/restore/fatal全部迁移 | Desktop `actor.rs:142`、Server `actor.rs:308`持有running/fastest/requested/interval/base/SpeedMeter；Desktop`:813`guard、`:827`fresh interval、`:887`metrics、`:921`civil pause、`:1099`running、`:1110`speed、`:1155`restore；Server对应Pacing方法和actor caller保留 | 已实现，未引入跨宿主基类/共享锁/新engine owner |
| `:30`：重复running不reset、日界无条件reset、fatal只停止、restore reset | Desktop Pacing `:183`、`:190`、`:196`、`:200`；Server `:345`、`:352`、`:358`、`:362` | 源码保持明确差异，不用统一helper抹平采样语义 |
| `:32`：Desktop 1µs / Server 1ms、14ms预算、semaphore/事务/generation、pending fixed保持 | Desktop `:1241`fixed_tick_interval、`:43`14ms；Server`:35`fixed_tick_duration、`:31`14ms；Desktop`:682`pending fixed与`:837`run_cycle(1) | 重构保留事实；**不能核销ADR0010固定倍率聚合既有G19**；出现字段/常量不等于生产16ms聚合caller已接 |
| `:33`至`:34`：Pacing新增case、未跑需root | Desktop `actor.rs:1424`起速度模式/采样用例；Server interval tests；actors/status.json final_validation只列实际选中R229–R232等 | 实现和测试源码均在；不把“写了测试”当红绿过程已验证，不要求本轮执行 |
| hosts-R2-N13 `:36`至`:45`：mock App/actor/cmd/按需receiver、fatal/protocol caller | Desktop `actor.rs:690`cfg(test)ActorHarness，`:701`集中构造，`:739`、`:748`按需订阅；`actor/fatal_tests.rs:43`、`actor/protocol_tests.rs:8`真实测试caller；fatal`:83`检查原cmd channel关闭 | 测试owner/caller已实施，没有生产caller承诺；receiver初值None，不伪造空channel |
| 已执行有限检查 `:47`至`:51` | raw sender检索只见actor内部及doctest故意访问；原记录rustfmt/diffcheck不等于类型/行为 | 原文诚实边界，未形成新增代码漏实现 |
| 代表性短filters `:53`至`:62` | 对应测试存在；新增API compile_fail/no_run doctest在actors/status.json validation_limits明确未单独执行 | 验证债，不是sender私有化/方法未实现；all-targets不冒充doctest运行 |
| Root最终代表性验证 `:64`至`:66` | hosts/final-validation.json记录build07/check08、232lib、41追加及WS2核销；actors/status.json记录被选case及未覆盖限制 | 原初“待root”已由后续证据部分收口；未执行的原套件、doctest/E2E/perf不写成通过 |

## coordination：逐章条款矩阵

| 原文行号 / 条款 | 当前证据 | 状态 |
|---|---|---|
| `:3`至`:14`：34动作分工、必要caller、统一Rust/只Node短测 | hosts/status.json是34条动作，涵盖actor/fixture/build/deadline/performance；该文明确不是产品新路线 | 分工/验证政策，不要求所有fixture owner接入生产 |
| 协调者改动 `:16`至`:18`：六处broadcast订阅 | `server/tests/actor.rs:306`、`:329`、`:365`、`:515`、`:610`和`protocol_updates.rs:14`均subscribe_events | 六caller存在，未发现残留外部event_tx |
| 复核 `:20`至`:22`：独立三门/有效发现修复复审 | `hosts/review-actors.md`、`review-account.md`、`review-production-entry-caller.md`有独立范围与结论；status.json/manifest绑定源码 | 存在历史独立复核记录；本轮不把其等价重构批准扩大为所有正式产品需求已实现 |
| 收口 `:24`、`:26`：34动作/198检查/owner-method-caller、可选薄wrapper允许不做 | JSON台账记录真实文件/owner；可选wrapper省略是范围决定，没有发现因此丢失必需caller | 不按是否有同名class/wrapper判断产品缺功能；本轮没有重新审完198条历史diff |
| `:28`：ArtifactInventory深JSON、PerformanceComparisonRun校验/防别名、async facade、WS flush、civil注释；negative witness误报纠正、Writer短fixture | `scripts/run-full-regression.mjs:179`用JSON复制并freeze；`escrow-performance-harness.mjs:418`async facade、`:486`config验证、`:500`sample验证、`:507`append clone、`:533`report clone；`run-escrow-verification-matrix.mjs:447`/`:479`与test`:361`重算receipt后的伪造witness拒绝；Writer runtime`:587`、`:605`拥有发布/文件IO | 已有实现及测试接线；不恢复已反证的witness漏检疑点；不以本条重开已登记G39跨worker比较契约 |
| `:30`：四account caller/三个Market非法内部case复核闭环 | 上述caller/market矩阵与caller-migrations.json、Domain orderbook review | 已落地，不重复登记漏迁移 |
| `:32`：不跑全回归/E2E/perf、Writer/桌面CLI真实短结果与首失败历史 | final-validation.json给出各记录路径，保留WS初失败/环境核销与未选suite限制 | 证据边界；没有声称完整回归或性能通过 |
| `:34`：production_entry_performance三处TradingPlan getter | `packages/engine/examples/production_entry_performance.rs:197`、`:198`用account()/active_child_order_id()/filled_qty()；`hosts/review-production-entry-caller.md:10`至`:22`限定该迁移、未执行性能 | 已实施；active child OR real fill条件不把受理当成交，不能据caller通过声称性能验收通过 |
| `:36`：最终build07/check08/232+41与WS核销、check09仍待root | final-validation.json保留当时check09待结果；另有 `validation/rust-default-targets-check-09-result.json` 记录默认feature cargo check exit_code=0、39.535秒、仅类型检查 | 后续check09已有记录，不把旧“正在继续”列当前实现欠账，也不把cargo check当测试执行 |

## 新候选 S63-01：Desktop 副作用命令确认语义需裁定

- 正式来源：`docs/decisions/0010-unified-host-protocol-and-local-refresh.md:13`统一命令确认语义，`:60`「有副作用的命令必须等待宿主确认，CommandQueued 不等于订单接受或成交」。
- 工作记录：`hosts/actors/status.md:11`明确Desktop set_speed仍只提交mpsc、不等回执；`:32`要求重构保留句柄/回执差异。它记录重构不改变原行为，没有在正式ADR增加确认例外。
- 当前生产链：`apps/web/src/host/tauri-host.ts:180`invoke set_speed，`:183`await set_pause_preferences；`apps/desktop/src-tauri/src/lib.rs:279`、`:295`、`:305`、`:317`分别返回handles方法结果；`actor.rs:558`、`:564`、`:576`仅mpsc::send，`:1095`至`:1102`actor之后处理，没有oneshot应用回执。
- 因而能确认的代码事实是：这些Desktop Tauri command返回成功只证明已送入actor channel，不证明actor已经应用speed/running/pause preferences。Server同类句柄`actor.rs:815`、`:838`明确等待reply，因此跨宿主返回点不同。
- 用户层例子：`app/useSessionHostLifecycle.ts:137`await暂停偏好后继续start，Tauri adapter又在resume invoke成功后将running置true。FIFO可确保随后同sender命令的入队顺序，但不能将每次invoke完成解释为actor已应用；actor在处理前失败/关闭的边界也不能从前一次成功send知道。
- 反证/限制：工作记录明确这是旧行为保留，不是本次封装引入的回归；`EngineHost.setSpeed`当前为void，`start/stop`也是异步发起接口；mpsc保持排队顺序。**如果正式“宿主确认”只要求确认入队，这些方法可能已满足该解释**；没有找到正式ADR对该词作此例外定义。本轮没有构造运行失败或证明用户已经遇到错误。
- 建议：作为**契约解释/实现候选**交总审计独立复核，优先考虑Q；若确认必须等待actor应用，再列独立G并覆盖SetSpeed/SetRunning/SetPausePreferences回执与失败边界。不得仅用工作记录“保持旧行为”核销正式要求，也不擅自改变公共EngineHost接口。本报告不直接把它定为已确认新增G。
- 去重：该问题不同于既有G04的旧baseline重交付和G19的固定倍率聚合；目前只确认返回点事实，不扩大到所有命令或重开已等待reply的restore/查询。

## 候选反证与收口

| 疑点 | 处理 |
|---|---|
| sender仍在代码中所以未封装 | 私有owner内部必须持sender；外部正常caller均方法访问，仅compile_fail故意触碰。排除 |
| Market旧测试移除意味着边界丢失 | 三完整同名case在cfg(test)内部各一次，fixture setter也仅测试。排除 |
| 最早actors状态“待root”意味着未完成实现 | 后续同文final_validation及JSON明确实施/复核/代表性运行；未跑doctest诚实保留。排除代码G |
| check09文档仍待结果 | 独立check09 result已exit0，旧工作记录没有追写不是产品代码漏实现。排除 |
| Pacing对象/16ms常量可核销G19 | 固定路径仍run_cycle(1)；重构只保持原行为。G19继续保留 |
| 收口说negative-control误报就是工具缺guard | 当前typed rollback/control witness检查与重算receipt负控存在；没有新的生产缺口证据。排除旧误报 |
| fixture owner无production caller是未实现 | ActorHarness与诸fixture本就仅测试，记录没有承诺生产接线。排除 |

本分配范围确认封装和caller迁移已实施，没有新增已确认G；S63-01保留待裁定的副作用确认候选。G04/G19/G39等既有正式需求缺口不因重构收口被自动核销。本轮无产品改动、Git写或长测试。
