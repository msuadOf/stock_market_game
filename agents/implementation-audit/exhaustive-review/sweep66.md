# Sweep 66：hosts build / deadline / fixtures1 独立复核记录续查

## 全文范围

- 已从头连续读取至 EOF：`agents/oop-refactor-implementation/hosts/review-build.md` **71 行**；`review-deadline.md` **131 行**；`review-fixtures1.md` **134 行**，合计 **336 行**。第一次合并输出受工具截断，随后单独补读 deadline 全文与 fixtures1 开头，build及fixtures1末尾均在前次完整可见输出内。
- 基线按父任务为 `b76ece3`（合并HEAD产品相同）；沿用本线程已读取的AGENTS/principles及现行testing/交易规则。只读静态追查，未改产品、测试、Git，未运行测试、构建或长验收；仅新增本文件。
- 审核历史“待root”时同时查看各组status末尾及机器记录；后续代表性执行不冒充完整suite，不把旧误报重新列为缺口。

## review-build 全章条款

| 原文条款 | 当前实现 / 后续证据 / 状态 |
|---|---|
| L1–19 结论、九文件范围、历史46短测/3 sandbox及Rust未执行 | 此为当时reviewer范围，不是当前游戏验证。当前真实build入口 `scripts/build-targets.mjs:338` / `:351` 创建BuildRun，prepared路径borrowed；`:239` / `:255` finally cleanup。harness/runtime及matrix真实入口存在。 |
| B1 L21–33 negative-control typed witness误报撤销 | `run-escrow-verification-matrix.mjs:726` reuse调用validateExecutorEvidence，`:480`同时核验negativeControlDetected与disabled_merge，不符在`:481`抛NEGATIVE_CONTROL_NOT_DETECTED。测试`:361`重算capture receipts后仍拒绝伪witness、`:381`禁止启动child、`:385`错误码断言保留。已反证，不新报、不恢复重复guard。 |
| B2 L35–45 Writer落盘/既有目录/部分失败补测 | `runtime.rs:587` Writer为唯一写点，`:636` write_bundle调用create/write。`:927`真实落盘bytes/receipt/目录拒绝，`:964`event-stream目录阻断、已写保留/后续未写验证均在；不是fakeHarness自己写文件代替Rust Writer。历史要求已实现测试源码；root后续记录Writer5/5，见hosts最终代表性验证。 |
| 三门 L47–51 A股/必要范围/边界 | 费用、T+1、价时、存档领域无改动；Writer保持capture→receipt→各artifact顺序，`runtime.rs:605` write遍历集合。BuildRun目录owner、prepared借用边界保留，不把OOP对象本身当产品新能力。负控fresh在matrix`:661`，reuse在`:726`都经过validator。 |
| 后续 L53–55 Rust/三个CLI待root | `hosts/deadline/status.md` / `hosts/fixtures1/status.md`末尾“Root最终代表性验证”登记build07/all-targets check08、Writer5/5、桌面CLI5/5，以及两个WS sandbox失败后原断言2/2核销。此为后续历史证据，不是本轮运行；未覆盖旧测试或真实matrix/E2E/性能依然不能声称执行。 |
| 最终版本绑定 L57–71 SHA256九文件 | hash绑定该次独立review快照；本轮不拿旧hash推导当前新增diff已review，也未重跑完整baseline diff。当前所追需求caller存在；历史build子目录已不在hosts树下，不因工作记录移动/删除反推产品build代码缺失。 |

## review-deadline 全章条款

| 原文条款 | 当前实现 / 后续证据 / 状态 |
|---|---|
| L1–23 范围、N08通过、R2-N19初期P2/修后通过 | 四个deadline/regression文件当前存在；修复前54/54与修后4/4不能合并为当前55/55。本轮未执行。 |
| A股 L25–35 工具生命周期，无交易语义改动 | `run-full-regression.mjs`仅编译库存/执行工具；不对账户/撮合/费率做变更。当前交易规则官方来源重新查询非本条承诺，旧来源日期不冒充本次确认。 |
| 必要性 L37–44 BoundedCommandRun、ArtifactInventory拥有资源和不变量 | `run-with-deadline.mjs:44`对象持child/timer/abort/流状态，`:150`公共facade；`run-full-regression.mjs:179`封存record，`:272`执行读取后fromDecoded真实caller。不存在孤立未用类。 |
| 边界 L46–57 spawn/register/settle/异常优先级，Windows限制 | `run-with-deadline.mjs:69`spawn、`:80`执行timer、`:84`硬timer、`:87`abort注册、`:118`close优先callback→abort→timeout→exit、`:136`settleOnce标记再`:143`清timer/listener。Windows`:13`taskkill+`:17`unref仍原边界；不承诺OS已确认全部后代退出。同步injected-spawn内部abort竞态和settle后流listener保留是明确旧限度，不自动升级为本批遗漏。 |
| L59–64 digest顺序、extra/duplicate、filesystem首错 | `run-full-regression.mjs:196`先比原decoded摘要；`:276`逐descriptor、`:290`metadata、`:294`digest交错校验。`ArtifactInventory`没有先重算摘要吞输入篡改。 |
| R1 L66–96 structuredClone深extra接受回归 | 当前constructor`:184`和toJson`:213`已JSON.parse/stringify；`:167`freeze显式stack。2000层合法JSON不再额外依赖structuredClone递归栈；`:490`测试构造2000层extra并测试输入/输出深修改隔离，修复有效反证。不能要求丢extra或添配额。 |
| 验证 L98–107 54/54仅修复前、sandbox保留 | 当前 `hosts/deadline/status.json:40`记录“修正后当前55case四脚本整套”，`:229`仍明确整套未重跑、相关4case已验证。历史验证债仍是55case最新整套，不据此认定代码缺失。 |
| 修复复核 L109–131 JSON复制/iterative freeze、4case绿、55case待root | current源码与修复说明一致；JSON wire接受集、字段顺序、duplicate未被对象化收窄。后续status中的代表性root验证不把55套件全覆盖债核销成PASS；本轮未重跑。 |

## review-fixtures1 全章条款

| 原文条款 | 当前实现 / 实际测试caller / 状态 |
|---|---|
| L1–9 11文件/六动作、仅静态审查待协调验证 | 当前fixture全部仍只在 `packages/engine/tests`；后续status末尾有root代表性验证，旧“worker未跑”应视为过程记录。 |
| A股与依据 L11–36 交易所分类、舍入/期次、个人获知、周末/存档事实 | `tests/auction.rs:140` / `:182`合法SaveSlot→真实restore；`information_acquisition/fixture.rs:128` Scenario及`:273`更正；`publications/weekend_publish.rs:40` WeekendScenario；`save_contract/main.rs:116` SeasonedSaveFixture。均为测试输入ownership，非生产交易规则新实现。官方依据与synthetic breadth/披露排期限制保持诚实。 |
| 必要性 L38–51 N03/N04/N05/R2-N01/N02/N04六动作 | 账户/Book私人事实未由fixture自动变成本人已读；exposure fixture `attention_discovery/exposure.rs:133`每次exposed_at查询，`:146`只读codes；SeasonedSaveFixture`:132`每次fresh session，两日处理仍在；BehaviorScenario只借用MarketView/observations，不向production API塞fixture类型。 |
| 原测试保留 L53–70 六组断言/seed、三个新增case | 对应测试函数/断言源码可见；本轮不重新计算历史assert数量，也不声称所有旧测试已跑。新增save缓存clone、竞价身份、breadth输入均留在原文件中，无生产caller需求。 |
| 合法restore/invalid L72–92 不修复篡改后坏档 | `auction.rs:713`先合法档restore，`:750`之后删除active candle、`:753`直接restore断InvalidSave；`save_contract/failures.rs:17`仅serialize→decode→restore；information失败`:172` / `:189` / `:202`from_parts，坏值没再走构造repair。已实现真实拒绝路径，未因fixture方法化绕开。 |
| 检查限度 L94–112 baseline clone/身份/breadth、零覆盖未支持 | `save_contract/main.rs:140`clone_save_value仍owned Value；`:151`OnceLock只读缓存；`behavior.rs:132`setter、`:138`observed<=total guard、`:140`–142仅更新三字段。全部caller `:175`3/2、`:260`5/5、`:298`正stock_count/同值，没有observed=0。缺负向case/零coverage属于测试helper将来适用范围，未接生产输入，不能将其报成全市场观测代码遗漏。 |
| 静态快照 L114–134 diffcheck/hash/未Cargo | 旧hash与diffcheck是历史静态证据；本轮未执行Cargo，root代表性运行记录不能证明11文件所有suite通过。保留真实验证限制。 |

## 候选反证与结论

- **B1伪witness接受候选已排除**：reuse先validator后artifact比较；重算哈希不绕过typed witness。此与既有G39自由调度整体产物比较问题不同，不能以修B1解释G39。
- **B2 Writer缺测试候选已排除**：Rust Writer直接测试已经存在，root最终代表性记录Writer5/5；已有部分文件后失败是明确契约，不要求staging回滚来扩范围。
- **R1深JSON接受回归已排除**：两个复制端点与freeze均修正，深2000case在。55case完整重跑仍属验收证据债，不作为新代码缺口。
- **同步spawn内部abort竞态/Windows后代终止确认**：原报告L55–57明确旧边界，实际默认spawn不等于测试注入同步触发；本批没有承诺扩大终止确认，不凭局部可构造fake事件报生产漏接。
- **Behavior breadth零覆盖/负向guard测试**：实际三处setter调用均正覆盖，原报告明确非完整生产校验器。未找到对外可达caller或最新要求该测试helper支持全部observation，不能新增G。
- G39三个完整产物比较链（含sweep33新增统计finalizer）与Q05持续脚本测试发现范围仍由总账/对应专项处理，不用这些OOP门禁完成声明核销。

本批未确认新的代码遗漏。历史有效发现均已被当前caller/实现反证；未覆盖最新全套脚本、旧suite、真实matrix/E2E/性能的执行证据依然需诚实保留，不把后续代表性验证写成完整验收。
