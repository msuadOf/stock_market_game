# Sweep64：deadline 与两组 fixture 工作记录补审

日期：2026-10-03。产品代码基线 `b76ece3`；本批涉及三个 runner 及 Rust integration tests 对当前 merge HEAD 无源码差异。沿用本 agent 已读的根 AGENTS/principles；补读现行 `docs/testing.md` deadline 条款。全文连续读完以下三篇，初次输出截断的 fixtures1 已单独补读完整：

- `agents/oop-refactor-implementation/hosts/deadline/status.md`：123 行。
- `agents/oop-refactor-implementation/hosts/fixtures1/status.md`：78 行。
- `agents/oop-refactor-implementation/hosts/fixtures2/status.md`：39 行。
- 合计 240 行。另按结构化字段读取对应 status.json 的最终 action/validation 摘要；没有把 JSON 的旧局部 executed=false 或早期 prose 等同于当前仍未实施。

仅新增本审计记录，无产品/Git 写操作，无测试、编译、长验收或线上动作。下列“历史通过”均是已存在 evidence 的记载，本轮没有复跑。

## deadline：全部章节矩阵

| 原文行号/承诺 | 当前 caller/代码与判定 |
|---|---|
| 3–8：两个动作源码/自审/独立复核完成，仅脚本/短测试 | 当前 BoundedCommandRun 与 ArtifactInventory 已存在；不引入交易规则、存档或产品侧变更。历史批准不能替代下面正式 deadline 的完整 process-tree 检查。 |
| hosts-N08，12–20：owner 和 facade/caller 接线 | `scripts/run-with-deadline.mjs:27` runBoundedCommand facade、`:44` BoundedCommandRun、`:150` runWithDeadline；`run-long-validation.mjs:7`、`run-web-tests.mjs:86,143`、`run-full-regression.mjs:626` 消费相同 options/injected run。不是类存在而 caller 仍旧 free flow。 |
| 21–23：10000/300000 上限、reserve、close 优先级、只完成一次 | `run-with-deadline.mjs:6–8` constants，`:29` 最大300000，`:32` reserve验证，`:80` execution timer，`:84` hard timer，`:121–129` callback/abort/timeout/nonzero 优先，`:136–146` settleOnce→清timer/remove listener→completion。主干已实现；进程组不足见 N64-1。 |
| 24–26：pre-abort/运行 abort/重复事件/callback/双流 | `run-with-deadline.mjs:40` pre-abort 不spawn；`:76–78` 双流 data；`:93–103` 原chunk callback和callback error；`:106` abort→kill后等待 close；`:112` spawn error走settle；`:136` 完成幂等。相应 test 现存，但未在本轮跑。 |
| 27–28：无关闭复核发现；Windows taskkill fire-and-forget 保持 | `run-with-deadline.mjs:13–18` 当前仍 spawn taskkill/unref/return。这个残余在原文已明确，不误报本重构引入；但正式规则仍要求进程树结束，应作为 N64-1 的平台边界保留，而非“所有 deadline 已完备”。 |
| hosts-R2-N19，32–39：deep clone/freeze、原顺序/extra/duplicates、先验摘要 | `run-full-regression.mjs:179–213` 拥有 JSON wire clone，`:167` iterative freeze；`:196–209` 校验 decoded identity 后才构造，不重签替换外部digest；`:187–189` 原字段插入顺序摘要。`:216` 返回 frozen descriptors，不排序/去重。 |
| 40–44：build adapter、read重建、执行metadata/digest与fs隔离 | `run-full-regression.mjs:507` fromBuild；`:523` atomic write；`:263–295` read/lstat→fromDecoded→逐descriptor containment→realpath/lstat→byte/hash seal。逐条IO优先级保留。路径/文件校验不保证读取后到exec的替换窗口已消除，原文也不承诺。 |
| 45–49：输入/输出篡改、顺序/extra/重复、漂移均在测试启动前拒绝 | `run-full-regression.test.mjs:426` immutable，`:469` sealed重排extra/duplicate，`:490`深层JSON；生产 constructor/toJson独立clone；readAndValidateInventory执行前逐一校验。合法reseal接受不是发布可信manifest，不扩大该工具信任域。 |
| 50：文件替换竞态、Release manifest 不宣称解决 | 按原文保留诚实边界；不新开同一未承诺原子exec缺口。 |
| 验证，54–68：54原权限短测/34子集/8sandbox失败→54外跑通过 | 历史 evidence 层级已分开。case10000、command10000、并发4为原命令配置。本轮未执行，也未据8历史EPERM推断当前代码仍失败。 |
| 69–99：只读diff、复现命令、基线/native probe、root权限重跑 | 过程证据；无当前必须重跑全部旧命令的产品承诺。`:74`完整短测命令走 run-with-deadline facade，Node case timeout与concurrency明示。 |
| 深层 JSON 修正，104–118 | 当前 `ArtifactInventory` constructor `run-full-regression.mjs:184` 与 toJson `:213` 已 JSON.parse(JSON.stringify)，freeze `:167–175` iterative；没有 structuredClone 接受边界回归。`test:490` depth2000保留。旧红灯已源码修正，不能重开。 |
| 119：修正后55整套未重跑；原54与后4分别报告 | 仍为准确的特定旧批验证限制，不合并数字；最终代表性验证不是55整套脚本通过的证据，不转成新生产功能遗漏。 |
| 最终，121–123 | 结构化台账 final_validation 优先；build07/check08、选中短case是历史事实，未选suite/API doctest/matrix/E2E/性能明确不声称。文首“完成”只限该重构和授权代表性检查。 |

## fixtures1：六动作及最终状态

| 原文行号/动作 | 当前 fixture owner/真实测试 caller 与状态 |
|---|---|
| 3–5：仅fixture迁移，最初待root | 最后`:78`和status.json已追加代表性验证/独立复核收口；最初待执行为历史阶段，不开未实现G。 |
| 9–15，hosts-N03 | `packages/engine/tests/information_acquisition/fixture.rs:128` Scenario owned state，`:146` new，`:265` post_undisclosed_fact，`:273` publish_correction；acquisition_gold/failures/view_gold消费methods，个人 acquired state仍测试显式更新。status.json两选中unread/version pin case记为通过10000ms；当前生产信息获知深层缺口按原总账，不由fixture核销。 |
| 19–25，hosts-N04 | `tests/publications/weekend_publish.rs:40` WeekendScenario；`:74` install；`:78`显式周五tick；`:85–100` end_civil_day→seeded.ops.advance→dispatch，周六不偷偷step。既有 synthetic Q1排期不是准则变更。当前status.json本action selected_rust_cases为空，不能把最终通用build/check句子当这两case都实跑；是代表性选择边界，非fixture未接线。 |
| 29–35，hosts-N05 | `tests/save_contract/main.rs:116` SeasonedSaveFixture，`:132`每次鲜建并跑两天，`:140`baseline OnceLock后clone；没有共享可变Session、没有用restore替代fresh。status.json有cached_baseline_clones_do_not_share_tampering通过，未据此宣称所有恢复测试通过。 |
| 39–45，hosts-R2-N01 | `tests/attention_discovery/exposure.rs:92` AnnouncementExposureFixture；`:133`按as_of从PublicLibrary派生曝光，`:143`market只读借用。exposure/discovery failures真实调用；曝光不record_acquisition。status.json两选中case历史通过。 |
| 49–55，hosts-R2-N02 | `tests/auction.rs:8` AuctionFixture；`:117`quiet_on_exchange同步code和exchange；`:140–174`首证券行情/auction_orders/next_order_id/envelopes同步；`:177`restore_with_orders调用真实GameSession restore。沪深的合法测试身份及排序字段没有被“复用fixture”抹平；两选中case历史通过。合法fixture修补仅测试，不进入坏档生产校验。 |
| 59–65，hosts-R2-N04 | `tests/behavior.rs:79` BehaviorScenario共持market/observations；`:85`同paths键集构造；`:132`breadth setter，market/observations借用。原retail engine API caller持续使用场景；status.json两选中case历史通过。fixture造输入不等于权威游戏可注入虚构成交。 |
| 静态核查，69–74 | 历史rustfmt/diff/assert数保留证据，不等于运行。DTO字段未改getter不构成owner封装遗漏。 |
| 最终，76–78 | status.json的最终代表性结果覆盖最初“待root”；包含无选中case的action不能被通用结尾提升为所有目标测试通过。本轮没有复跑任何fixture。 |

## fixtures2：六动作、修订轨迹及最终状态

| 原文行号/动作 | 当前 fixture owner/真实测试 caller 与状态 |
|---|---|
| 3–5：只integration tests，无生产API/规则改变 | 源码路径全为tests，新增fixture不改正式persist/制度边界。 |
| 11，hosts-R2-N05 | `tests/civil_clock.rs:86` SpringFestivalScenario owned Session/due，`:91`新建，`:119`own_due按注册事实约束。原日结仍显式GameSession API，rollback/observer retry不是fixture自编模拟。本action selected_rust_cases为空；编译/静态review有最终记录，不凭泛化结尾声称civil_clock所有目标已运行。 |
| 12，hosts-R2-N06 | `tests/fundamental_beliefs/main.rs:113` FundamentalBeliefCase持Scenario/info/market/book/issuer，`:123`7参数new，`:142`acquire调用record_acquisition，`:151–170`临时context/inputs后apply_cause。issuer total_issued_shares不是float_shares；个人未获知材料仍拒绝。status.json两选中case历史通过。 |
| 13，hosts-R2-N08 | `tests/industry_reports/correction_restatement.rs:28` CorrectionScenario，`:36`旧年度报告JSON固定，`:87`真实closing.correct，后续close methods调用ClosingEngine。没有用改旧snapshot模拟重述成功。两选中case历史通过，不替代生产集团/更正事件caller的旧G边界。 |
| 14，hosts-R2-N10 | `tests/plans.rs:51` PlanScenario、`:56`create，`:71–97`明确先ChildOrderAccepted后ChildOrderFilled。测试事实注入本来是PlanBook单位生命周期测试，不冒称真实订单簿成交。status.json两选中case历史通过。 |
| 15，hosts-R2-N11 | `tests/publications/failures/mod.rs:25` Base独占closing/library/scope，`:36`真实snapshot+publish合法Q1，`:86`request复用输入；失败测试各自篡改同一必要字段。status.json两个选中拒绝case历史通过。 |
| 16，hosts-R2-N12 | `tests/session.rs:93`一个SaveSlot owner，`:187`消费restore，`:238–275`envelopes及FIFO cursor同步，`:277`仅一次GameSession::restore，`:278`随后核对reserved cash/shares。保存字段是DTO，不把owner内部合法fixture补齐流移到外部坏档校验。两个选中case历史通过。 |
| 验证限制，20–26 | 原190 test、assert数是历史静态证据；新case最初无红绿证据不能改写成已TDD。本轮不复制测试结果；`:39`最终代表性验证/结构化status已收口编译及选中短case。既有SaveSlot checkpoint不证明日内用户持久化，正式ADR-0025边界优先。 |
| 修订，30–33 | civil_clock注释为中文过程修正；BeliefIssuerInputs当前具名issuer；TradingPlan private字段通过同名getter；仅status篡改test继续用serde而没有添加多余PlanEvent。当前calls已迁移，未找到owner实际未用的旁路。 |
| 35：最初Rust待root | 由`:37–39`与status.json final_validation更新，保留历史，不开G。 |
| 最终，37–39 | build/check/代表性case结果不等于所有旧suite、API doctest或真实矩阵完成。完整验收债继续按主账，不重复开fixture实现缺口。 |

## 正式 deadline 与实际生产工具链

现行 `docs/testing.md:93` 普通 Node 命令与case 10000ms；`:99` 长child和整批到进程树终止/临时清理均300000ms，需要进程外supervisor；`:119` 冷build与after/sensitivity分批deadline，不把两批简单加总冒充一个K7批次。

当前 `run-web-tests.mjs:91–103` 共享普通batch起点，`:105`并行shards、`:114`case10000、`:122`剩余command budget，`:128`首失败abort siblings，`:141–153`正式入口外部internal-worker supervisor。全回归 `run-full-regression.mjs:328–351` 多Rust binaries每worker给test_threads/RAYON，`:374`长用例采用剩余shared deadline，`:626–643`外部phase supervisor。没有把Node case timeout或多核主干误报未实现。G39实际受理轨迹比较、Q05脚本测试发现范围属于不同契约，存在runner不能核销它们。

## 新候选 N64-1：POSIX 嵌套 detached 子进程逃出外部 deadline 的终止范围

原工作记录 `deadline/status.md:21` 承诺保留硬上限及cleanup，正式 `docs/testing.md:99` 明确进程外supervisor需要覆盖进程树。当前 `scripts/run-with-deadline.mjs:69–75` **每次**spawn都在POSIX设置detached=true；`:21` terminateTree仅 `process.kill(-child.pid, SIGKILL)`，只终止该child的进程组。

实际 caller 不仅启动一层：`scripts/run-web-tests.mjs:146–152` 外层以同facade启动internal worker，worker `:110–124` 又以同facade启动多个shards，因此每个shard是独立process group。`run-full-regression.mjs:636–642`启动internal phase，phase `:344–351`又用同facade启动Rust binaries；binary也离开internal phase的group。外层deadline/abort杀worker/phase group时，不包含这些已经detached的shards/binaries。

触发边界：internal worker/phase的Node事件循环阻塞时，其内部timer/abort不能收敛；外部supervisor虽然仍活跃、会按时kill直系worker，但当前process-group范围无法覆盖孙进程，测试进程可留存超deadline。这正是正式规则要求进程外supervisor的场景，不能以正常内部timer成功反证。即使孙进程平常还有自己的Node timeout，Rust binary等没有独立第三层supervisor，且总进程树收尾不能只靠孤儿自行退出。

反证：每个单层child group的非detached后代确实可被kill；内部正常调度的失败会调用siblings abort；当前hard timer按时reject，因此不是“所有超时都无效”，不是交易引擎行为问题，也不是本OOP refactor新引入。缺口在嵌套真实caller与外部process-tree保证之间。应由总审计复核平台过程树管理/统一组继承或递归终止策略，不通过放宽deadline、弱化清理断言修正。未启动真实超时fixture，本候选依据源码和POSIX进程组语义。

Windows对应残余 `deadline/status.md:28` 已明示：`run-with-deadline.mjs:13–18` taskkill fire-and-forget，未wait/check其error/exit；不能从main child close推断所有后代已被taskkill收敛。建议与上述完整树终止收口一并跟踪，避免另开重复平台G。本轮不声称Windows现场已发生失败。

## 反证与未扩大范围

ArtifactInventory depth2000回归已有源码修正与针对性case；没有重新报告。所有十二个fixture owner有直接methods/caller，不把历史“待root”段当当前源码未实现。最终结构化验证中某action没有selected_rust_cases只意味着代表性验证没有选中该目标，不伪造通过也不自动变成生产功能缺口。Windows遗留、读后文件替换竞态、未跑真实matrix/E2E/性能均保留原限制。新增一个工具deadline候选 N64-1，未修改任何runner/fixture，也未执行测试。
