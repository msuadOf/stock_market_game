# luna08：ADR-0015/0017/0019 实现复核

- 基线：`08e4fc7`（工作树 `/data1/baiyifan/workplace/stock_market_game/.worktree/implementation-reaudit`）。
- 阅读范围：连续 EOF 通读 ADR-0015（47 行）、ADR-0017（157 行）、ADR-0019（60 行）；已读根 `AGENTS.md` 与 `docs/principles.md`。以下行号均为该基线当前行号。
- 边界：静态追踪生产实现、已有测试定义和旧审计结论；未运行测试，未改产品代码或 Git 状态。

## 章节覆盖矩阵

| 文档原文范围 | 章节 | 实现追踪与判断 |
| --- | --- | --- |
| ADR-0015:1–10 | 问题、母单成为权威会话状态 | `packages/engine/src/session/plan_execution.rs:41` 起读取 `parent_orders`；`packages/engine/src/session/pipeline/continuous_lifecycle_projection.rs` 与 `adaptive_plan_chain.rs` 承接执行事实。检查范围内母单语义确已保存在 session/save 并经真实订单生命周期推进；未见把目标量直接算成交。 |
| ADR-0015:11–22 | 拆单、修订/反向替换、订单路由校验、边界 | `session/pipeline/continuous_tick_transaction.rs:188–233` 汇入候选与计划根；`ready_stock_stream.rs:69–84` 将已完成股票的 typed 结果交回计划链；订单校验仍由 `AccountValidatorDriver` 和股票簿分层执行。ADR-0015:35–47 后续已把 ValueStrategy 专属范围修订为 TradingPlan 执行子状态，故不能要求机构专属策略仍存在。 |
| ADR-0015:23–34 | 已验证、后续验收 | `continuous_tick_transaction_tests.rs:515–585` 覆盖存档恢复继续执行；:1534 起有 ID 边界回滚；:1662 起核对母单活跃子单。测试源存在不等于本轮运行通过。多 seed 校准、参与率/冲击仍属未来验收，不从代码静态存在推为完成。 |
| ADR-0017:1–19 | 状态、修订说明 | 2026-09-24/25 三项修订均视为生效。不得恢复旧 `npc → player → plan_chain` 来源优先、跨委托 ReceiptLocalKey 大小顺序优先，也不得要求 worker 数不同的整局产物逐字相等。 |
| ADR-0017:20–27 | 上下文 | P0/P1截点、影子隔离和失败边界仍是实现检查准绳；envelope 是游戏内资源/审计模型，不能称交易所清算。 |
| ADR-0017:28–52 | 阶段和 P0–P9 决策 | 核心路径见下文“阶段链”。P0 到期与 P1 snapshot、P2 根发现、P3 增量校验、P4 增量股票任务、统一收尾、收据/结算/投影、P8 校验与 P9 单点 swap 均有生产调用链。旧来源序及跨 worker 相等要求明确作废。 |
| ADR-0017:53–64 | 数据流、envelope/费用守恒 | envelope ledger、receipt local identity、账本审计与 settlement 分层仍在 pipeline 模块；价格改善释放、卖方封顶费用须以既定 nominal/charged 区分审阅，不把 `deliver_cash` 当 escrow。 |
| ADR-0017:65–114 | Event 表、CivilUpdate 屏障、输出身份 | Event key 用作身份与输出整理而非因果顺序；CivilUpdate 仍独立于 market tick，Protocol restore 另有日级入口约束。来源序不是交易优先级。 |
| ADR-0017:115–120 | fatal、poison、失败原子性 | `candidate_commit.rs:45–99` 先完成 fallible 检查再取得 commit token，:96–100 单点调用 `commit_tick_shadow`；authority 不接受阶段部分写入。业务拒单仍不是 fatal。 |
| ADR-0017:121–136 | 九条分歧 | #1/#4按后续 ADR-0018 局部冲突及真实撤单语义审视；#2–3、#5–9仍按固定截点、ID消耗、typed业务拒绝、seq非因果、v2恢复、P3/P4校验分层与卖单费用裁定核对。具体旧结论见后文。 |
| ADR-0017:137–157 | 备选、后果、关联 | 不据“单一并行实现”推导无死锁/性能已验收；也不额外要求被ADR-0019删掉的任意挂单/请求数配额。 |
| ADR-0019:1–12 | 草稿范围背景 | 当前基线为单局优化，不承担多局容量验收。 |
| ADR-0019:13–30 | 不设任意配额、合法负载资源边界 | 全仓调查未发现所列32/5000/50000门槛作为运行/存档订单配额的证据；保留单笔类别申报量、资金/股份/时段等业务规则与传输字节/深度限制。这些不是同义配额。静态 grep 不能证明任何合法负载机器都能承载。 |
| ADR-0019:31–46 | 最小事实存档、编辑器与待决冲突 | `GameSession::restore` 从 save 重建账户、盘口等并验证引用；`GameSession::restore` 的入口在 `session.rs:2716`。执行中必要身份/排序/T+1/费用累计仍存。编辑资金导致活动买单资金不足等整理政策明确未决，不报告为已支持静默自动撤单。 |
| ADR-0019:47–60 | 单局、测试取舍 | 不因 server 固定两局推导本轮多局容量测试；保留单局恢复/隔离规则。删旧容量拒绝测试的政策已明示，不以“缺少逐常量测试矩阵”报缺。 |

## 阶段链复核

- **P0/P1：** `quote_expiry.rs:89–116` 只允许一次 P0，`apply_quote_expiry` 在 :119–207 先 hydrate ledger、按生命周期过期释放并产出真实 cancel/receipt；`:125–127` 非连续阶段不释放。`decision_resources.rs:92–108,110–205` 从当前（P0 后）envelope 汇总 reserved cash/shares 并并行遍历账户校验/构造资源快照。没有发现重复把 P0 release 加到账户预算的迹象。卖单 cash escrow 为 0 的账本语义仍需按 ADR #9 理解，不能把卖出所得加进当 tick 可用现金。
- **P2：** `ready_ingress.rs:34–59,72–90` 异步已就绪的 NPC/player 输入与计划根发现汇合；其注释明示来源标签不用于账户优先。`AccountReceipt` 仅供同账户 cash/shares lane 冲突排序（`local_admission.rs:16–58,98–175`）；计划前驱是显式依赖 `:178–219`。计划根按固定遍历发现不等于全局交易优先。
- **P3/P4 与 typed 回写：** `continuous_tick_transaction.rs:188–233` 捕获共用 post-P0 输入并启动股票 shard；`ReadyStockStream::continuous_progress` 在股票完成后按 candidate identity 取 outcomes、校验 execution round，再调 `advance_after_typed_outcomes`（`ready_stock_stream.rs:67–84`）。无关股票的完成通知走共享唤醒入口 `stock_stream.rs:29–45`，工作完成可增量回传。未发现靠 Event 数组排序推测命令成功或从新账户余额触发二次决策。
- **ID、回滚、一次收尾：** P3/P4各自拒绝 ID 规则和跨轮索引集中在 validator；transaction 末尾 `continuous_tick_transaction.rs:236–272` 执行完整流排空、finish/finalize 后才设 candidate `next_order_id`。`candidate_commit.rs:126–159` 检查 receipt keys、事件键、账本 journal/evidence 并准备 commit，`PreparedCandidateCommit::commit` 才提交。整 tick failure 丢弃 shadow；ID 边界已有单测定义但未运行。
- **价时与资金：** `local_admission.rs` 对同股票 gate 登记实际受理/依赖，而不以账户号或来源类排序；同股簿处理仍由 `IncrementalContinuousStockCoordinator` 负责。账户资源预算使用持续 `AccountValidatorDriver`，P1不重建。独立股票并行完成顺序本身不应被提升成跨股交易优先约束。
- **ReceiptAggregation/Settlement/Projection：** 全链在 stream/finish 后一次聚合结算；receipt keys 按合法身份检查唯一、全局 cursor 连续，不能将 receipt index 当跨委托业务顺序。P8/P9在上述 commit 准备器中完成。限于静态审计，未重算每一费用算式或动态验证所有资源守恒 fixture。
- **日界与 CivilUpdate：** market tick 的 day-end finalizer 在股票/计划续行排空后经 `finish_continuous_shards` 完成（`stock_stream.rs:109–164`）并只进当前候选一次；自然日日结独走 `end_civil_day` / `end_civil_day_update`，不伪装市场 tick。`ProtocolSession::restore` (`protocol/civil/session.rs:100–140`) 限定公共完整日终存档且排除活动日内委托/母单/待受理输入；verification checkpoint 是明确独立入口 `:147–151`。
- **restore caller：** Server actor restore 调 `ProtocolSession::restore`（`apps/server/src/actor.rs:1358–1372`），Desktop actor restore 同样调用（`apps/desktop/src-tauri/src/actor.rs:1139–1155`）；公共 CivilSession 在 `civil/session.rs:100–140` 校验日级契约。Server route 另先做 bounded decoder（`apps/server/src/routes.rs:838`）。`GameSession::restore` 深重建并不等价于宿主公共日终恢复许可。没有把 verification-only restore 混作生产 caller。

## 旧结论复核

- **G16：维持“仍缺”但界定为局部所有权/复制目标。** `RootReadContext` 明确包含整个 `PlanBook`（`session/decision_chain/roots.rs:5–14`），`capture` 每批仍执行 `session.state.plans.clone()`（:17–29）。从旧入口移至 root capture、同批用 Arc 共享该份 snapshot 不能消除每轮完整 PlanBook clone；ADR-0018 未接受的大范围 COW/历史版本根方案也不能反向要求实现。本结论不主张可以删历史、无测量地断言性能损失大小或要求整套重构。
- **G38：维持“生产分类未接入”，范围仅同账户预算排序。** `allocation.rs:115–126` 明确 `ExistingPlan` 排在 `NewOpportunity` 前；生产计划请求创建点 `decision_chain.rs:968–979` 和 `:1232–1243` 都写 `ExistingPlan`，未看到新机会分类从生产信号传入。因此现有预算分配器不能区分计划续行和新机会。不得把这扩为不同账户/股票撮合的来源优先；不把 `AllocationExperience::default()` 单独列缺，因为其信心/失败影响已上游消费，避免重复扣减。
- **G39：旧审计存在证据边界错误，现行 K7 契约需修订，而非运行产物整局相等。** ADR-0017:8–14已解除无关事件、收据、存档/哈希逐字对等。当前 `escrow-verification-contracts.mjs:130–160,165–189` 仍要求预算/扰动 artifact 多重集字节相同；`run-escrow-verification-matrix.mjs:583–595` 对通过项跨 entry 仍比较 artifact hash vector。这既过度约束自由实际受理时序，也不是满足局部冲突规则的证据。应修 runner/契约按实际局部受理事实校验守恒、价时、同实体因果/依赖、失败原子性；当前不运行矩阵，也不由此判定真实 engine 已违反交易规则。`

## 排除的假阳性与新增候选

- **排除任意容量候选：** ADR-0019:13–30 删除任意挂单/请求上限，但明确不保证任意负载；512MiB存档字节额度、嵌套深度、A股类别单笔申报量、资源不足拒单不是“偷偷保留 50000 条挂单配额”的证据。
- **排除排序候选：** `ReadyIngress` 初始向量包含 NPC 后玩家的构造次序，不能只凭 concat 认定 NPC 来源优先；真正可比较的 `AccountReceipt`/resource lanes 和 stock gate 看 `local_admission.rs:61–80,159–175,223+`，按同账户实际 lane receipt 及同股 gate/明确 predecessor 处理。固定容器顺序/输出 key 不证明跨实体交易优先。动态竞争轨迹仍需测试验证。
- **排除恢复缺口候选：** `ProtocolSession::restore` 对公共日终有更窄限制是有意契约，verification checkpoint 和内部 `GameSession::restore` 不能拿来要求公共 restore 接受 intraday save；反之 Server/Desktop caller 直接 bypass ProtocolSession 才会构成漏洞，目前查到的 caller 未 bypass。
- **未新增产品级漏项。** 本分片确认两个仍待实施/验证项：G16 clone、G38 分类；G39 是 verification contract 与 ADR-0017不一致的工具/契约债。其余静态证据未足以支撑新的产品语义候选；特别没有恢复被作废的“任意配额、来源优先、自由调度整局相等”。
