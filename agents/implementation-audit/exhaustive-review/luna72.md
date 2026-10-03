# Luna72：Session 实施与独立复核记录全文再审

基线产品提交 `08e4fc7`，其父提交为 `b76ece3`；审计工作树 `a7c7ce3` 是后续 merge。按目标核对提交树，Session / plans / strategy 产品源码与 `08e4fc7` 一致。遵循仓库 `AGENTS.md`、`docs/principles.md`；正式 ADR、`docs/trading-rules.md` 优先于实施记录。连续阅读全文至 EOF：`session/implementation.md` 44 行、`session/leaf-review.md` 49 行、`session/lifecycle-review.md` 35 行，共 128 行。只新增本记录；未改产品或 Git，未运行测试、编译、长验收或联网。

## 全文章矩阵

| 文件 / 原文范围 | 逐条复核项 | 当前实现追踪与结论 |
|---|---|---|
| implementation 1–5 | 基线、19 项动作、扩展 cursor、owner/caller 证据链接 | 当前源中可定位各声明的 owner 与生产入口；本审计树缺少原 action-index、relationships、source-manifest 和 validation JSON，因此不把工作记录摘要当作重建完整需求正文或重新核验历史测试日志的证据。 |
| implementation 7–9 | GameSession 唯一 committable state、facade hooks、COW/shadow/commit 边界 | `session.rs:1125–1135` 中 poison 与测试 hooks 留在 facade，`CommittableSessionState` 只有一个，`:1328–1340` 的 tick clone/commit 实际只提交 state。未发现 GameSession 的 `Deref` 暴露；ProtocolSession 的只读 `Deref` 是不同对象。 |
| implementation 10 | CandleBook 持有完整历史与活动日 K，外部仍投影双字段 | `candles.rs:92–121` 是唯一 `histories` / `active` owner。save/restore、snapshot、hash 仍分别消费既有两个 map；没有因窗口或派生 view 截断权威历史。 |
| implementation 11 | BeliefParticipantState 四成员归并、attention 独立、存档投影与失败残态 | `personal_state.rs:10–12` 的 participant 拥有 watchlist、price memory、information、belief；attention 留在 session 独立 map。`:69–105` take/install 的顺序保留，重复安装时 attention 先检查、watchlist 替换后 panic，其余成员不变；这是旧失败边界，不是重复安装能力。 |
| implementation 12 | 封存观察、RootReadContext、单账户 root、feedback 后 lifecycle 重读 | `decision_chain.rs` / `decision_chain/roots.rs` / `plan_chain_candidates.rs` 有实际捕获和生产 root 消费者；lifecycle 从现行 candidate PlanBook 重读。固定输入资源与当前计划分别有明确来源，不能要求混成单一 snapshot。RootReadContext 复制 PlanBook 的成本仍属已有 G16，不由 owner 迁移关闭。 |
| implementation 13 | Coordinator typed 结果、StockRoute 三 map、generation/retry/reconsideration | `plan_chain_candidates.rs:107` 定义独立 map，root coordinator 有 take、observe、安装及结果交还的真实调用路径；adaptive routing 消费 generation/retry/reconsideration。既有 R01 多余 append 已删除，不再重复要求 append。 |
| implementation 14–16 | plan getter/receiver、单位与交易边界、官方依据声明 | `TradingPlan` 字段受 `pub(in crate::plans)` 限制且外部消费已迁 getter；`ParentOrderPlan` 有 lifecycle receiver。Money 分、数量股、T+1、零股、价格约束、费用和日终范围由原有领域实现/ADR保持。本批没有交易规则变化；本轮没有重新核对官方网页。 |
| implementation 18–24 | 旧 core/leaf/lifecycle/caller/roots 发现修复状态 | 下文逐项核查旧发现；不能只引用初审“待修”，也不能把早期编译失败继续报作当前状态。 |
| implementation 26–32 | TDD、集中编译、63 源文件复核、非完整回归边界 | 原文如实区分没有行为 red/green、历史集中编译/短测与完整回归。当前树未附其结果 JSON；本审计不复述为本轮运行结果。 |
| implementation 34–38 | 最终短测和类型检查记录 | 文本保留首次 39/41、EPERM 与后续重跑的区别，也限定检查不等同完整回归。不能把历史记录提升为当前执行结果。 |
| implementation 40–44 | 无调用过渡包装器删除、最终 build/case 声明 | `run_chain_for_account` 全 engine 搜索不再有定义/生产 caller；`InstitutionDecisionRoot` 仍由 coordinator 真实调用。删除旧包装器不等于决策链缺失。最终运行数字仍按历史报告引用。 |
| leaf-review 1–9 | 范围、baseline、静态复核方法 | 范围限计划/策略、OpeningFigures、恢复、收费 audit、snapshot；报告明确不代替 roots/lifecycle 完整复核、不运行 Cargo。当前结论仍应限定于 owner/caller 静态链。 |
| leaf-review 11–15 | poison 漏迁、getter 漏迁、ZiNoise 投影测试缺口 | `v2_tests.rs` 当前使用 facade `session.poison`；`persistence.rs` 的 plan 与 linked-plan 校验读取同名 getter；`zi_noise.rs:296` 有非默认值完整投影测试。三项均有源码反证，不能重开初审项。 |
| leaf-review 17–23 | 大 A、最小范围、边界覆盖及既有 revision 失败语义 | 未见本重构引入新交易政策。特别是 `plans/revision.rs` 中 VersionOverflow 的局部写入顺序仍如 review 所述；裸 receiver 非原子失败边界被明确保留，不能误称默认 session tick 已泄露 candidate。 |
| leaf-review 25–35 | 修复后复核、Account/Position caller 与额外 ParentOrderPlan 增量 | getter、strategy projection 测试、restore_strategy 与恢复 caller 均存在。`ParentOrderPlan::from_saved_facts` 保留两个活动子单 Option 独立性，没有为方便 fixture 而 zip 丢失非法状态。 |
| leaf-review 37–49 | 最终 caller、冻结 SHA 与审查声明 | SHA 是当时签署范围证据，不视作永久冻结；本审计未重算完整 leaf diff hash，也未重跑短测。 |
| lifecycle-review 1–9 | baseline、限定源码/辅助 caller、ADR 与领域依据 | 生产链包括连续与竞价 lifecycle、protocol、candle、attention；辅助文件不是其完整 owner diff。引用官方来源日期沿用正式文档，不是本日重新核验。 |
| lifecycle-review 11–15 | 三门：母单 fill、协议 rollback、Candle、Attention | `execution/records.rs` 将结算事实交母单 receiver；竞价路径也有 checked receiver。`ProtocolState` 与 `PublicationFactCursor` 由 checkpoint/rollback/发布事实使用。CandleBook 保留无成交日和首笔成交边界；scheduler 到期候选经匹配后才处理，不将到期等同观察。 |
| lifecycle-review 17–23 | L1 paired writer、L2 getter caller、L3 反向替换矩阵 | L1 `execution.rs:140` 的 `replace_child_facts` 成对写两个 Option；L2 continuation 测试已改同名 getter；L3 `execution.rs:399` 有未关联母单 Buy→Sell 的重建 fixture，经生产 materialize/typed-fill 接缝断言重建结果。它是 adapter fixture，不代表完整账户撮合实跑。 |
| lifecycle-review 25–35 | 限定 diff-check、最终 SHA 与静态审查声明 | 复核范围、未跑测试和源码签署声明清楚；不从“静态通过”外推行为运行或完整回归通过。 |

## 生产链及旧发现复核

| owner / receiver | caller → consumer | 结论 |
|---|---|---|
| `CommittableSessionState` / shadow receiver | `GameSession` tick 与 P9 candidate commit → state swap | 唯一状态集合和 facade hooks 分离；错误 candidate 不通过 receiver 暴露为 authority。 |
| `SessionCandleBook` | 连续 finalize、auction 日终 → save、restore、snapshot、hash | 所有者和消费端均存在；仍是旧字段投影，未迁移 schema。 |
| `BeliefParticipantState` / `PlanPersonalState` | plan coordinator take → 单账户 root → install | 四成员移交与 attention 隔离真实接通；非法重复 install 的局部失败残态符合记录。 |
| `RootReadContext` / `InstitutionDecisionRoot` | 一次 capture → 并行/单账户决策 → typed 批次交还 | 不是悬空结构；继续保留旧 G16 PlanBook 深复制成本。 |
| `TradingPlan` getter / `ParentOrderPlan` receiver | plans lifecycle、persistence、continuation、continuous/auction lifecycle → candidate 或真实 fill | 字段限制后生产及测试 caller 已迁移；真实 `filled_qty` 来自结算，不由 submission 产生。 |
| `ProtocolState` / `PublicationFactCursor` | 三宿主 protocol session → checkpoint/rollback、发布 facts | 同 tick 序号与回滚有实际消费者；不代表宿主交付/背压等旧 G18/G19 已关闭。 |
| `NpcAttentionScheduler` | Session enqueue/恢复 → `pop_due` → candidate root | heap 只持候选调度；到期先匹配权威 attention，不复制 RNG 或自动读取信息。 |
| `CumulativeFeeAuditV2` / `LiveOrderReservations` / `RetailDecisionContext` / `CandidateTargetProposal` | save validation、snapshot 两簿汇总、retail strategy、PlanLifecycleReview → 校验/派生结果 | 均有 production consumer；是校验或纯派生组合，不新增费用规则、可受理订单、交易预算或第二权威状态。 |

P0–P9 和 lifecycle 的跨层核对以真实调用顺序为准：P0 quote expiry 的 checked release 汇总影响随后 live reservation 观察；P1 使用独立预算/订单事实，不把 SelfView 可替换现金回灌预算；P4 取消/释放不重加已封存预算；母单接受、撤单与实际 settlement fill 区分；连续与 auction day-end 共用其既有 lifecycle/交易日转换；P9 只提交成功 candidate。当前改动是这些链路的 owner/getter/receiver 迁移，未证实阶段顺序、Money/qty 单位或原失败优先级改变。上述是静态源码核对，不是本轮行为测试。

## 候选说法反证与遗留

- “旧 review 的 poison / getter / ZiNoise / paired writer / getter test caller / 母单反向矩阵仍漏修”：当前分别有 facade 字段路径、getter 消费、完整投影测试、paired writer、getter test 与反向 fixture，旧结论已关闭。
- “历史文档说等待 root build，所以产品当前未编译”：最终 implementation 段记录后续 build/check 与定向 case；历史阶段文本已经推进。但本审计没有原始结果 JSON，不能独立背书历史退出码。
- “P9 之前的局部 receiver 必须自己保证任意拒绝时字节级原子”：本批明确保持原 revision/receiver 失败顺序；candidate/P9 的外层隔离不可与裸库方法的局部失败保证混为一谈。
- “提交了母单/child 即产生真实成交”：生产 fill 来源仍是结算 typed fill；接受/撤单更新 lifecycle 不替代成交事实。
- “root/生命周期迁移已关闭所有策略和市场语义缺口”：不成立。既有 G16、散户 dated/信息消费边界、G18/G19 等独立范围不能由 OOP owner 实施结论核销；本轮不新增或重复这些总账项。

结论：指定 128 行已全文审阅；记录中的关键 owner、receiver、getter 与生产 consumer 在当前产品源码可定位，初审有效发现均有后续修复反证。未发现独立于现有问题总账的新产品缺口或交易语义漂移。结论仅是本次静态审阅；未运行测试/编译、未重验旧 validation JSON、未重新核验官方规则，也未重新签署 63 文件完整 diff。
