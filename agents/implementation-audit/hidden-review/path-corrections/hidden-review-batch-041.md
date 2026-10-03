# 批次 041 独立复核

## 阅读完整性

| 文件 | 行数 | SHA-256 | EOF |
|---|---:|---|---|
| `agents/oop-refactor-audit/exhaustive/modules/engine-session-04.md` | 145 | `0c0c42c0026789a03e6c1c8c4b1732f423755242a9138c4d0e822a6e9b1acacd` | 是，连续全文至末尾 |
| `agents/oop-refactor-audit/exhaustive/modules/engine-session-06.md` | 195 | `72b02b3d8bbbdc09ec6aeba1b5731b76af0fb2974a39e27fb8c7f078eee9a550` | 是，连续全文至末尾 |
| `agents/oop-refactor-audit/exhaustive/modules/engine-session-07.md` | 105 | `a0ab6bc3b4f4a64272fbfb917a8ee5b69926c061981c71c75a3eca0e23163b9f` | 是，连续全文至末尾 |

已读根 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`、当前 G01–G68 实现审计主账及相关 ADR。代码基线为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。本批仅审以上三篇来源；未运行测试/构建、未执行 Git 写操作、未查官方规则。

## 结论

三篇历史文档的主要结论是保留既有职责、拒绝为 OOP 而加包装。当前代码支持该判断：计划 root/typed continuation、订单母单、session/P9、CivilUpdate 发布事务及 DTO 投影均有实际 owner 和 caller。它们不是待实施 OOP 提案，也不能证明其列出的行为测试边界已全部运行或验收。

04 中 `PlanPersonalState` 的根运算现由 `InstitutionDecisionRoot` 持有并被 `roots` 生产决策路径调用；这验证了原文指出的已有迁移方向，而不是新候选。06 中 `PlanChainOperationBatch` 与 typed continuation 已被 tick 流水线实际调用；07 的协议 DTO/视图仍处于边界投影，`ProtocolSession` checkpoint 覆盖协议发布失败。未发现需要新增产品对象或本批能独立确认的实现遗漏。

## 章节族状态与当前证据

| 来源/章节族 | 当前状态 | 当前代码与 caller |
|---|---|---|
| session-04：个人状态、计划报价/urgency、披露、envelope、母单、hash、快照与时钟 | 保留结论再证。`PlanPersonalState` 在 decision root 中聚合个人字段；报价/urgency仍为 `GameSession` 私有边界；披露游标留在 `DisclosureDispatch`；envelope 是存档边界投影；母单是 session 运行态。 | `packages/engine/src/session/decision_chain/roots.rs:187-221`（root 所有个人状态并返回 typed 操作）；`packages/engine/src/session/decision_chain/quote.rs:4`、`urgency.rs:50`；`disclosures.rs:58-65,99-151`；`envelope_projection.rs:14-38`；`execution/orders.rs:11-17`。日终顺序与完整 session rollback 见 `packages/engine/src/session.rs:2073-2128`。 |
| session-04：测试支撑文件 | support 结论再证；测试 fixture/断言不是生产抽取目标。原文指出的 take/install、urgency 输出矩阵、披露中途失败等缺测提示仍是“如改边界应补”的建议，不能转写成已验证或主账产品缺陷。 | 对应测试模块仍在 `packages/engine/src/session/` 下；本批不运行它们，也不将测试名称当成验收结果。 |
| session-06：adaptive root 与候选协调、计划执行/续行/事实同步、玩家 FIFO | 当前 owner 与调用链存在；保持 typed 命令、局部依赖键和实际股票/账户受理边界。旧正文提及可按职责整理、可增加 interpreter 小对象等均为可选想法，不构成未完成动作。 | `packages/engine/src/session/plan_chain_candidates/adaptive.rs:74-122` 产出 ready route、仅按 `(AccountId, StockCode)` 建 pending；`pipeline/adaptive_plan_chain.rs:260,421,514` 连接候选/结果；`plan_execution.rs:25-60` 验证计划请求；`plan_execution/actions.rs:130-175` 物化 adoption；`plan_execution/synchronization.rs:10-79` 原子应用 pending facts 后清除；`execution/records.rs:5-117` 按真实受理/成交/撤单更新母单。 |
| session-06：`ProtocolSession`、CivilUpdate 与发布 checkpoint | 保留协议边界，明确协议 rollback 不等于市场 tick P9，也不扩大裸 `GameSession` adapter 的原子承诺。 | `packages/engine/src/session/protocol/civil/session.rs:225-280` 在帧准备失败时 rollback；`:283-330` 包住日结、事实附加、验证与 SaveSlot candidate。`GameSession::step_frame` 由 `protocol/commit.rs:7-25` 调用唯一 step 后组帧；独立日结及 session checkpoint 在 `session.rs:2073-2128`。 |
| session-07：commit/delta/facts/replay、玩家订单、快照/views、planner tests | DTO 与只读投影保留；排序/身份只用于稳定输出，不能解释为交易先后。ReplayGuard 的长期历史增长仍需按既有设计议题处理。 | `packages/engine/src/session/protocol/commit.rs:7-49` 投影已提交帧；`protocol/delta.rs:20,182` 定义差量并计算；`protocol/facts.rs:10,16` 定义事实 DTO 与构造；`protocol/replay.rs:10,16` 持有重放游标；`protocol/player_orders.rs:29` 读取订单簿；`snapshot.rs:46,126` 定义快照并生成投影；`views.rs:127` 生成价格路径观察。当前代码路径存在不代表所有建议的边界矩阵已测试。 |

## 总账、ADR 与语义边界

- G01–G68 当前主账中没有一项可由本批职责保留或对象抽取核销。G16 的历史复制/长运行成本是独立性能与所有权缺口；`PlanChainOperationBatch`/`ProtocolSession` 的存在不能证明复制已经消除。Q 总账中也未发现这三篇材料直接解决的待决项。
- ADR-0017 是 accepted 的现行 tick 契约：P0–P9 与 P9 单点提交继续有效。其 `npc → player → plan_chain` 来源序及全局密封序的历史描述，已被 ADR-0018 §7、§11.2.3 明确决定并由 ADR-0017 开头记录为取代；不得以旧 session-04/06 的叙述恢复来源优先级。
- ADR-0018 整体仍为 proposed，不得将全篇当成已接受规则。仅明确决定且已同步写入 ADR-0017 的受理顺序/收据身份修订适用；其他开放议题仍按其状态处理。ADR-0019 对数量配额、ADR-0024 对投资者资金池、ADR-0025 对日终公共存档，以及 ADR-0026 对机构个人经验的后续决定优先于旧材料中的相应历史表述。
- 本批没有新增交易规则。存量 A 股单位/撮合/资源语义依正式规则及现行 ADR；`seq`、generation identity、稳定输出顺序不是跨实体撮合优先级。未执行官方规则核查，故不宣称重新认证交易制度。

## 候选状态

无新的必要 OOP 候选。原文中窄方法归整、`PlanQuoteInputs`、`PlanExecutionInterpreter`、checkpoint 字段分组等属于可选内部整理；代码证据未显示第二权威或缺少真实 caller，故不升级为产品缺陷。原文建议但未直接覆盖的边界测试在后续改动相关行为时仍应复核；本批不据此改写断言或主账。

未运行测试、构建或官方规则查询；没有产品代码改动。本结论仅是对指定历史材料、当前结构/caller 和总账适用性的静态复核。
