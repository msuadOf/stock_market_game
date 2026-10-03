# Luna34：Task 3–8 smoke / D6-D7 全文复核

## 范围与方法

- 产品基线按委托为 `08e4fc7`（merge 同）；本工作树读取时 HEAD 为 `a7c7ce3`。仅静态核对，不运行测试、不修改产品或 Git。
- 连续全文读取 `.omo/evidence/escrow-parallel-engine/task-3-8-smoke.md` **69 行**、`task-3/d6-d7/README.md` **58 行**、`task-3/d6-d7/structured-comparison.md` **51 行**，合计 **178 行**，均读至 EOF；没有以摘录代替全文。
- 先读根 `AGENTS.md` 与 `docs/principles.md`。按最新 `docs/test-cleanup-checklist.md:81-91`，尤其 `:85-88`，理解旧 sealed corpus 验证栈的批准退役边界；旧历史通过数不提升为当前结果。

## 章节矩阵

| 原文章节/行 | 原文主张与旧结论复核 | 当前代码/调用者核对 | 判断 |
|---|---|---|---|
| smoke 1–17：日期、修订、授权的一次合并 smoke、并发参数、PASS 数与日志 digest | **历史记录**；文中明确称 2026-09-23 时源码不变，且不声称重跑每条旧命令。其 785/785、16 jobs/threads、46.70s 均不代表当前 HEAD 或本轮执行。 | 当前唯一生产 tick dispatcher 是 `packages/engine/src/session/pipeline/authoritative_tick.rs:20-54`；三个 phase 先准备候选、过 precommit 验证，再提交。 | 没有把旧 smoke 冒充现况；无产品遗漏。 |
| smoke 19–27：覆盖 Task 3–8 的 conservation/audit、P3/P4、receipt/settlement、auction/day-end、fatal/commit、v2 persistence | 保留为当时覆盖说明，不据此认定今天这些测试仍按旧符号可跑。 | 现有 P5 会在私有 ledger candidate 上插入 envelope、应用 receipts、校验完整 evidence 和 cursor，成功才返回候选：`receipt_aggregation.rs:38-103`。P9 的 `prepare_candidate_commit` 校验健康态、cursor 并 rebase，`commit` 是 authority swap：`candidate_commit.rs:45-99`。 | 当前生产事务语义仍有代码承接；旧日志不是当前验证。 |
| smoke 29–44：Task 7/8 专项旧结果、verifier、K7、跨组件评审 | 全属历史专项证据，文中也限定为“already completed”；不能据此声称当前宿主或完整验收已通过。 | 最新 cleanup `docs/test-cleanup-checklist.md:85-88` 明确删除 corpus replay 与 sealed bundle 装配逻辑，密封证据留存但不再可执行复验。 | 按新决定退役；不重新要求旧工具或把 evidence 当现行验收。 |
| smoke 46–69：closing gates、Corepack wrapper 失败及底层 types 替代步骤、三宿主旧结果 | 原文诚实记载 wrapper `ERR_VM_DYNAMIC_IMPORT_CALLBACK_MISSING`，并未计通过；这是历史环境事实。后列 exit/pass 值不升级为现在通过。 | `docs/test-cleanup-checklist.md:88-90` 仍将“无执行代码可复验”与历史日志留存分开，且明确有一次无日志 exit 1，不宣称 5/5。 | 旧结论边界诚实；无静默掩盖。 |
| README 1–16：原始 replay 来源、输入情景、事件数/hash | 历史 `c434f1d`→`2b3e55b+overlay`，特定 seed、两股、720 ticks、1429 events；不能作为当前重放输入或当前实现证明。 | cleanup `docs/test-cleanup-checklist.md:60-61,85-88` 已移除旧反改时间戳测试与 sealed replay 栈；当前生产 dispatcher/receipt链见上。 | 依批准退役，不补造重播。 |
| README 18–30：D6 仅去 seq、保 tick/payload；21 个位置变化、规范输出 digest、FIFO/identity 结论 | 这是历史 witness 的检查口径。README 报规范文件 hash `550dc6…`；structured comparison 另报 business multiset hash `961090…`，名字/投影描述不同，不能仅因 digest 不同推定冲突。 | 历史 comparison JSON 自记谓词；现行 tick 提交仍验证 receipt keys、event keys、应用 journal：`candidate_commit.rs:126-159`。该机制不等价于旧 D6 对比，也无需等价。 | 旧比较不再执行；identity 保障由现行契约单独承担。 |
| README 32–50：D7 common projection 字段边界、mid/end/profiles digest 与可手工复核说明 | paired common JSON 当前原始字节确实各自成对相同；四个文件原始 SHA 与 README 所列 `615ea9…` / `dfedae…` 一致。 | 本轮只核 hash 与配对关系；不对历史 baseline/candidate 本体作重放。存档现行入口 `persistence.rs:258-322` 对 schema/policy、market/sequence/order/history集合显式校验；runtime hydration 引用位于 `persistence.rs:181`。 | 存档与恢复由当前实现另行承接，不能用 D7 旧 witness 代替。 |
| README 52–58：machine-readable comparison、Markdown 摘要不是 executable verifier | 该声明与最新 cleanup 一致。 | `docs/test-cleanup-checklist.md:86-88` 说明 matrix 去掉不可再生的 bundle 装配，corpus evidence 只留档；:87 区分仍保留但已无生产 caller 的 helper 与唯一在运行路径使用的 `verifyConservationSnapshot`。 | 不应把这两个 Markdown/JSON 摘要误当今天可执行门禁。 |
| structured comparison 1–13：PASS、旧 commit、D6 720 ticks/事件数及断言汇总 | PASS 限定于当时同一组 raw captures。 | 当前成功交易仍走 phase-specific prepared commit；失败发生在 swap 之前则只丢候选：`authoritative_tick.rs:25-47`、`candidate_commit.rs:45-99`。 | 历史 PASS 有限有效；事务所需行为现行实现覆盖。 |
| structured comparison 15–23：每 tick计数、identity、FIFO、OrderAccepted/OrderCanceled identity | 不把旧独立比较当作当前证明。 | ReceiptAggregation 在候选中应用每个转移并检查总守恒及全局 cursor：`receipt_aggregation.rs:60-103`；envelope ledger 的 transition 先 private shadow 后 swap：`ledger.rs:113-145`。 | 无现行 identity 契约缺口证据。 |
| structured comparison 25–51：mid/end D7 common hash、StrategyState推导、runtime_v2字段转换 | 本轮复核确认 README 与 JSON 配对 raw hashes 一致；另发现 structured Markdown/JSON 所报的 `baseline_common_fields_sha256` 是 `506460…` / `726ba5…`，与 README 及四个 `*-common.json` 的 raw SHA `615ea9…` / `dfedae…` 不同。structured comparison 未在其章节说明这是另一种规范化投影或为何两种 digest 可并存，字段仅称 “common fields”。 | 静态读取 `structured-comparison.json` 可见相应 digest key；用 `sha256sum` 对四份现存 common JSON 可复现 README 数值且候选/基线相同。未修改、不重写历史 artifact。 | **新候选：D7 历史报告 hash 语义/口径未闭合。** 这影响证据可审计性，不是产品实现缺口；若只对现存 JSON 各自 SHA，本候选不成立，但原报告中另一个 digest 的生成输入/规范化未由三篇全文解释，需将两套口径明确关联后方可排除。 |

## 现行账本、receipt、回滚与恢复链

- Receipt 交易：`receipt_aggregation::apply_session_receipt_transaction` 先拒绝 Session/ledger cursor 不一致，事务成功后同步更新；owned 路径在 candidate 上插入新 envelope、展平 worker receipts、应用 transition 与 terminal removal，校验 complete evidence 和 cursor 增量，最后返回 candidate（`receipt_aggregation.rs:7-26,38-103`）。
- Ledger transition：公开 `apply` 克隆 shadow，所有 receipt local key 去重、逐条审计/转移、aggregate conservation 验完才替换原 ledger（`ledger.rs:113-166`）。P4/P5 的 private 快路径有 enclosing tick candidate 的丢弃契约（`:209-219`、`receipt_aggregation.rs:60-68`）。
- 连续及 auction caller 都在 tick shadow 上先运行 transaction，成功才 `restore_success`，随后才累加 receipt keys、receipt journal、events；例如连续 `continuous_tick_transaction.rs:150-178`，竞价 `auction_tick_transaction.rs:126-157`。P9 再校验 receipt/event journal 与 cursor、rebase live envelopes 后才 commit（`candidate_commit.rs:126-159`）。
- 失败回滚边界：`authoritative_tick.rs:25-47` 的 phase prepare 或 `validate_precommit` 失败均在 `prepared.commit()` 前返回；候选对象在 caller 之外被丢弃，`mark_committed` 只在 commit 成功后发生（`:49`）。旧 smoke 的“rollback”结果不能替代这些当前路径，但未发现 authority 在 P9 前写入的路径。
- 恢复边界：`persistence.rs:258-322` 从 schema/policy、market集合、book序列、resting/filled identities、history market 集开始显式拒绝不匹配；存档恢复后 ledger hydration/validation 亦由当前 envelope projection 族维护，测试 caller 有 `envelope_projection_hydration_tests.rs:61`、`envelope_projection_tests.rs:70-121`。未将 D7旧 StrategyState投影等价误当成当前强制验收。

## 退役决定、旧结论与新候选

- 最新 cleanup 第 12 项优先：`docs/test-cleanup-checklist.md:85-88` 已批准删 `escrow_corpus_replay`、CorpusProjection族及旧 adapter/bundle 装配；sealed evidence 原样保留但不再有执行代码复验。`task-3-8-smoke.md` 的旧 preserver/verifier 与历史 D6/D7 不可提升为未完成产品要求，也不以缺旧测试符号报缺口。
- 旧 D6 同 tick/FIFO、D7 save common/profile 转换结论只代表旧 capture；本轮没有运行 replay 或测试。既有独立审查 `sweep34` 对“无生产遗漏”结论仍被当前代码路径支持，但其“JSON 对应 hash 与布尔字段”一句没有指出 D7两份 Markdown hash 与 raw 文件 digest 的差异，因此本报告保留上述证据口径候选。
- 产品遗漏：本轮没有确认新的现行产品契约遗漏。唯一新候选是 D7 历史证据 digest 定义不清；其状态不确定，不能扩大成现行实现缺陷。解决需解释 `structured-comparison` 两组摘要 hash 各自的输入/规范化规则，并使其与 README 明说的文件 SHA 可追溯对应；不应篡改历史文件或伪造重跑结果。

## 结论

Task 3–8 smoke 与 D6/D7 artifacts 已按全文范围审查。账本/receipt/事务提交/恢复的当前生产路径有独立现行实现和调用者，旧 corpus verifier 已按用户批准清理退役。没有新增产品漏项；D7 两种 common-field digest 口径尚待解释，是唯一证据可审计性候选。未运行测试，未修改产品或 Git。
