# sweep08：母单、escrow tick 与草稿容量全文复核

## 范围与口径

- 审计产品基线为 `b76ece3`；工作期间其他审计文档合入，读取时 HEAD 为 `4ad5a2e`，本任务没有修改产品代码、Git 状态或运行测试。
- 已读根 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md` 及现有总账、`reaudit-pipeline-contracts.md`。`agents/` 下没有更深的 `AGENTS.md`。
- 以下三份文档按连续全文读取，不以关键词命中替代全文：ADR-0015 共 47 行；ADR-0017 共 157 行；ADR-0019 共 60 行。总计 264 行。
- 本轮逐条追正式入口、状态所有者和消费者，测试仅阅读源码；没有以本轮未执行测试推导生产能力缺失。A 股边界使用现有 `docs/trading-rules.md:21`、`:41`、`:44`，没有改变规则或新增交易所主张。
- 结论：这三份全文没有确认可新增为 G 的生产遗漏。既有 G16、G38、G39 仍保留；下面列出已实现、已取代和待验收边界，避免把文档历史态再次计数。

## ADR-0015：47 行连续全文

| 原文位置与章节 | 当前生产证据 | 状态 |
|---|---|---|
| `docs/decisions/0015-parent-order-execution.md:6` 问题：目标、成交、剩余与截止时间需要恢复 | `packages/engine/src/session/execution.rs:14` 的 `ParentOrderPlan` 明确保存 target/filled/child/active/expiry；`packages/engine/src/session.rs:499` 使用专用 `SaveParentOrderPlan`。 | 已实现 |
| `docs/decisions/0015-parent-order-execution.md:13` 按账户/股票唯一母单，一张在途子单，同向不清零、反向替换 | `packages/engine/src/session/execution/orders.rs:50` 按 existing code 复核，`:74` 检查 expiry/dynamic，`:76` 按方向更新或替换；`packages/engine/src/session/execution.rs:163` 受理时检查已有 active child。 | 已实现；旧 ValueStrategy 独占范围由后文修订 |
| `docs/decisions/0015-parent-order-execution.md:18` 只凭真实生命周期更新，不承诺成交、玩家不自动获得母单 | `packages/engine/src/session/execution/records.rs:7` 消费 fill，`:72` 消费 submission，`:103` 消费 cancellation；`packages/engine/src/session/execution.rs:180` 按 side/order identity 更新；`packages/engine/src/session/pipeline/adaptive_plan_chain.rs:1552` 校验 parent acceptance。 | 已接实际 P3/P4 路径；没有目标数量直接结算 |
| `docs/decisions/0015-parent-order-execution.md:23` 已验证：部分填恢复，拒绝错误账户/股票/数量/过期/子单引用 | `packages/engine/src/session/persistence.rs:530` 确认机构账户范围，`:549` 校验 code、整手、满额、expiry、ID；`packages/engine/src/session.rs:2880` 从 live child 唯一重建剩余量，不唯一返回具体 InvalidSave。 | 已实现；测试源码 `persistence/v2_tests.rs:24`、`:514`，本轮未执行 |
| `docs/decisions/0015-parent-order-execution.md:29` 多 seed 校准、寿命/撤单/冲击、专门诊断 | 已有 `NpcOrderLifecycleBook`、报价复核与 typed 计划报告；例如 `continuous_tick_transaction.rs:292` 取得完整 plan completion，`continuous_lifecycle_projection.rs:16` 投影生命周期。 | 验收/校准边界；没有从文档后续验收推导所有统计已完成 |
| `docs/decisions/0015-parent-order-execution.md:35` 个人 TradingPlan 执行子状态、次日复核续发、不复活陈旧子单 | `packages/engine/src/session/plan_chain_candidates.rs:230` 分发 QuotePlans/Lifecycle/AccountExecution；`packages/engine/src/session/pipeline/auction_day_end.rs:2185` 同步并对每个 active plan 应用 TradingDayEnded，`:2162` 清 parent map。真实多段计划由 adaptive coordinator 继续。 | 已实现主链；零散 ParentOrderPlan 保持机构内部结构不构成散户缺能力 |

## ADR-0017：157 行连续全文

| 原文位置与章节 | 当前生产证据 | 状态 |
|---|---|---|
| `docs/decisions/0017-escrow-parallel-tick.md:7` 三项修订：来源类序、收据键序、跨 worker 整局相同 | `ready_ingress.rs:34` 收集 NPC/player roots，`local_admission.rs:100` 只建立 Cash(account)/Shares(account,stock) 冲突边，`:178` 保留报价前驱，股票临时入口处理同股受理；输出编号没有回灌交易优先。 | 产品来源平权已实现；K7 跨 worker 完整产物断言仍属 G39 |
| `docs/decisions/0017-escrow-parallel-tick.md:20` 上下文与游戏 escrow/A 股边界 | `transition.rs:77` 卖单零 cash 腿与实收封顶；领域限制仍由 P3 账户、P4 股票负责。`docs/trading-rules.md:44` 明确这是游戏简化。 | 已实现，不能宣称真实交易所清算 |
| `docs/decisions/0017-escrow-parallel-tick.md:28` 单一 rayon 路径与 shadow 单提交 | `authoritative_tick.rs:20` 分阶段调用 prepare/commit；`continuous_tick_transaction.rs:96` 先 plan_tick/transaction 再准备 commit；WASM `apps/web-wasm/src/lib.rs:24` 与 `apps/web/src/host/wasm-worker.ts:129` 初始化相同 rayon pool。 | 已实现；预算 1 不需要独立串行引擎 |
| `docs/decisions/0017-escrow-parallel-tick.md:36` P0 一次过期，P1 post-P0 live 截点 | `quote_expiry.rs:89` 重复 P0 返回 fatal，收据/events 进入私有 plan；`decision_resources.rs:93` 从 live envelope 按账户并行 seal，`:136` cash 扣 reservation，`:155` sellable 扣 reservation。 | 已实现；没有额外加回 P0 released |
| `docs/decisions/0017-escrow-parallel-tick.md:38` P2 固定快照、真实全账户 roots、typed 续行 | `ready_ingress.rs:73` 从 observed accounts 捕获真实 roots；`plan_chain_candidates.rs:304` 冻结 RootReadContext，`:324` 并行生产 roots，`:342` 仅取已完成 typed payload；`ready_stock_stream.rs:52`、`:87` 消费连续/竞价实际结果续跑。 | 已实现多段主链；RootReadContext 的完整 PlanBook 复制是既有 G16 |
| `docs/decisions/0017-escrow-parallel-tick.md:39` P3 私有持续预算、ID/密封索引与 overflow | `account_validation.rs:239` 唯一累计 validator，`:288` 后轮共享原 snapshot/继承预算，`:320` 成功后装入预算，`:342` 仅 accepted Place 计 ID 并 checked_add；Cancel/rejected 不计 Place。 | 已实现；同 tick 撤单/拒单释放不回补 P3 |
| `docs/decisions/0017-escrow-parallel-tick.md:40` P4 股票 shadow 续接、独立任务及时通知、一次收尾 | `stock_stream.rs:348` 从 available 取回独占 shard，`:360` scoped worker 回 payload 并通知；`:419` coordinator 消费完成结果后立即续跑；`continuous_tick_transaction.rs:282` 排空 stream 后 `:305` 单次 finish，`stock_stream.rs:180` 竞价按股并行 finish。 | 已实现；股票字典的汇总顺序不等于受理优先 |
| `docs/decisions/0017-escrow-parallel-tick.md:41` ReceiptAggregation 唯一性、连续编号、双账本 | `receipt_aggregation.rs:63` 完整验证、insert created、应用 receipts、再次验证与 cursor checked_add；`ledger_conservation.rs:24` 逐 envelope 检查 PreSeal/SealedBatch 并分别加总 cash/shares。 | 已实现；不同 envelope 局部键不要求沿全局编号递增 |
| `docs/decisions/0017-escrow-parallel-tick.md:42` 一次 Settlement、正数量 Fill、实收费用、Buy-before-Sell | `settlement.rs:54` 检查 Fill 正数量，`:83` 按 side 聚合，`:96` 账户并行准备，`:119` Buy 后 `:127` Sell；`continuous_tick_finalizer.rs:158` 统一交易事务后才安装账户 patch。 | 已实现；没有用 nominal 重收费用 |
| `docs/decisions/0017-escrow-parallel-tick.md:43` Projection 已消费事实恰一次 | `continuous_lifecycle_projection.rs:31` 拒绝重复 operation identity，`:38` 核对 consumed operation，`:57` 核对 consumed receipt；`continuous_tick_transaction.rs:323` 把 plan completion consumed 交最终 lifecycle。 | 已实现；已有全填无 OrderAccepted/部分填源码测试 |
| `docs/decisions/0017-escrow-parallel-tick.md:44` P8/P9 最终校验、只在显式入口完整证据/哈希 | `candidate_commit.rs:126` 校验 receipt/event identity 与 applied journal，完整 TickCommitEvidence 受 capture 开关控制；`:46` 校验 cursor、rebase ledger，`:78` 两种 hash 都限 cfg(test)，`:96` commit 单次 swap。 | 已实现；普通 tick 没有整局双 hash |
| `docs/decisions/0017-escrow-parallel-tick.md:47` 多根/多撤/失败/全填/溢出/局部规则验收 | `adaptive_plan_chain_tests.rs:173`、`:223`、`:516`、`:651`、`:656`、`:661`、`:1078`；`continuous_tick_transaction_tests.rs:958`；`account_validation_driver_tests.rs:549`、`:594`。 | 测试源码覆盖相应场景；没有宣称本轮或长矩阵通过 |
| `docs/decisions/0017-escrow-parallel-tick.md:49` root/stock 共用通知、单 worker 帮助任务；`:51` 普通报价撤旧→下新跨恢复保留 | `stock_stream.rs:41` 同 tick 临时通知，`:372` 单 worker yield，`:440` PlanRootReady 仅轮询；`local_admission.rs:178` 校验显式报价前驱，`session.rs:2921` 恢复 pending_npc。 | 已实现；没有把撤单成功作为普通报价的新 P3 前提 |
| `docs/decisions/0017-escrow-parallel-tick.md:53` 数据流/StrategyState、资源向量与费用字段 | `decision_snapshot.rs:30` 保存 StrategyState；`npc_decisions.rs:88` 只输出 prospective states/raw intents；`transition.rs:38` 买方费用/价格改善释放，`:197` cumulative unpaid 封顶，`:215` commission→stamp_tax→transfer_fee。 | 已实现；费用追收从累计 nominal 减累计 charged，不只取本腿 nominal |
| `docs/decisions/0017-escrow-parallel-tick.md:65` 事件/CivilUpdate/TickFrame、Session 共享身份与 P0 分段 | `protocol/civil/session.rs:225` 完整 frame 成功才 publish，`:283` 独立 civil update，`:343` 按 tick 管理 fact cursor；`candidate_commit.rs:162` 检查 P0 高位身份段、对应 event、JS 安全域及键唯一性。 | 生产协议存在；三宿主交付/暂停细节由宿主审计范围交叉覆盖 |
| `docs/decisions/0017-escrow-parallel-tick.md:116` 失败隔离与 poison | `failure.rs:73` 唯一正式 tick，`:80` 只记录 poison，`:85` save require_healthy；`candidate_commit.rs:96` prepare 成功后才 infallible swap；`stock_stream.rs:363` 放弃 tick 后 receiver 关闭，只丢弃私有结果。 | 已实现；panic 不承诺恢复，未要求 catch_unwind |
| `docs/decisions/0017-escrow-parallel-tick.md:121` 九条分歧 | #1/#2/#3/#5/#8 由上述快照、预算、P3/P4 及 fatal 路径实现；#4 当前簿取消余量由 typed cancel 投影；#6 事件 identity/seq 覆盖；#7 `persistence/v2.rs:97`、`:109` 显式拒绝旧/未来版本；#9 费用路径见 transition。 | 主链已实现；#6 验证工具仍计 G39，不重建已退役旧语料工具 |
| `docs/decisions/0017-escrow-parallel-tick.md:137` 备选、`:144` 后果、`:150` 关联 | 没有独立串行 engine、常驻股票 authority actor 或卖单 cash escrow；临时 scoped worker/通知不是被排除的常驻模型。 | 已遵循；有限证据不证明所有负载无死锁/固定性能 |

## ADR-0019：60 行连续全文

| 原文位置与章节 | 当前生产证据 | 状态 |
|---|---|---|
| `docs/decisions/0019-draft-market-scope-and-capacity.md:7` 背景与 `:13` 无任意条数配额 | `apps/server/src/actor.rs:949`、`apps/desktop/src-tauri/src/actor.rs:627` 使用 unbounded command channel；P3/persistence 查找未见 5000/50000 的请求或挂单数量门槛；`session.rs:2920` 恢复 pending queue。 | 已落实旧配额移除；不能把游戏软 watchlist 注意力边界当机器挂单配额 |
| `docs/decisions/0019-draft-market-scope-and-capacity.md:18` callers 停止等待不撤回已投递操作，控制处理交还机会 | Server `actor/fatal_tests.rs:356` 有 command burst/caller stop waiting 验证源码；actor 命令入口投递后独立消费，`actor.rs:1117` bounded processing batch 仅是调度。 | 有生产实现与代表性测试；100000 分块不是全局订单配额 |
| `docs/decisions/0019-draft-market-scope-and-capacity.md:24` 性能记录/机器故障，`:27` 删除股票/NPC/历史乘积配额，`:29` byte/depth 入口 | `persistence.rs:944` 当前 decode 512 MiB；Server `routes.rs:41` 加 1 MiB 包装，`apps/server/src/lib.rs:109` 使用同一 body limit；保存领域校验继续检查资金、引用、费用。 | byte/depth 仍获保留；性能证明需要实测，不因 unbounded 推导任意承载 |
| `docs/decisions/0019-draft-market-scope-and-capacity.md:31` 最小存档/方便修改、`:36` 重建盘口预留与母单剩余 | `session.rs:499` 专用 parent save 不存 active_child_remaining_qty，`:2880` 从唯一 live child 重建；`persistence/v2_tests.rs:470`、`:514` 验证源码。runtime v2 仍校验现有 ledger 与资产交叉事实。 | 母单剩余精简已实现；完整新最小存档仍是文档明确未完成的方向，不能升级为已定实现漏项 |
| `docs/decisions/0019-draft-market-scope-and-capacity.md:39` 不可推导事实/历史保留，`:42` 编辑冲突具体诊断 | `session.rs:2920` 保留 pending player/NPC；`persistence/v2.rs:121` 捕获 runtime/fees/RNG 等事实；`persistence.rs:549` 和 `session.rs:2910` 显式 InvalidSave，不补钱或默撤。 | 已有可执行校验；资金不足如何整理文档明确待决定，不恢复历史资金来源证明 |
| `docs/decisions/0019-draft-market-scope-and-capacity.md:45` 协议/宿主 rollback 使用内存候选与共享 frame | `protocol/civil/session.rs:19` clone game/checkpoint state，intraday 共享不可变片段，`:79` 直接换回 checkpoint；Server `actor.rs:1124` 捕获、`:1214` rollback。 | 已实现；civil day 内部 save backup 精简文档明确另待，不能误算协议仍走 save 往返 |
| `docs/decisions/0019-draft-market-scope-and-capacity.md:47` 固定两局/单局验证 | `apps/server/src/actor.rs:403` MAX_SESSIONS=2，`:933` 局数受理，`:949` 每局独立命令入口。 | 固定两局是明确保留政策，不是应删除的任意挂单配额；不要求多局压力验证 |
| `docs/decisions/0019-draft-market-scope-and-capacity.md:56` 测试精简、不保留旧开关/旧路径/迁移 | `persistence/v2.rs:97` 缺失/旧/未来版本都显式拒绝，无迁移 fallback；命令 burst 与保存恢复代表性源码存在。 | 已落实主要契约；不恢复旧容量拒绝矩阵 |

## 新候选及反证

| 检查的候选 | 原文依据、代码事实与反证 | 最终处理 |
|---|---|---|
| 散户不能保存机构母单因而没有统一执行 | ADR-0015:42 允许普通散户小单复用协调器，不要求机构 ParentOrderPlan 结构普及全部账户；正式 typed route 已投影普通报价与实际成交，恢复仍校验机构 parent 是原有内部分工。 | 不新增 |
| roots 失败后后台继续计算导致 authority 部分提交 | `plan_chain_candidates.rs:324` detached root 只持有不可变 snapshot/自有 personal，结果发给可丢弃 channel；`:374` install 仅在候选消费者。`stock_stream.rs:430` scoped stock 工作结束前不离开 scope。没有 worker 写 authority。 | 失败原子性反证成立；不能从暂存 root 活跃推导数据泄漏/提交 |
| source 初始 vector NPC 在前等于 NPC 全局优先 | `local_admission.rs:100` 资源链按真实账户资源，`:223` 同股 gates 的并发入口决定局部受理；`ready_ingress.rs:57` 只拼就绪候选，root 完成及时加入。 | 不重提已被取代的来源类序 |
| 同 tick cancel/refund 未释放可分配余额是漏实现 | ADR-0017:37、:39 明确资源只在 P1 seal；`account_validation.rs:288` 后轮共享不变 resources 和累计 budgets，符合约定。 | 不新增，也不建议回补 |
| 后续大卖出腿不追历史欠费 | `transition.rs:202` unpaid = nominal_after - charged_before，`:203` 再按本腿 gross 封顶；三项按固定次序分配。 | 已实现 |
| 字节存档额度/服务端两局/时间片是旧容量限制复活 | ADR-0019:29、:49、:19 分别明确保留；现有 512+1 MiB、2 局与 batch steps 一致，不是挂单/请求数量上限。 | 不新增 |
| 普通 tick 完整证据/双 hash 未删除 | `candidate_commit.rs:78` hashes 是 cfg(test)，`:141` capture_commit_evidence 开关；普通生产 prepare 只做必要 journal/守恒。 | 不新增 |
| RootReadContext 全历史 PlanBook 复制、新机会标 ExistingPlan、K7 跨 worker 完整 artifact 相等 | 分别对应既有 G16、G38、G39。源码再次确认 `plan_chain_candidates.rs:304` capture、`decision_chain.rs:972`/`:1236` ExistingPlan；验证工具比较入口由现有总账记录。 | 保留既有编号，避免重复统计 |

本轮所有结论来自静态入口/消费链与测试源码。没有运行 Rust、Node、K7 或长验收，不宣称这些测试通过；没有找到新 G 不意味着对无限负载或所有跨线程调度作正确性证明。
