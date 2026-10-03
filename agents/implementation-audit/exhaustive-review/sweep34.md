# Sweep34：Escrow 历史 smoke 与 D6/D7 见证全文复核

## 范围

- 已连续全文读完 `.omo/evidence/escrow-parallel-engine/task-3-8-smoke.md` 69 行、`task-3/d6-d7/README.md` 58 行、`task-3/d6-d7/structured-comparison.md` 51 行，共 178 行；没有以标题或关键词扫描替代全文。
- 已读根 AGENTS/principles，检查 `.omo` 无额外嵌套 AGENTS；最新核销依据为 `docs/test-cleanup-checklist.md:81` 第12项（特别是85–88行），旧总账 S06 与159行核销声明。该任务只新增本报告，不修改密封记录、产品、Git 状态。
- 产品基线 `b76ece3`，读取时 HEAD `4ad5a2e`；只读 diff 核对本次引用的 failure/candidate_commit/account_settlement/persistence-v2/matrix/test-cleanup 文件，产品无差异。未运行测试、编译或长验收；执行了现存历史文件的 SHA-256 只读核查。

## 全文条款映射

| 原文位置/承诺 | 当前状态和生产证据 |
|---|---|
| smoke 1–16 行：2026-09-23 revision、用户合并 smoke、源码不变、16 jobs/16 Rayon/16 test threads 与300秒 | **历史验收记录。** 这些配置属于当时命令，不是当前源码默认值承诺；没有本轮重跑。现行普通短测/共享长验收 deadline 按根AGENTS与test-cleanup约束，不能用46秒历史lib测试冒充本轮普通短测。 |
| smoke 18–23 行：785通过/0失败/0忽略、46.70秒执行、122.42秒总wall、RSS、原始日志hash | **历史字节证据完整。** 已实际计算 `task-3-8-current-smoke.log`，SHA-256=`5649d2171828894f35e61cf6871c068e5beae9c6fbb5d70c4e287fe1a445b3a7`，与原文一致。测试数量与源码revision绑定，不要求当前保持785个测试。 |
| smoke 25–30 行：Task3 envelope守恒/audit、Task4 P3/P4拒绝回滚、Task5 receipt结算/预算截止 | **当前生产路径仍存在。** `pipeline/mod.rs:238` 健康检查→private TickShadow→P0/P1 seal；`ledger_validation.rs:7,102` 校验 receipt kind/before/累计进度/audit；`account_settlement.rs:162` canonical unseen receipts，166行准备Account shadow；`settlement.rs:54,74` 只消费Fill/charged并拒买方印花税，95行起并行准备全部账户，139行错误返回后才交出patch。当前成交接线 `continuous_matching.rs:517`，资金/股份预算 `account_validation.rs:710,740,758`。不把测试helper当唯一生产caller。 |
| smoke 27–30 行：Task6 opening/closing/day-end、Task7 typed fatal/poison/P9、Task8 v2 persistence/live恢复 | **当前生产路径仍存在。** `auction_day_end.rs:947,968` 明确开收盘边界，`stock_auction.rs:308` 完成真实股票竞价；`auction_day_end.rs:2144,2156` 清空余单/envelope后日界T+1解锁。`failure.rs:51` step_inner走权威phase dispatcher，失败poison；`candidate_commit.rs:126,131,136,153` 先验证event/receipt/ledger/cursor，再97行无失败swap；`persistence/v2.rs:122,228` 捕获与验证完整strategy/live envelope/receipt状态。 |
| smoke 32–44 行：Task7宿主、Task8 29/29、preserved verifier、K7/独立审查与不重跑全部历史命令 | **历史证据/部分工具退役。** 宿主、K7通过数字不是本轮验收。旧preserved-test/密封bundle整装不可按历史记录重新要求实现；最新test-cleanup12及总账159行已明确接受退役。保留当前typed fatal实际生产接口如 `apps/web-wasm/src/lib.rs:234,248,344`；完整三宿主协议由宿主sweep复核。 |
| smoke 46–69 行：closing gates、preserved/integration/web/types/three-host日志、Corepack失败不计通过 | **历史证据字节完整/失败显式。** 本轮计算 `task-3-preserved.log`、`task-6-8-integration.log`、`task-7-web-smoke.log`、`task-7-types-generation.log`、`task-7-three-host.log`，五个SHA-256全部与文中一致。原文明确失败Corepack wrapper不计通过；后续test-cleanup90行也保留环境失败边界。不能把文中历史“current-source”理解成今天HEAD。 |
| README 1–19 行：raw捕获不是第二实现；c434f1d→2b3e55b+overlay、特定seed/2股/720tick、1429 events | **密封历史来源。** raw event文件实际hash分别为原文ab0001e0…、e8e56616…；本轮未尝试v1重播或新造overlay。当前`extraction_replay.rs:229,264`仍有当前重放独立hash验收，修复前“反改时间戳复原旧hash”已按test-cleanup60行退役。 |
| README 21–34 行：D6去display seq保留tick与完整业务payload、21位置、原同实体FIFO | **历史比较结果，文件等值可核查。** 两个events-normalized文件实际SHA-256均=`550dc6bd1fdf6198af7bfbb436db04d03bc49f46c575b92e0f9c219e46e650b7`。同实体FIFO不是靠全局multiset即可证明，本轮不以文件等值升级为重新执行21位置/FIFO比较器。当前event identity验证实际caller `candidate_commit.rs:162` 检查events/key数目、payload投影、去重；receipt identity同文件194行。 |
| README 36–50 行：D7只移除枚举representation边界、common与profile等值、任何不批准差异必须失败 | **历史文件hash已核对。** mid-common两侧=`615ea983…`、end-common两侧=`dfedaeaa…`、profiles两侧=`d3c49975…`，与原文完整hash一致。当前只接受schema-v2（`persistence/v2.rs:97,109`），不会把旧v1自动fallback成新档；生产恢复不依赖这些common投影。 |
| README 52–58 行：structured JSON持久结果/输入hash、Markdown摘要、比较不是可执行verifier | **历史记录保留。** 本轮检查JSON对应hash与布尔字段，未修改任一见证。latest cleanup88行明确不再有可执行代码复验，README自己也不承诺它是运行程序。 |
| structured 1–15 行：PASS仅属旧baseline/candidate；720ticks/1429events、身份/FIFO/完整payload、21位置9ticks | **历史验收，不是当前确定性保证。** `structured-comparison.json:50,52,53` 保存FIFO true及canonical multiset双侧hash=`96109015…`。原README normalized文件hash=`550dc6bd…`是另一表示，摘要明确写canonical，不可直接把不同字节表示hash判为错误。 |
| structured 17–32 行：mid共用权威字段等值、16完整StrategyState导出旧profile、schema/policy/runtime变换、cursor0 | **历史表示变换/当前运行事实仍建模。** `persistence/v2.rs:14` 明确runtime字段，122行capture，184行取权威完整strategy；当前新局没有v1迁移，97/109行显式拒旧版本。摘要common hash=`506460fe…`在JSON234/235行同值；与common磁盘格式hash不能机械比较。mid zero envelopes/cursor是该历史fixture结果，不是全游戏必须恒为零。 |
| structured 34–47 行：end同样变换、cursor2、seen2 | **历史结果/当前持久链仍建模。** `session.rs:1184,1186`权威receipt seen/cursor；`candidate_commit.rs:65,72` commit前cursor一致且rebase不改cursor；`persistence/v2.rs:136,171,199`真实capture；恢复验证228行起。旧fixture的2不是生产常量。 |
| structured 49–51 行：每个输入bytes/hash在JSON | **历史记录，非现行可执行门禁。** 可用raw文件确认byte来源；本轮实际核对README列出的10个raw/normalized/common/profile文件hash。未重新运行retired structured comparator，因此只声称字节hash一致，不声称历史所有predicate复验通过。 |

## 退役核销与保留边界

`docs/test-cleanup-checklist.md:85` 已批准删除 `escrow_corpus_replay` 与 CorpusProjection族；86行删除旧adapter/corpus/exact/prepare-baseline以及bundle装配模式；88行明确密封历史证据仍留档但不再可执行复验。当前源码相符：`examples/escrow_verification_harness/runtime.rs:107,331` 保留corpus_projection键并固定None，`committed.rs:176`输出同样null占位；实际current-run矩阵只import并运行 `verifyConservationSnapshot`（`run-escrow-verification-matrix.mjs:15,486`）。artifactReceipt/validateObservation/determinism/perturbation helper与validatePerformanceReport仅测试调用已获批准保留（cleanup87行），不登记为生产漏接。

生产失败隔离没有因旧corpus清理消失：`failure.rs:79` 权威tick错误只写poison；`candidate_commit.rs:153,247` 所有可失败准备在authority swap之前；账户结算直到 `account_settlement.rs:215` 才返回完整账户/retail/belief/seen patch。测试源码 `failure_tests.rs:4,30,112` 保留状态与poison边界；`persistence/v2_tests.rs:357,420,586,680,728` 保留版本拒绝、live恢复、卖方逐腿费用历史和zero-cash真实生产step路径。测试源码存在不代表本轮已运行。

## 候选反证

1. **README与structured摘要hash不同：不认定新缺口。** normalized磁盘文件与canonical业务多重集是不同表示；common磁盘文件与structured canonical字段同理。raw来源hash一致且structured JSON与其摘要一致；没有原可执行comparator，不伪称已重现canonical算法。
2. **缺旧sealed corpus replay/comparator/preserved verifier：已明确核销。** 不要求恢复v1、时间戳改写、旧策略profile桥或bundle装配；不把获批删除报告成“尚未实现”。
3. **smoke通过785与当前测试数量不符：不是产品缺失。** 这是特定旧revision运行记录；今天不能借其PASS给当前HEAD背书，也无需维持历史case数量。
4. **quiet点live_envelopes=0导致无法恢复挂单：反证成立。** 当前v2_tests420行测试真实live envelope与receipt prefix，生产capture/restore非恒零；历史D7fixture只是安静点样例。
5. **helper只有测试调用、无bundle装配接线：获批KEEP边界。** cleanup87行明确承认，只有守恒校验仍在真实矩阵caller，不增加虚构生产保证。
6. **账户结算一半成功后失败污染authority：本范围未见。** settlement并行先准备所有account shadows，candidate成功才swap；receipt/经验投影错误也在完整patch前传播。fatal进入poison，save通过healthy检查拒绝保存。

本轮新增确认生产实现遗漏为0；历史证据文件hash核对通过，所有历史执行和FIFO/canonical比较谓词仍只按原记录陈述。没有把历史PASS升级为当前全量验收，也没有重开已获批准退役项。主审需独立复核本报告完整diff。
