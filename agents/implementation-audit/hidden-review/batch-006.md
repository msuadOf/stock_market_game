# 批次 006 独立复核

## 阅读完整性

| 文件 | 实读行数 | SHA-256 | EOF |
|---|---:|---|---|
| `agents/oop-refactor-audit/challenge-2026-10-03/hosts/report.md` | 1486 | `2bbffb971f1fed5fb1ddd7bda7aaeb00c3d4c38d7ce0ae0ea22693cd91ee4c14` | 是，连续分段至末尾“重新冻结”段 |
| `agents/oop-refactor-audit/challenge-2026-10-03/hosts/review.md` | 103 | `c7a6068b628a988042b362b7f2dcc7f99c7b89c181e78fc3319bccf21df32678` | 是，至最终签名结论 |
| `agents/oop-refactor-audit/challenge-2026-10-03/pipeline/reader-instructions.md` | 17 | `448d42a9a7612dd730c69ad7632ff3be22ede1ffb7473a242df5d01fee305723` | 是，至 JSON/完成约定 |

已读根 `AGENTS.md`、`docs/principles.md`、ADR-0017 与 ADR-0018，并核对实现审计主账及 G/Q 对照。以只读 `git rev-parse` 确认当前审查树为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`；未执行 Git 写操作、测试、构建或官方规则查询。

## 结论

这三篇文档作为“第二轮调查及其当时独立复核”的历史记录内部一致：report 的候选处置和 review 的修订记录相互对应，reader 指令说明的是 pipeline 旧批次流程。review 明确把通过限定为调查门禁，也明确称未实施产品变更、未运行测试，结论范围诚实。

但以当前指定 HEAD 为基线，report 列出的 19 个 `new` 和 1 个 `extension` 已全部在代码中找到对应实现；将这些项目继续登记为待实施或当前抽取机会会过时。此判断只确认对应结构/caller 已存在，不表示所有候选实现通过测试，也不表示相邻产品缺口已修复。

## 候选状态

| 当前状态 | 候选 |
|---|---|
| 已在 `43b1aa5` 对应 | `hosts-R2-N01` `AnnouncementExposureFixture`（exposure.rs:92）；`N02` `AuctionFixture`（auction.rs:8）；`N04` `BehaviorScenario`（behavior.rs:79）；`N05` `SpringFestivalScenario`（civil_clock.rs:86）；`N06` `FundamentalBeliefCase`（fundamental_beliefs/main.rs:113）；`N08` `CorrectionScenario`（correction_restatement.rs:28）；`N10` `PlanScenario`（plans.rs:51）；`N11` `Base::valid_q1/publication_request`（publications/failures/mod.rs:40,86）；`N12` `TestOrderSaveFixture`（session.rs:93）；`N13` `ActorHarness`（actor.rs:691）；`N14` `WsPublisherConnection`（routes.rs:1076）；`N15` `ApiTestSession`（api_contract.rs:181）；`N16` `ServerFixture`（ws.rs:51）；`E01` `CaptureArtifact`（runtime.rs:135）；`N17` 路由 record 的 `preflightChecks`/`describeChecks`（build-matrix.mjs:58,73）；`N18` `MarketUiReportRun`（market-ui-report.mjs:234）；`N19` `ArtifactInventory`（run-full-regression.mjs:179）；`N20` `DiagnosticAuditRepositoryFixture`（audit-diagnostic-divergence.test.mjs:45）；`N21` `PerformanceComparisonRun`（escrow-performance-harness.mjs:481）；`N22` `Resource`/`EnvelopeConservation`（escrow-verification-contracts.mjs:241,285）。 |
| 既有结论维持 | 原表中的 `covered` 项只并入既有动作；`rejected` 项仍不能因当前测试夹具普遍对象化而自动恢复。`N03/N07/N09` 的拒绝理由需继续按具体 owner 与行为判断，当前代码没有证据推翻该拒绝。 |

“已实现”的核对限于结构、核心方法和调用点搜索；候选提案中的所有字段、失败时序和边界条件未逐一动态验证，不能给出实现验收结论。

## 章节族与当前代码

- 测试场景族 `N01/N02/N04/N05/N06/N08/N10/N11/N12/N13/N15`：对象位于对应 test crate/file，调用点实际消费对象；未发现其被接入产品领域状态 owner 的漂移。测试夹具仍不能掩盖 A 股规则断言或损坏存档负例。
- 服务与协议族 `N14/N16`：`WsPublisherConnection` 由 `run_ws` 每连接创建（routes.rs:1307），集成测试位于同模块附近；`ServerFixture` 持有测试 server 生命周期。`WsPublisherConnection` 当前代码保持 failure、generation、resync barrier 与 delivery mode 状态，但旧提案的状态转移/异步行为保证仍应以当前源码为准。
- 工具/证据族 `E01/N17/N18/N19/N20/N21/N22`：代码分别有 capture 值与 writer、共享 preflight record、资源会话、inventory seal、临时仓库 fixture、性能 comparison run、逐 envelope conservation owner。均属 harness/工具/测试层，不构成引擎交易状态或用户存档 owner。

## 旧结论再证与反证

- 再证：ADR-0018 明确取代 ADR-0017 中全局来源顺序等旧约定；report 把对象提取限定为不改变协议/收据/交易行为是正确边界。大 A 语义不因测试 fixture 或验证器对象化而变更。
- 再证：`WsPublisherConnection` 是真实连接级状态 owner，`run_ws` 仍承担 socket I/O；`ArtifactInventory` 以原解码记录计算 digest 并保留 JSON 表达边界；`Resource` 使用 `BigInt`，`EnvelopeConservation` 保持逐 envelope 收据方程。这些边界与 report/review 的谨慎限定相符。
- 新增反证/交叉登记：`N18` 已抽取为 `MarketUiReportRun`，但不能把它误报为实现审计 G61 已解决。其 `close()`（market-ui-report.mjs:304 起）仍按顺序 await 清理；某一步拒绝会阻断后续步骤，且没有可见的整次工具进程外 deadline。实现审计 G61（implementation-audit-2026-10-02.md:129）仍是独立待核项。对象存在不是资源清理修复。
- 新增反证/交叉登记：`N19` 的 `ArtifactInventory` 已有对象实现，但这不核销 G62/G63（实现审计主账:130-131），后两者分别是进程树外部 deadline 与普通 Rust case 10 秒硬期限，inventory 只负责制品身份封存。
- 交易语义：`N22` 验证器只检查证据收据；当前脚本代码的单位仍为现金分、股份股，不能将它描述成生产 `Money/Qty` 或撮合/结算权威。report/review 此项边界可靠。未重新查交易所规则，故无任何现行规则变更结论。

## 尚需注意

report 与 review 签名绑定旧资料的 SHA，不绑定当前 43b1aa5 源码；文件内容本身没有伪称 HEAD 是该签名的一部分。若将其用于当前执行计划，应先把全部 20 个动作标记为已落地并复核实际 diff/提交来源，再按 G01–G68/Q 的状态独立管理尚未解决的产品和工具问题。当前静态发现没有要求恢复被拒候选，也没有发现需要新增 OOP 抽取动作的证据。
