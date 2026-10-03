# Sweep50：两份已删除验证文档的末版全文审计

## 原档存在性、完整性与范围

已先用 `git cat-file -e` 确认两个明确 revision:path 都存在，退出成功后用 `git show` 全文读取；不是只筛关键词或读取当前同名文件。以下字节数及 SHA-256 直接针对 git show 输出原始内容，未做文本归一化。

| 原档 | 实际来源 parent | 行数 / 字节数 | 原始 SHA-256 |
|---|---|---|---|
| `scripts/simulation/escrow-corpus.md` | `21479883a9aa1ba6cda34df7c41e02018dec67d3^` = `84aca54f3e4d11aaf4a788766ec419acfaa266f8` | 190 / 11042 | `7f1a754a29f3e3723b882c2b0c1bfae4a9cc34a63df1902a10b6d6094b594d54` |
| `packages/engine/tests/preserved-test-inventory.md` | `f4ad7b4d1ffeeaf8d3bca9a11c599ba1fee0a905^` = `e0b348d56d98f23932e13102633ce49b84ed1433` | 119 / 10401 | `c047cf3d3596db0c560c8a1d1676e632b6105844239f33fdba558559043236a2` |

合计连续全文读取 309 行、21443 字节。代码基线仍为 `4ad5a2e`（产品树按父任务说明等同 b76ece3）。本轮仅静态审计，不运行测试、长矩阵、编译或 Git 写操作。

## 当前决定的优先关系

`docs/test-cleanup-checklist.md:81` 的 §12 是用户确认全清的后续决定，`:85` 明确退役 Rust corpus replay/projection，`:86` 退役 JS adapter/exact comparator/旧evidence bundle装配，`:88` 明确接受历史密封证据原封保留但不再有可执行代码复验。保留的现行格式 helper 依 `:87` 明确可以只由测试调用，不把未接生产再次登记成遗漏。

旧测试逐字冻结的取消另见 `docs/superpowers/plans/2026-09-24-single-world-multithreading.md:45`、`:172`；该决定撤销指定旧源码hash门禁，不撤销现行交易规则测试、守恒及失败原子性。ADR-0017 当前 #4、事件受理及并发语义也优先于原档当时要求。

## escrow-corpus.md 逐章条款族

| 原档行号 / 条款 | 当前状态与真实 caller 核对 |
|---|---|
| 1–16：工具非第二引擎、整数金额、CLI负控及工作区临时目录 | 旧corpus命令退役；金额/股份领域原则仍保留。现行 matrix 在 `scripts/simulation/run-escrow-verification-matrix.mjs:609` 同时传 TMPDIR/TMP/TEMP，`:852` 检查工作区临时目录契约。未因删除corpus脚本放弃正式工具临时目录边界。 |
| 18–55：sealed root/manifest/run哈希、只选active、路径/符号链接/整数拒绝、ADAPTED非PASS、事件/状态/provenance提取 | 全族属于获批退役的 sealed corpus adapter/projection；当前不再承诺旧run到CORPUS_SCHEMA的可执行迁移。原密封数据是否能旧新比对与当前SaveSlot业务校验分开，不能恢复旧解码兼容。 |
| 57–69：prepare-current生产重放、capture/diagnose而非semantic PASS | `escrow_corpus_replay`与diagnose-current已明确退役，未保留假caller。现行 `packages/engine/examples/escrow_verification_harness/runtime.rs` 仍真实捕获生产入口；其 `:331` 的 corpus_projection 为固定None/null，符合cleanup:85保留JSON键集的决定，不是漏接投影。 |
| 71–101：surface extraction、zero-cash witness不得冒名、rollover与cross-tick卖方费用受控提取 | 历史surface/witness集成要求随整栈退役核销，不再要求补旧zero-cash见证。现行行为仍有 `tests/session.rs:2897` 零现金卖单接受、`:2956` 卖单只预留股份；`pipeline/continuous_matching_tests.rs:1398` 部分卖出保留shares/audit。这些不是旧密封surface复验，不能假称已完成旧new映射。 |
| 103–133：buyer-fees、T+1、自成交、instant full fill身份、price-cage P4拒绝耗一个ID | 旧extract-equivalence及精确old/new ID差值映射退役，现行委托/回执引用与P4拒绝耗预分配ID仍由ADR-0017约束。`tests/session.rs:545` 有真实连续笼子，`:4649` 有真实部分成交恢复；保留真实pipeline，不凭旧字段缺席判漏实现。 |
| 135–152：acceptance-flip/three-leg catchup、独立seller账与nominal-fee见证 | 旧密封分歧#9映射验收退役；实际卖方费用封顶与实收增量规则仍有 `pipeline/settlement_tests.rs:415`、`:633` 等当前收据结算测试。未要求伪造或补写历史数据。 |
| 154–178：逐leaf精确mapping、每差异恰一行、禁止整对象whitelist、现金方程限定、RNG/计划/策略相等 | 此为旧exact comparator独立审查门禁，已获批退役，不移植成当前自由调度整局字节冻结。当前生产仍保留身份、去重、局部先后、账本守恒；跨运行不同真实受理允许不同RNG后续/业务轨迹，不能反推全局交易总序。 |
| 180–190：corpusUnmappedDiff正负控、compare双门禁、stress走新引擎守恒 | 旧unmapped negative gate与compare CLI退役。现行 matrix 在 `run-escrow-verification-matrix.mjs:15` import verifyConservationSnapshot，`:486` 真实调用；该helper在 `escrow-verification-contracts.mjs:373` 保留。matrix自身`:583`/`:774`仍强比较artifact的问题归G39，与旧corpus retirement不是一回事。 |

## preserved-test-inventory.md 逐章条款族

| 原档行号 / 条款 | 当前状态与替代证据 |
|---|---|
| 1–7：(a) sealed attempt12/overlay源码字节冻结 | 已明确撤销旧提交hash独立门禁；不是当前测试维护的永久接口。当前不再有 verify-preserved-tests或inventory机器源正式caller，属于主动清理。 |
| 9–18：(b) #9四项精确放行、冻结旧卖方现金预留测试 | #9 已接受卖单不占现金、费用实收封顶；旧预留测试必须迁移而非保留错误预期。当前 `tests/session.rs:2897`、`:2956`直接验证现行语义，soft-budget仍属同账户个人资金分配，不能当订单reserved_cash。 |
| 20–40：(d) PF1 fixture缩减，只缩工作量不削业务边界；year-close长验收必跑 | 逐hunk/source/assertion hash冻结已撤销，但当前行为覆盖继续存在。`tests/company_scenarios/main.rs:147`–`:168`检查短fixture阶段和账户类别；`controlled.rs:144`同公开材料两个先验修订；`controlled_experience.rs:4`同P&L不同经历决策；`lifecycle.rs:52`完整跨年链。正式 `scripts/run-full-regression.mjs:19`列required长项，`:368`查产物，`:378`以--exact --ignored执行，真实caller没有遗漏。 |
| 42–57：(c) step/save Result受控迁移、seeded holding不得伪造失败、双侧filled_qty==200 | 旧API-only逐hash允许集退役，防御式Result与业务边界保留。`tests/experience.rs:93`/`experience_feedback/main.rs:250`保留seeded holding无买单失败事件规则；`pipeline/continuous_tick_transaction_tests.rs:521`恢复真实活跃计划，`:539`分别推进uninterrupted/restored，`:560`两侧各断言filled_qty=200，`:562`保留相同子单。未因旧符号/目录删除丢掉该业务边界。 |
| 59–81：批准#4同tick新单不可撤迁移、短auction fixture | **旧#4已被新交易模型修订。** `docs/decisions/0017-escrow-parallel-tick.md:128`明确由ADR0018取代为可撤阶段按当前剩余量处理；现行 `tests/auction.rs:1058` same_tick_auction_place_and_cancel_releases_the_order，`:1099`确认无余单，`:1102`确认预留零。不能按旧inventory要求恢复SameTickOrderNotCancelable。可撤窗口测试仍在`:995`。 |
| 83–114：#6/#7历史1429事件、720tick/FIFO、规范hash与v2字段精确映射 | 这些是当时真实比较证据，保持历史含义，但不再有指定字节复验门禁。当前存档/事件协议已有；旧baseline profile等明确被生产StrategyState与现行SaveSlot替代，允许编辑档/恢复重建遵守后续ADR0019/0025。自由调度跨实例未来artifact强等价仍应收窄，见下条候选。 |
| 115–119：零参与者step_skeleton摘要与机器源完整hash | 摘要冻结/旧machine inventory门禁退役。当前tick、日界、行情及失败原子性仍由现行流水线/Session测试验证，不要求重建旧step_skeleton公开路径。 |

## 新候选及反证

- **没有新产品遗漏。** 旧zero-cash corpus witness、surface映射、精确差异表、root active索引适配、old/new字节冻结都在用户明确批准退役范围，不因原文当时写“required”就变成当前必做。
- **KEEP helper未接矩阵不构成新遗漏。** cleanup:87已单独接受validateObservation/verifyDeterminismMatrix/verifyPerturbationGate/artifactReceipt和validatePerformanceReport只由测试消费；仅verifyConservationSnapshot仍有matrix真实caller。现行matrix比较artifact契约错位仍归G39。
- **部分成交恢复双侧200没有消失。** 原inventory:55的业务目标已迁入真实B1 partial-fill恢复测试，并非要求原测试名原hash永存。
- **现行另一个测试仍强等价，交父任务分类。** `packages/engine/tests/company_scenarios/restore.rs:22`–`:28`自由推进两个实例后逐tick比较事件字节，`:39`–`:41`比较未来日终存档；fixture在 `company_scenarios/main.rs:61` 有五股、`:70`有2散户/2机构/1游资，`:142`没有静默关闭它们。立即恢复字节一致（restore.rs:16）仍合法；未来未冻结真实受理轨迹的字节断言属于同类验收契约疑点，不能用退役原inventory作为保留它的依据。本轮未运行用例，不声称已复现失败；不直接新增产品G，建议与G39/并发验收边界一起复核。

本轮原档全文与最新退役决定之间未发现未替代的现行产品必做要求。历史证据不可再执行复验的损失是用户已接受边界，不能假称仍可验证，也不能把它重新列为未实现代码。
