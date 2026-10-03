# 隐藏扫描批次 030

- 基线：`43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。审阅 caller：`.worktree/implementation-reaudit`。只写本批记录；未改产品代码、未作 Git 写操作、未运行测试或构建。
- 规则：读取根 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`；并核对 ADR-0015、0017、0018、0019、0020、0025。ADR-0017/0018 的当前契约保持：计划依赖只由真实 typed outcome 延续、资源约束及权威提交仍归会话状态所有者；OOP 结论不改变撮合/结算语义。ADR-0020 仍为 proposed，不据此宣称已接受方案。

## 来源完整性

| 来源 | 行数 / SHA-256 | 阅读状态 | 章节状态 |
|---|---|---|---|
| `agents/oop-refactor-audit/completeness-2026-10-03/session/parts/29.md` | 11 / `a5c7f712164a7ec90d8fb7f2cdd6fb48ef0884a945e87badfc125ab889b6ce10` | 连续读取至 EOF | engine-session-05；测试 fixture / restore 边界；原判断称无新增动作。 |
| `agents/oop-refactor-audit/completeness-2026-10-03/session/parts/30.md` | 7 / `931172892870f64fbd0ee4a4f19a4bfb19ad0e8d1d7e256ee73ab2c42c75c780` | 连续读取至 EOF | engine-session-06；PlanChain owner 与条件性续行上下文；原判断称无新增动作。 |
| `agents/oop-refactor-audit/completeness-2026-10-03/session/parts/31.md` | 65 / `b1a57e3a736a9bc7cae4399cb0f0612366ed45a8e2ddbea976ec77f5170f840f` | 连续读取至 EOF | engine-session-06；协议值、玩家候选转移、civil adapter；原动作已涵盖一项 adapter 原子性边界。 |

三个历史记录文件本身的哈希和实测行数与 scan plan 相符。**但其所引用的源文件证据并非都对应基线 43b1aa5**：例如 part29 称 `v2_tests.rs` 为 1273 行、来源 SHA `8bd7e238…`，基线文件实测 1375 行、SHA `2f84d835…`；part31 的 `player_candidates.rs`、`player_candidates_tests.rs`、`civil/mod.rs`、`publication_tests.rs` 源 SHA 亦与基线不符；part30 JSON 所列的 adaptive、测试、plan execution、actions 等源 SHA 多数不符。仅 `plan_execution/types.rs`、`commands.rs`、`protocol.rs`、`boundary.rs` 等抽样记录与基线哈希一致。故不能把历史逐文件哈希/全文阅读声明直接当成当前基线证据；下文按 caller 基线重新检查关键 owner / 调用边界。

## 当前实现复核

- `v2_tests.rs` 当前 1375 行；文件顶层为测试模块，fixture 与断言不拥有生产状态。读档职责仍落在 `GameSession::restore`、基础 SaveSlot 校验和 runtime v2 校验。原 support 分类有现状支撑；fixture 是否适合做普通测试整理不构成 OOP 缺口。`packages/engine/src/session/persistence/v2_tests.rs:1`。
- 当前 `PlanChainOperationBatch` 与 part30 描述的具体字段结构不同：基线字段为 roots、operations、candidate source、routes、reports；`StockRouteCoordination` 收纳 pending、reconsideration、retry_market。其 pending route 以 `(AccountId, StockCode)` 与 generation 关联。`PlanExecutionRoute` 持单命令 continuation，并把 typed outcome 交回 `GameSession::resume_plan_execution`。这支持“一个计划链协调 owner + 条件性续行”的职责结论，但历史 JSON 的具体字段名及其 source hash 已过时。`packages/engine/src/session/plan_chain_candidates.rs:90-96,105-125`、`packages/engine/src/session/plan_execution/interpreter.rs:29-65`。
- 当前玩家候选捕获仍由 `GameSession::capture_player_candidate_batch` 对 pending queue 做一次性转移；实际消费者是 `ReadyIngress::capture_sources`，随后组合 NPC 与 player 候选。批次 DTO 没有订单路由权威，保留现有分类合理。`packages/engine/src/session/player_candidates.rs:3-11`、`packages/engine/src/session/pipeline/ready_ingress.rs:33-48`。
- 当前 `GameSession::end_civil_day_update` 校验完整日内帧后调用 `end_civil_day`，再生成事实和 refresh；`ProtocolSession::end_civil_day_update` 在其唯一生产调用外围建立 checkpoint，失败 rollback，成功后再建立 day-end save candidate。与旧记录相同，后置步骤可能失败；但基线源码显示 adapter 是私有方法，只有该 `ProtocolSession` 路径调用，不能沿用旧 finding 中“存在直接测试/内部裸调用路径”的说法。若只按可达公开契约衡量，外层 checkpoint 已覆盖完整流程，不构成新增 OOP owner；若未来改变可见性/引入另一 caller，需重新审计失败原子性。`packages/engine/src/session/protocol/civil/mod.rs:164-234`、`packages/engine/src/session/protocol/civil/session.rs:283-329`。
- Protocol DTO 与校验仍是边界职责：`TickFrame` / `TickBatch` 校验序列和帧组合；`CivilBoundary` 校验日界值。测试-only 的 publication 与 candidate 测试不应迁入生产对象。未发现基线实现违背现行 ADR 或改变沪深 A 股交易语义。

## G/Q、分类与反证

- 五类结论：**原 owner/retain 获当前代码支持**（PlanChain、协议 DTO/校验、玩家队列转移）；**support 成立**（持久化及协议测试 fixture）；**部分历史证据已过时**（源哈希/行数不符，PlanChain 字段结构有变）；**原动作已涵盖但调用描述需更正**（civil 更新的 checkpoint 边界，当前只有 ProtocolSession 生产调用）；**新增 OOP 候选**：无。
- 对照当前 G01–G68/Q 总账及 implementation audit：本批 OOP 类型/状态归属结论不能核销任何 G 或 Q，也没有证据改变其既有 disposition。G27 在总账中已核销，其余开放项仍按总账状态；本批不重判各条产品行为。尤其不可从 ProtocolSession 有 checkpoint 推断宿主发布、日终持久化或其他跨层 gap 已关闭。
- 反证候选：对“计划续行上下文应有独立 drain owner”的反证是当前 `StockRouteCoordination` 只在特定 typed route outcome 消费 pending，终态不需 retry 时随 batch 生命周期结束，且没有被丢弃的权威市场状态；对“裸 GameSession adapter 有独立原子性缺陷”的反证是当前其私有可达性与 ProtocolSession 唯一生产调用链，checkpoint/rollback 包围后置验证与 candidate 构造。若以后出现新 caller 或源码可见性变化，此反证失效，需重看。

## 结论

状态：`complete_with_stale_source_evidence`。三份指定说明均完整到 EOF 且自身 hash/行数通过；历史源文件哈希与基线存在多个不一致，故原审计源码证据不能整体沿用。依据当前代码重新核对后，原主要 owner / support 分类成立，无新增 OOP 动作；civil finding 的“直接裸调用路径”表述应视为被当前调用图反证。静态审阅，不代表测试通过、全覆盖或完整交易规则审查。
