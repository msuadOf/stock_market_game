# sweep59：OOP 独立总复核、初始边界与中间记录

## 全文与判定边界

- 连续全文读取 `agents/oop-refactor-implementation/final-review/final-review.md`（50 行）、`initial-boundary.md`（45 行）、`progress-01.md`（28 行），共 123 行。
- 对照现行 `b76ece3` 合并同等产品及已有实施审计，检查记录中残余/后续承诺的真实 owner、caller、恢复与临时事实消费者。仅创建本审计文件；没有产品、Git 写或测试运行。
- 正式授权正文路径在本 worktree 没有复制，改只读原工作树的 `/data1/baiyifan/workplace/stock_market_game/agents/oop-refactor-audit/challenge-2026-10-03/action-index.md` 相关 A03/N07/N32 条款；不把审查工作记录误作新的产品要求。
- 本批没有确认新 G。中间 FR-02 的六个 dated writer 组合当前已经实施；这解决内部 owner 聚合要求，不自动解决散户生产 dated writer 接线 G08。FR-01 是原独立身份记录问题，最终记录已关闭，不是生产匿名账户缺陷。

## 历史签署与当前 SHA 范围

只读 Node 的 `fs`/`crypto` 对 source-freeze 340 条 path 的当前 bytes/SHA-256 实算：**329/340 与冻结一致，11 条已变化，无文件缺失**。冻结文件于 `8cf34a1` 提交，`git diff --stat 8cf34a1..b76ece3` 对这11条确认后续改动；因此 `final-review.md:6` 明示「后续源码变化不能自动继承」有实际适用对象，不能把历史278 case PASS当当前全head回归通过。

11 条变化路径：`apps/server/tests/actor.rs`、`apps/web/src/app/save-commands.test.ts`、`apps/web/src/app/session-host-lifecycle.test.ts`、`packages/engine/examples/escrow_verification_harness/committed.rs`、`packages/engine/src/diagnostics.rs`、`packages/engine/src/diagnostics/causal/microstructure.rs`、`packages/engine/src/orderbook/book_state.rs`、`packages/engine/src/session/decision_chain.rs`、`packages/engine/src/session/institutional_behavior.rs`、`packages/engine/src/session/pipeline/decision_snapshot_capture.rs`、`packages/engine/tests/session.rs`。

差异不直接等于失效/漏洞。例如 microstructure 后续仅把 nested `side` match 合并成 `Some(direction)`，BookState 后续移动原私有异常 fixture 测试并补中文范围说明；现有总账/各层复核已记录后续变化。本批不签署所有11条完整diff，亦未重跑其测试。机器 manifest 只证明原签署与索引，不独立证明当前语义。

## initial-boundary：45 行逐章节

| 原文位置/条款 | 当前代码及范围证据 | 状态 |
|---|---|---|
| `initial-boundary.md:3` 身份/基线/尚未final、`:8` 只读限制 | 最终记录 `final-review.md:4` 明示 reviewer 没有实施源码/跑测试；本批也不把该 reviewer 的初始状态当最终缺交付。 | 审查范围记录，后文final已收口 |
| `initial-boundary.md:11` 权威 action-index、128ID、optional全授权 | 正式 `action-index.md:146` N07 及 `:154` PositionExperienceTransition 是同一动作内部组合，不另设第二账本；`:2024` 两accumulator是N32子目标。`action-ledger.json` 当前128条、完整正文/optional及version binding字段存在。 | 授权内部重构已落实，不能从旧低优先级省略收益明确子目标 |
| `initial-boundary.md:15` 七项核销：owner/caller/旧写口/保持行为/短测/独立diff/本人亲读 | 当前 Account字段受控、AccountBook cache invalidation、personal_state聚合、P3资源对象、协议checkpoint下列逐条核。最终 JSON机械完整性与278记录存在，仅是历史证据。 | 没有把wrapper名称或纯计数当实现证明 |
| `initial-boundary.md:27` AccountBook→Account→Position COW/缓存/T+1 | `session/account_book.rs:18` 共享group/page，`:124` get_mut make_mut并`:131` invalidate；`:110` values_mut、`:157` insert也invalidate；`account.rs:146` 等通过Arc::make_mut修改。strategy Replace `pipeline/npc_state_projection.rs:200` 经get_mut再restore_strategy。 | 正式caller已迁移，未见绕过page验证缓存 |
| `initial-boundary.md:28` Order/parent/plan身份不得改交易优先，`:29` P0/P1/P9 | `pipeline/ready_ingress.rs:97`/`local_admission.rs:100` 实际资源冲突受理；`account_validation.rs:1110` AccountBudget；`candidate_commit.rs:96` 单次swap；私有typed事实与最终Settlement保持分工。 | 已实现保持契约；G39工具跨worker过严另属既有问题 |
| `initial-boundary.md:30` state/checkpoint/facts与日终SaveSlot | `session.rs:1135` CommittableSessionState，`:4075` clone_for_shadow，`:4169` commit_from；`protocol/civil/session.rs:19` 完整checkpoint，`:73`/`:79` 捕获/换回；`:34` PublicationFactCursor。ADR0025:30 公共日终gate保留。 | 内部活动fixture不是公共保存能力 |
| `initial-boundary.md:31` 四行业/个人经历/先guard后apply与旧失败面 | `experience/position_transition.rs:11` 只借同code legacy与optional epoch，没有新权威；dated lifecycle调用真实组合。公司账套各行业owner与失败边界承接现有accounting复核，不能用OOP通过核销G35/G36。 | 保持行为重构，不承诺修复既有产品断链 |
| `initial-boundary.md:32` Host/UI资源身份，`:33` 工具fixture生命周期 | `protocol/civil/session.rs`分离fact cursor/runtime/save candidates；CaptureArtifact在 `examples/escrow_verification_harness/runtime.rs:135` 拥有bytes/hash/path，`:587` writer处理输出资源；生产/测试责任没有变成万能manager。 | 当前内部对象有实际资源/状态；跨宿主E2E仍另需验收 |
| `initial-boundary.md:35` 短测限定与领域依据，`:41` 已读/待全文/冻结 | `final-review.md:44` 继续明确未跑全回归/E2E/性能和部分doctest；当前 `docs/testing.md:91` 同样要求普通case/command十秒。 | 不将未跑长测列为本OOP任务遗漏，不将短测当长验收 |

## progress-01：28 行逐章节

| 原文条款 | 当前真实owner/caller与后续收口 | 状态 |
|---|---|---|
| `progress-01.md:3` 未final不批准128项 | 最终 `final-review.md:3` 后续签署，限定340最终源与278唯一case。 | 中间状态已被final取代 |
| `progress-01.md:7` Account/AccountBook/Replace/restoration | 当前cache/caller证据见上；`session/persistence.rs:938` 仍在恢复前完整validate_personal_states/save validation，不能用受控恢复方法绕输入守卫。 | 生产接缝保留 |
| `progress-01.md:8` 四成员runtime唯一、四map保存/hash、同键guard、duplicate install旧部分写 | `decision_chain/personal_state.rs:10` 单一participant四成员，`:69` take，`:90` install；`:99` duplicate watchlist分支只覆盖watchlist然后panic；`persistence.rs:1104` 比较四map键，`:1112` 具体InvalidSave。 | 保留旧失败面，不要求panic内部强原子化；公共键集接受范围仍校验 |
| `progress-01.md:9` N32两个accumulator有实态、方向/quote/扫描起点 | `diagnostics/causal/microstructure.rs:22` directions+pairs+same，`:58` prior_quotes+impacts+recoveries；`:81`后续quote从index+1，`:122`恢复从index，`:166`每fact先direction后quote，side=None不填方向。 | 已实现，不是static facade；观测冲击不是现实因果证明 |
| `progress-01.md:11` AccountBudget/SellerChargeAllocation/ReceiptSettlementPlan | `account_validation.rs:1116` 从seal snapshot加载，`:1139`只接受实际budget update，`:1148` lane adoption；`transition.rs:197`累计未收费用封顶，`:215`固定三费顺序；`settlement.rs:40` owns totals，`:54`正Fill，`:96`账户并行，Buy后Sell。 | 生产消费者存在，仍只结算charged，不重算nominal |
| `progress-01.md:13` 组review承接/截断/机械台账局限 | final:24–26明确本人亲读与具名组级diff分别披露；当前action-ledger与review bindings有原reviewer/final SHA字段。 | 有后续闭合，不能把本批三篇全文冒称重读340全部源码 |
| `progress-01.md:21` FR01 accounting匿名review | final:30明示canonical身份和最终版本补录；final-integrity.review_bindings的结构存在，旧FR01没有指向产品账户逻辑。 | 已收口的审计元数据问题，不新增G |
| `progress-01.md:22` FR02六dated内部组合未实施 | 现行 `experience/feedback/lifecycle.rs:43`/`:148`/`:202`/`:221`/`:305`/`:361` 由six writer实际调 PositionExperienceTransition；它只借legacy/optional epoch，见 `position_transition.rs:25`。final:31说明原reviewer复审/4短case。 | 内部组合已实现；散户session仍legacy是另一契约G08 |
| `progress-01.md:24` 中文/保护文件/待冻结 | final:36确认原搬移英文范围与三保护文件；本批只比较340产品源，未重新审译全部注释或重算原工作树AGENTS三保护文件。 | 精确限制，本批不扩大保护签署 |

## final-review：50 行逐章节

| 原文位置/条款 | 当前核对结果 | 状态 |
|---|---|---|
| `final-review.md:3` 128/128与`:6`340冻结署名 | action-ledger128、source-freeze340、原final-integrity没有miss/drift，当前重算329一致11变。 | 历史签署范围准确，当前后续源不能自动继承 |
| `final-review.md:8` 五组ID/真实caller与内部目标 | 前述AccountBook、六writer、microstructure、三资源对象、facts cursor、artifact均有实际状态；直接DTO/fixture备选允许，不给薄候选额外forwarding。 | 相关实质owner当前仍存在，没有新增遗漏 |
| `final-review.md:14` 三门禁领域/必要性/边界保持 | 原语义保持包括旧首错/checked/部分写入和外部API收窄；当前生产调用仍是受控field/owner，原有G仍不因此核销。 | OOP通过≠整个市场功能完成或现实法源全认证 |
| `final-review.md:20` 仓库外源码API不兼容声明 | Account/Position受控private与内部迁移是授权重构；wire/serde不从源码可见性推导变更。 | 不要求恢复任意public setter来兼容假想外部调用者 |
| `final-review.md:22` 独立coverage与本人亲读 | 记录明确组级全diff承接与关键段亲读；本批只是新一轮文档全文和指定实际consumer核查。 | 不冒称个人重验所有历史组review或340完整diff |
| `final-review.md:28` FR修复、N01/N02映射、lifecycle/地产署名、其他有效发现 | metadata中review_binding_missing为空、actions状态通过；FR02当前源码存在确证。lifecycle/地产/前端其他已修事实由既有三大层复核承接。 | 无从旧中间发现新增现行漏项 |
| `final-review.md:38`278 unique Rust、compile/typecheck/frontend | final-validation.json统计278/278、最长1.778s、并行8、Rayon每进程4、deadline10000；records真实数据仍保留。 | 历史有限短验证；本轮没有重跑，不自动用于11后续路径 |
| `final-review.md:44`明确未运行项、非TDD红史、有限证明 | 明确未跑长矩阵/GUI/compile_fail doctest，只有compile/diff的长fixtures不称通过。 | 诚实范围，不写成漏实现；长期验收债仍在总账 |
| `final-review.md:46`只允许plan元数据close、`:48`最后索引复核 | final-integrity.allowed_post_signature_changes两类纯元数据；278 schedule全false与pending清空是历史验证索引收口，不代表新运行。 | 后续11源码改动需各自证据，不能用这个许可覆盖源码 |

## 新候选与反证

| 候选 | 反证/当前边界 | 判定 |
|---|---|---|
| 六dated内部借用仍没实现 | 六真实writer均调用PositionExperienceTransition；旧FR02中间态已经被final和现行源码取代。 | 已实现 |
| OOP已通过所以G08/G16/G39可以核销 | 内部writer组合不代表散户生产caller切换；`RootReadContext::capture`仍`:25`全PlanBook.clone；K7比较是另一个工具契约。 | G08/G16/G39仍独立保留 |
| AccountBook字段受控但Replace绕过cache invalidation | 替换正式经get_mut→invalidate→restore_strategy；mutable迭代/insert同样invalidate。 | 未确认遗漏 |
| 重复个人state install部分写证明新提交不原子 | 这是原panic失败面刻意保持，只作用candidate；市场typed fatal原子commit与底层panic保证不是同一契约。 | 不新增，也不擅自catch_unwind |
| 历史278 PASS可证明当前head所有源码通过 | 329匹配11变，原review明确禁止自动继承；后续审计已有相应差异检查但本批未重测。 | 范围限制，非新产品G |
| reviewer未执行长验收因此128动作未完成 | 用户当时授权的是行为保持对象迁移与针对性短验证；长/E2E明确不在该批运行范围。 | 不按未跑长测判生产缺失 |

本轮实算只验证冻结path/hash/bytes，没有复验全部独立review来源、法源、测试或长期性能。结论是未发现本批工作记录引出的新增生产遗漏，并准确保留历史签署与当前源码范围差异。
