# Sweep49：三份 ignored 草稿的全文条款与现行决定核销

## 来源与范围

源文件从主工作区 `/data1/baiyifan/workplace/stock_market_game/.omo/drafts/` 读取，产品代码从审计 worktree 读取；不假设 ignored 文件在审计树存在，也不把不同来源混作同一提交证据。已完整连续阅读三份草稿，共398行；首次合并读取输出发生截断，随后分段补读全部 k7 草稿1–167行和 escrow 草稿1–173行，包含 front matter、所有旧review block、Todos、案例矩阵、验收与批准边界。

| 主工作区源文件 | 全文行数 | 实际 SHA-256 |
|---|---:|---|
| `.omo/drafts/resolve-blockers-wayland.md` | 58 | `40406fb24b014bd532de855c3801b7d6f02e702f8c785fc19dd57185dd1f8824` |
| `.omo/drafts/k7-deterministic-multicore-utilization.md` | 167 | `3c3c76946bcd95699fd9dfafb37acbecc3b48456604a64409ca35192bae52045` |
| `.omo/drafts/escrow-parallel-engine.md` | 173 | `04cf8bb40a8e90a12bc96a5fdf5bcfd3b63cb0136a778402fc3dd3b9bacdc4ac` |

沿用已读根AGENTS/principles和本轮审计指令。产品指定基线b76ece3，读取时HEAD4ad5a2e；只读diff核对receipt_key/retail_projection/routes/baseline-run/ADR0017无差异。本轮未执行测试、编译、进程管理、安装或Git写操作；仅新增本报告。draft不是独立硬需求，取代顺序以正式ADR、最新用户决定及明确退役清单为准。

## resolve-blockers-wayland（58行）

| 原文位置与条款 | 核销/当前证据 |
|---|---|
| 1–9行：awaiting-approval、pending-action只写plan | **历史批准边界。** 当前`.omo/plans/resolve-blockers-wayland.md`已存在；不把ignored草稿旧状态解释成今天必须停工或重新询问批准。也不因计划存在就宣称所有执行已完成。 |
| 11–19行：C1 dirty-tree/receipt债、C2 Weston视觉IPC、C3三宿主parity、C4 fresh K7、C5 release/final gates | **实施主题由后续plan/审计接管。** 清洁树/旧review债和历史测试结果是特定当时状态，不能直接报当前产品漏实现；Weston/真实三宿主QA属于证据与宿主专题。当前engine真实step与typed error见`failure.rs:29,51`，实际WASM错误接口`apps/web-wasm/src/lib.rs:234,344`；不以存在Rust接口代替Wayland截图或三宿主运行证明。 |
| 21–28行：1200×800/nonzero mode/PNG、WebDriver或截图+IPC、atomic checkpoint/resume、source pin、8MiB限制 | **部分已有工具/部分由正式决策替代。** README163–177行保留Weston部署说明；本次未运行compositor或检查像素，不宣称native QA通过。`baseline-run.mjs:61,323,941`有checkpoint-v4/--resume/完整身份与digest校验；生成前旧“不能resume”状态已不适用。server请求额度现为engine512MiB+1MiB（`routes.rs:41`、`server/lib.rs:109`、`persistence.rs:944`），由ADR0019:29明确批准，不是静默提高旧8MiB门槛。 |
| 30–37行：旧任务未完成、空截图、native依赖已具备、单seed/串行/无resume、pnpm/clippy、124脏文件、39体积 | **历史诊断不自动升级当前事实。** runner已支持resume/并行seed/共享deadline（`baseline-run.mjs:120,129,133,941`），当前资源policy-v7（65行）；旧单seed和8MiB风险按新工具/ADR核销。截图、各旧task审查回执及旧dirty统计是历史证据限制，由对应证据sweep核查，不能把旧计数填成当前status。 |
| 39–45行：不凭checkbox重写、Wayland主/Xvfb备、独立run/port/runtime、不能用旧WASM | **持续验收原则/未在本次运行证明。** `check-web-release-wasm.mjs`与release-policy正式工具存在；正式release规则由ADR0027/0028承接。真实host通信仍有总账G01–G05，不因草稿声称native先决条件可用就核销。 |
| 47–58行：Scope IN/OUT、无伪证/减seed/静默限额/覆盖脏树、旧批准只建plan | **范围约束仍需尊重，执行路线取后续决定。** 没有新交易制度需求；K7后来五分钟与代表性fixture政策取代草稿旧重负载规模限制；不因草稿要求调通block就虚构官方资料、测试或屏幕证据。 |

## k7-deterministic-multicore-utilization（167行）

| 原文位置与条款 | 核销/当前证据 |
|---|---|
| 1–18行：明确SUPERSEDED、serial coordinator、预分配逻辑FIFO而非到达序、只规划 | **整体旧路线已取代。** ADR0017:7–18修订全局来源序及跨worker字节等值；30行单一并行实现，不建独立serial reference；同股入口当前按实际接受先后，不能恢复旧预分配Envelope全局顺序。 |
| 20–36行：9个ownership/seam、代码行号、纯engine/账户/T+1/计划/行情/存档 | **职责原则承接，具体channel/串行协调路线退役。** `orderbook.rs:377`价格时间撮合，`account.rs:493`settlement；`account_settlement.rs:162`receipt消费，`candidate_commit.rs:97`P9最终swap。计划续行按真实typed outcome，不重新引入旧Session/Account/Plan专属FIFO串行writer。旧行号重解析，不能把消失的旧模块当缺失功能。 |
| 38–43行：79%/20%历史profile、14MB旧bytes、1/16worker、10%性能门槛等 | **历史测量/草稿提案。** 未验证其可复现；>=10%/<=5%/<=10%阈值是明确proposed acceptance，不提升为现行已批准必须达标值；CPU高使用率不等于加速的原则继续有效。 |
| 45–53行：LogicalRequestKey/stage ranks/NPC先player/complete roster seal/local substage/global queue-head/VecDeque与256chunk | **旧算法已取代。** 当前ADR0017:34准许P3/P4增量交错；39–40行资金与股票冲突处的实际接受顺序；`account_validation.rs:391`account-local arrival，`continuous_matching.rs:480`实际股票入口落簿。旧256chunk、所有producer全join与global smallest head不作现行漏项。 |
| 55–67行：每root局部提交、前笔卖款/撤单本tick可用、失败保留前缀、原allocator、顺序publication | **交易可见性与失败域被正式Escrow决定取代。** P1不可变预算（ADR0017:37）；同批释放下一tick可用（126行）；tick私有candidate失败整体丢弃（39行、`failure.rs:77`），P9成功才swap（`candidate_commit.rs:97`）。旧“失败仍保留本轮交易前缀”不能复活。 |
| 69–71行：midauction/pending-input存档、旧schema/bytes不变/临时队列 | **内部恢复能力部分保留、持久策略改变。** 当前v2含完整strategy/live envelope/receipt（`persistence/v2.rs:14,122,228`），旧档明确拒绝（97/109行）；对外持久档按ADR0025:12成功自然日日结生成。不能用旧任意call间save草稿要求绕过现行日终产品策略，也不能把内部live恢复测试删成不存在。 |
| 73–84行：serial oracle→FIFO→parallel kernels、K7 sealing、排除account partitions等 | **实施顺序取代，语义约束保留。** serial独立引擎、byte/schema保持、完全不用channel均不再现行；ADR0017:30准许tick内临时完成通知，仍禁常驻actor权威与跨实体业务锁。未授权GPU/STP。 |
| 86–114行：Todo1–7全部阶段、entity_fifo测试/模块、reservation O(orders)、quote kernel10%、旧parity/host/K7 | **整套Todo由Escrow实施取代。** 不要求创建`entity_fifo`或串行oracle、255/256/257专属cases。当前真实并行账户准备`settlement.rs:105`、股票推进与增量预算已有；哪些优化仍待测以当前ADR0018和G39为准，不拿旧任务未勾判缺失。 |
| 116–128行：七组TDD矩阵（seal/shared账户/rollback/价时auction/plans/reservation/lifecycle） | **按现行语义承接风险点，旧预期失效。** 当前同tick取消新受理余量已允许（ADR0017:128、`continuous_matching.rs:1330,1355`）；旧“卖款立即资助后单”、原partial-error-prefix、旧schema字节相等、任意midauction对外持久化不作为当前断言。价时/T+1/费用/跨tick与失败隔离仍需真实覆盖。 |
| 130–136行：old/serial/parallel三build全budgetbytes比较、paired workloads和RSS/profiling门槛 | **旧门禁被并发接受语义替代。** ADR0017:15–18明确无需不同worker整档hash相等。当前runner及matrix仍有旧整档等值门禁，已由总账G39确认，见下方；本轮不重复编号。 |
| 138–154行：v3单seedprocess、30/400天、17+77=94、全seed/末seed31复跑、fresh roots/resume | **矩阵身份思想承接，旧policy/规模被后续正式约束取代。** 当前seed/cross-year/multipliers常量仍在`baseline-run.mjs:19,20`，canonical检查994/1278行；v7有多child/Rayon总预算120–134行、primary5/cross-year8自然日21–22行、300000共享/299000执行与1000清理24–26/133行。不得按已退役草稿恢复分钟级普通测试或放宽deadline。当前批准矩阵执行结果由K7证据sweep检查，本轮不启动94次。 |
| 156–167行：F1–F4、三exit、需另批准、数值门槛提案 | **历史workflow/证据限制。** 本轮只审计，无执行授权升级；独立审查仍按AGENTS，provenance/守恒/真实局部价时继续有效，serial oracle和自由调度跨budget字节一致退役。 |

## escrow-parallel-engine（173行）

| 原文位置与条款 | 核销/当前证据 |
|---|---|
| 1–16行：plan-approved-r24、用户#9、R†整体废除、zero-cash旧新相等声明被修订、handoff-only | **最高头部状态优先于下文旧block。** #9由ADR0017:133正式承接：每腿min(应计欠费,本腿成交额)，现金预留0，只旧预留=0适用旧新相等。`transition.rs:202,219`cap与佣金→印花税→过户费，`account_validation.rs:758`卖现金0。旧R†approval/stop-resume机制整体核销；historical handoff-only不等于今天引擎未实现。 |
| 17–20行：r21两个blocker，retail按(account/stock/side/order_id)一次聚合，receipt显式rank | **当前实现承接。** `retail_projection.rs:346,375,394,409`按RetailOrderIdentity聚合qty/gross/charged，每order一次apply；`receipt_key.rs:14,49,66`显式PreSeal/SealedBatch与源rank。生产caller `account_settlement.rs:196`调用retail projection。后续不同委托收据不得用键排序当交易优先（ADR0017:11–14/113）；身份rank存在不意味着恢复旧全局序。 |
| 21–105行：r22/r21/r20/r19/r18多个in_flight/null review记录 | **明确历史状态，非活跃未完成任务。** 顶部16行已声明r24覆盖；不重复启动旧review、不能将null判为产品遗漏，也不从旧Session ID猜执行结果。 |
| 106–112行：r15 R†批准已失效、r14 inventory/保留测试/公开占用、单一parallel无serial声明 | **已明确核销/正式承接。** #9实际public占用按卖0/买含费；旧preserved inventory/semantic corpus按`test-cleanup-checklist.md:85–88`获批准退役；顶部单一实现优先于正文C2与Prometheus旧双实现建议。 |
| 115–128行：C1 ADR、C2 serial-reference、C3规则、C4 poison/v2/3hosts/TS、C5Rayon、C6 evidence/K7 | **现行核心已有；serial标签失效，验收证据不得冒充完成。** ADR0017存在；`failure.rs:29,51,89`typed step/poison/save healthy；v2 schema/projection已接线，native/WASM同Rayon依赖`apps/web-wasm/Cargo.toml:26`。host/工具未闭环见现有G01–G05/G39，不用旧C2标签要求另造serial。 |
| 130–140行：P4拒ID、只能撤前tick、无STP、wasm-rayon、SettlementError→IntentRejected | **除同tick撤单旧默认外由最新ADR承接。** P3拒不耗ID、P4拒耗ID（ADR0017:127，`account_validation_driver.rs:101,108`）；只有prior-tick可撤被ADR0017:128明确修订，真实`continuous_matching.rs:1330`读当前簿。旧默认不能覆盖用户新交易规则。普通下单前业务拒绝产生IntentRejected，内部结算不变量仍StepFatal，不能把“impossible-by-construction”解读为系统错误都变业务拒绝。 |
| 142–149行：旧源码v1/step Vec/reservation/账户/compute/依赖/规则资料与Metis claims | **历史grounding需重定位。** 当前`failure.rs:29`返回Result，`persistence/v2.rs:5,6`v2/policy-v2；旧行号不算缺功能。2026规则依据只按当前trading-rules记载适用日期，本次没有联网复核。 |
| 151–157行：释放下一tick、breaking save、Escrow替代coordinator、Prometheus serial-first/默认 | **用户决定承接，旧建议退役。** ADR0017:30单一parallel；37/126资源截点；131旧档拒绝。Prometheus serial-first与draft112行、正式ADR相反，不升级成第二实现硬需求。 |
| 159–173行：Scope IN/OUT/no channels/no STP/no GPU/no parity promise/plan-only approval | **按后续决定细化。** no channels被ADR0017:30修订为允许临时完成通知、禁止常驻actor权威；no迁移/STP/GPU/假性能保证仍保留。最终产物/运行证据需由对应专题验收；旧plan-only边界不阻止本轮已授权只读审计。 |

## 候选反证与现行遗漏归并

1. **缺serial oracle/entity_fifo模块候选：否决。** 旧草稿已superseded，最新ADR单一parallel，不恢复双实现。
2. **当tick刚受理挂单可撤违反Escrow：否决。** prior-tick-only旧默认已明确修订，当前簿决定撤余量，资源仍下一tick可复用；不是违规。
3. **server静默扩8MiB候选：否决。** ADR0019:29明确批准512MiB+1MiB，生产routes41/lib109一致。
4. **K7缺resume/multicore/deadline候选：否决。** current-run checkpoint-v4、v7预算、共享五分钟门禁都有实际caller。不得复活旧v3单进程不变要求。
5. **r18–r22 in_flight/null或旧review门禁遗漏：否决。** draft头部r24明确覆盖历史；preserved/corpus工具获批退役。
6. **跨worker完整artifact相等误当业务确定性：现行G39，不新增编号。** `reaudit-tools.md:36`已具体定位matrix583–594与contracts135–160；`baseline-run.mjs:1286`canonical raw rerun仍作相等检查。旧deterministic draft的确提供历史来源，但现行ADR0017:15–18明确取代该语义，不能以草稿核销G39。
7. **Wayland截图/host parity/最终K7未验收：证据专题，不能从草稿单独确认新代码漏项。** 本次没有运行native compositor、host matrix或K7；现有G01–G05与G39仍保留，截图和历史seal完整性由对应证据报告归并，避免把旧awaiting-approval与今日缺实现混为一谈。

本轮新增确认生产遗漏0项；保留G39现行工具错用自由调度字节相等门禁，其他候选按最新决定核销或限定为历史证据范围。草稿全文指纹已记录，未修改任何原始ignored来源或产品代码；主审需独立复核本报告diff。
