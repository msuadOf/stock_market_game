# luna50：两份删除文档全文与当前调用链独立复核

## 原档、基线与完整性

- 基线提交 `08e4fc75b52a71a3262a8a938c57b44f8b5b4960` 存在；两份指定历史版本也分别解析为提交 `84aca54f3e4d11aaf4a788766ec419acfaa266f8`、`e0b348d56d98f23932e13102633ce49b84ed1433`。读取对象是指定提交的 parent 中 Git blob，不是工作树同名文件。
- `scripts/simulation/escrow-corpus.md`：完整连续读取至 EOF，190 行、11042 字节，SHA-256 `7f1a754a29f3e3723b882c2b0c1bfae4a9cc34a63df1902a10b6d6094b594d54`。
- `packages/engine/tests/preserved-test-inventory.md`：完整连续读取至 EOF，119 行、10401 字节，SHA-256 `c047cf3d3596db0c560c8a1d1676e632b6105844239f33fdba558559043236a2`。
- 合计连续核对 309 行、21443 字节。全文的章节覆盖和按原顺序条款矩阵如下；没有按搜索命中片段代替全文审查。

## 后续决定与优先级

`docs/test-cleanup-checklist.md:81-91` 是更新且已独立复核的退役决定，优先于两份旧文档：用户确认全清旧版兼容与 sealed-corpus 适配栈；Rust corpus replay/projection、JS adapter/exact comparator、旧证据 bundle 组装已删除；`.omo/evidence/` 历史密封文件保留，但接受不再能用可执行代码复验。它也明确校准了 validator 的真实 caller：矩阵运行时直接使用 `verifyConservationSnapshot`；`validateObservation`、`verifyDeterminismMatrix`、`verifyPerturbationGate`、`artifactReceipt` 现在仅由测试调用；`validatePerformanceReport` 仅由 matrix 测试调用。这些 helper 的测试专用状态已被明确接受，不能作为新候选重提。

领域语义以现行 ADR 为准。ADR-0017 的开头修订（`docs/decisions/0017-escrow-parallel-tick.md:7-18`）明确废止来源类别全序、ReceiptLocalKey 跨委托排序及不同 worker 数整局字节相等要求；这些输出顺序不再是交易先后。旧 #4 测试要求与 ADR-0018 的后续模型也须核对，不能按旧文字复活已退役语义。

## `escrow-corpus.md` 逐章复核

| 原文范围与条款 | 旧结论复核及当前证据 | 核销判断 |
|---|---|---|
| 标题及首段：Task 9 comparator 是工具而非第二引擎；金额为 cents、数量为 shares；规定负门禁和临时目录 | comparator 命令及其 Task 9 接口按 cleanup §12 退役。整数金额/股份单位仍是领域概念；当前 matrix 临时目录契约属于现存工具自己的边界，不依赖旧 comparator | 旧 comparator 退役；领域单位未退役，不构成产品缺口 |
| `Sealed input and provenance` 前半：active root、manifest/run hash、sealed status、路径/链接/重复身份/不安全整数拒绝、ADAPTED 非 PASS | 这些是 `loadSealedCorpusRun` / `adaptLegacyStream` 的旧安全与适配契约；cleanup §12 删除 adapter/corpus 文件，密封证据仍留存但不再可执行复验 | 已明确接受的历史可复验能力损失；不要求重建旧 parser |
| 同章中段：updates/state/provenance、seq 与 policy 元数据处理、seller fee projection 见证、timeseries 归一化 | 属于旧格式投影契约。当前不再有旧 stream 到 CORPUS_SCHEMA 的生产 caller。不能把 `corpus_projection` 固定 null 误判为缺实现：cleanup:85 明确要求保留 JSON 键并置空 | 按退役决议核销；null 键与现行 JSON 契约一致 |
| 同章 `prepare-current` / replay / `diagnose-current`：新会话重放密封输入，产生捕获但不将 capture match 当验收 PASS | `escrow_corpus_replay` 与相关 Rust 投影族已按 cleanup:85 删除。现存 verification harness 继续从生产入口捕获当前运行；它不再声称对不可再生 sealed legacy input 作旧新语义对照 | replay adapter 的未替代能力已由用户接受；当前运行捕获仍存在 |
| 同章 `assembleLegacyProjection` 与受控卖方 surface：零现金 normal seller 不能由 funded run 冒名；rollover、跨 tick partial fill、live remainder、checkpoint 和费用方程 | surface evidence extractor 属旧 exact comparison 全栈。当前真实会话测试与 escrow settlement/conservation 测试仍覆盖现行交易行为，但不是旧密封样本的映射复验。不可声称历史 witness 已重新 PASS | 旧跨版本证据退役；现行行为测试继续承担当前契约 |
| 同章 equivalence 四项：buyer fee aggregate、T+1/self-trade、price cage P4 rejection order ID/cursor、instant full fill identity | `extract-equivalence` 与旧 ID 精确映射随 adapter 退役。现行 ADR 和订单/收据实现仍约束 P4 拒绝、T+1、成交身份等；全局稳定展示身份不得反向成为交易优先级（ADR-0017:7-18） | 不能要求旧编号映射重建；现行受理/身份规则仍有效 |
| 同章 divergence #9：acceptance flip、three-leg fee catchup 与独立 seller 账 | 旧 sealed witness 及 compare mapping 退役。当前 `verifyConservationSnapshot` 继续检查 receipt、envelope 和账户资源守恒；现行费用增量测试覆盖具体结算行为 | 历史 #9 证据不可复验已接受；守恒和费用行为仍有当前覆盖 |
| `Exact comparison`：逐 leaf mapping、每一变化恰一条审核记录、cash equation 白名单、RNG/plan/state 相等 | 整个旧 exact comparator 已移除。不能将其要求迁移成不同 worker 或无关实体全局字节一致；ADR-0017:15-18 已明确不要求。仍需检验局部因果、真实争用规则、守恒和失败原子性 | 旧差分门禁退役；若发现当前矩阵错误，须按现行 G39 等记录单独追踪，不能算作旧文档缺口 |
| 末章：正负对、删除/交换/新增 payload negative gate、CLI PASS 双门禁、stress 走新引擎校验 | corpus negative gate 与 CLI 已删。当前 matrix 实际调用 `verifyConservationSnapshot`（`run-escrow-verification-matrix.mjs:15,484-491`）；validator 检查每个 tick 连续、标识一致的快照。其他 KEEP helper 仅由测试调用是 cleanup:87 已知且批准的边界 | 旧 negative gate 退役；保留的 current-run 守恒检查已接到真实 caller |

## `preserved-test-inventory.md` 逐章复核

| 原文范围与条款 | 旧结论复核及当前证据 | 核销判断 |
|---|---|---|
| `(a) 未改动保护`：sealed attempt-12 + overlay 冻结未改动测试源码字节 | 这是旧的特定提交/hash verifier 契约，不是测试源码永久接口。该 verifier/inventory 机制随用户确认的清理范围退役。测试本身仍按当前业务语义维护 | 字节冻结门禁退役；不应因旧 hash 不匹配而恢复旧实现 |
| `(b) 冻结 #9，严格四项`：卖方费用预留、现金不足 acceptance flip、可能小额 partial fill、同账户 reservation budget；排除 plan soft budget | 这些四项是旧手续费/现金预留证据契约。当前测试改按新语义验证：卖单只预留股份、不占用现金；不能把个人 plan soft-budget 的 allocated/available cash 误当订单 escrow。cleanup:88 还确认 schema v1 拒绝仍由 `persistence/v2_tests.rs` 直接覆盖 | 旧的四项源码冻结退役；当前交易/存档规则仍需按新测试与生产代码判定 |
| `(d) PF1 fixture reduction`：hunk/hash 严控，只缩 fixture，不动 assertions；快慢例保持长验收 | 原文里“不得删改业务断言”与旧源码 hash 的门禁不再是当前 verifier；fixture 缩减范围、必要的完整跨日链仍在现行测试。`company_scenarios/main.rs:147-169` 明确核对阶段、股票类别、NPC 覆盖；`run-full-regression.mjs` 的 required-long validation 负责执行 lifecycle 场景（其历史运行记录不由本审计重跑） | 旧 PF1 机器 hash 退役；代表性业务覆盖与长验收要求未因此删除 |
| `(c) 精确 Todo 2 合同改写`：`step/save -> Result`、seeded holding 不伪造失败、双侧 filled_qty | 精确 symbol/hash allowlist 退役。行为级替代仍可见：`experience.rs:93`、`experience_feedback/main.rs:250` 锁定 seeded holding 不产生买入失败经历；现行 save/restore partial-fill 场景继续比较恢复前后执行。不得把旧符号名缺失当功能缺失 | 机器 hash 门禁退役；重要经历和恢复不变量仍由当前测试表达 |
| 已批准交易分歧迁移，#4：开盘首三分之一撤单与同 tick place/cancel | 原文要求同 tick 新建订单拒绝已过时。当前 `auction.rs:1058-1104` 直接验证同 tick 接受后撤销、订单从存档移除、预留归零；跨 tick 首三分之一撤单仍由 `auction.rs:995-1055` 覆盖。与现行 ADR-0017/0018 阶段和撤单语义一致 | 旧 #4 断言已由后续交易模型取代；当前合法撤单与资源释放行为均有覆盖 |
| 同节 Web 默认 auction fixture：缩短到可测规模但保留 NPC、交易日、auction 活动 | 当前 engine 测试的精简 fixture 仍检查 opening cancel window、continuous matching、五类证券及三类 NPC 数量（`company_scenarios/main.rs:147-169`）。不把这个 engine case 当浏览器/E2E 覆盖 | 缩短 fixture 不等于删语义；此处未发现交易单位或板块差异被静默抹平 |
| 分歧 #6/#7：1429 event、720 tick/FIFO、规范 hash、存档 v2 精确字段变换 | 这些是当时的双侧比较证据，保留历史意义但不再是持续冻结门禁。ADR-0017:15-18 明确不要求不同 worker 下无关账户/股票整局字节相同。SaveSlot v2 和现行恢复测试仍按当前 schema 验证，允许编辑存档与派生状态重建以现行 persistence ADR 为准 | 不恢复全局旧 hash 标准；当前存档契约与局部因果测试仍适用 |
| 末节 `step_skeleton` 三摘要及 machine source hash | 这是旧 machine inventory 的 characterization/hash 冻结，不等于现行公开 API 或产品行为。现行 tick、日界、行情与失败原子性由具体 Session/pipeline 测试覆盖 | 旧摘要门禁退役，无需重建 step_skeleton 入口 |

## 旧候选、反证与新候选

- 复核既有 `sweep50` 的主要结论：sealed replay/exact compare/历史 source hash 不应因文档曾标“required”而重开；cleanup §12 是明确的后续用户决定，且准确承认历史密封证据不再能被代码复验。
- 复核其“KEEP helper 未接 matrix 不是新遗漏”结论：`verifyConservationSnapshot` 是真实矩阵调用；其余格式校验 helper、`validatePerformanceReport` 的测试专用调用均已在 cleanup:87 明确登记并批准。没有证据支持将这些旧 comparison helper 恢复为生产 caller。
- 旧 inventory 的 #4 预期不应恢复。当前同 tick auction place/cancel 测试与跨 tick 撤单测试一起保留，原报告的判断方向正确；交易时段与卖方现金/股份单位没有从证据中漂移。
- 重新审视 `sweep50` 提出的“自由推进双实例逐 tick event/save byte equality”候选：`company_scenarios/restore.rs:4-42` 中两实例是一份明确的 save/restore twin，同初态、同输入逐 tick 继续，比较用于检验恢复是否改变后续执行；这与 ADR-0017 所禁止的不同 worker 调度、无关实体跨运行全局排序承诺不是同一主张。未发现证据足以把该用例登记为缺陷或要求删除。该候选撤回，不提出新产品缺口。
- 未发现被最新清理决定遗漏、又必须由当前工具/生产 caller 承担的文档条款。历史跨版本密封样本不能再复验是已接受限制，报告中应明确其限制，不能宣称旧语料仍可 PASS。

## 审计限制

本次为静态历史/当前源码复核；未运行测试、编译、性能矩阵或 Git 写操作，也未访问网络/交易所规则。本报告不重新证明历史验证记录的通过结果，不推断 `.omo/evidence` 的内容当前可执行复核。交易语义只核对相关已接受 ADR 与现行测试边界，没有引入新的 A 股规则。
