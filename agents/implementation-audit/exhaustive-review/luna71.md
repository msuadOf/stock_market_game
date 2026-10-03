# Stream 与 Session 三份全文材料独立 EOF 复核

日期：2026-10-03。当前 worktree HEAD `a7c7ce357bdc9f88c03633744b2d5815db49e9b2`，包含产品 `08e4fc7`（该提交本身只更新 Release 验收记录）。范围为指定的 stream 实施记录、session caller/core review 两份全文，以及其指向的 stream/admission/accountbook/COW/restoreowner 实现与正式规则。只读源码和文档；未运行 Cargo、测试、编译或性能验证；唯一写入是本审查记录。

## EOF 章节矩阵

| 文件 | EOF 行数 | 章节覆盖 | 原结论复核 |
| --- | ---: | --- | --- |
| `agents/oop-refactor-implementation/pipeline/stream/implementation.md` | 57 | L1-9 范围与依据；L11-20 owner/caller 与生命周期；L22-35 验证交接；L37-40 未完成门禁；L42-57 后续六个 caller 文件、fixture 迁移与编译后修正 | owner 的真实定义和生产入口存在。文中已明确未运行 Cargo/产品测试、未完成独立复核；不能把 rustfmt / diff-check 描述升级为行为验收。追加 caller 与非法 fixture 记录可在当前源码找到。 |
| `agents/oop-refactor-implementation/session/caller-review.md` | 38 | L1-5 reviewer、基线、范围；L6-12 三门结论；L14-20 非法 fixture；L22-38 执行边界与 10 个文件指纹 | 当前 Account、AccountBook、Market 与指定 fixture 路径支持原文结论。报告范围是追加的 10 个 caller/测试文件，不能外推为全 Account 或全仓 diff 的审计。 |
| `agents/oop-refactor-implementation/session/core-review.md` | 83 | L1-11 范围与总论；L13-39 领域语义、必要性、边界；L41-60 N04 覆盖缺口及关闭证据；L62-83 增量复核、执行与源码指纹 | N04 原覆盖缺口、后续补测内容和审查范围有区分；当前源码仍有对应 owner 与测试。报告明确没跑 Cargo/测试，且不声称旧基线 hash 字节兼容。 |

## Stream 与 admission 实际链

`StockStreamCoordinator` 在 `packages/engine/src/session/pipeline/stock_stream.rs:291-299` 独占当 tick 的 `available`、`pending`、`in_flight` 及 payload/notification channels，不拥有 session、账户或结算 authority。真实 facade `drive_stock_stream` 在 `:271-287` 创建 owner 并启动一次 `drive`；continuous/auction transaction 继续走既有入口。`dispatch_ready`（`:323-370`）每只股票单独取 shard，按同股票聚合 pending operations，worker 先将 typed payload 发回、成功后才发 Stock notification。通知故意不携带股票身份；`receive_completion`（`:396-406`）消费任一已回传 payload，并检查 in-flight 唯一性。两个异步 channel 可能交叉，但每次 Stock 唤醒都在 payload send 成功之后，且只有 coordinator 消费 payload，因此通知计数与可消费 payload 数一一对应；没有要求跨 worker channel 全序，也没有把完成次序变成交易优先级。

`drive`（`:419-455`）仍使用 `rayon::in_place_scope`，coordinator 留在宿主调用线程；单 Rayon worker 在 `poll_progress`（`:372-394`）用 `yield_now` 协助任务。PlanRoot 通知分支（`:440-445`）只调用 progress callback，不消费股票 payload。生产 `ReadyStockStream::continuous_progress/auction_progress` 对该分支调用 `ready_batch_without_waiting_for_roots`（`ready_stock_stream.rs:52-120`），完成股票则先以 typed outcomes 推进计划再提交后续 ready operations。最终 finish、ReceiptAggregation、Settlement、P9 仍在其他 owner，不在 stream coordinator 内。上述旧独立复核 `review-stream-callers.md:23-26` 的通知顺序、单 worker 和过时 root 结论与当前实现一致；其 R2 后的 owner 抽取没有提供新的反证。

`ReadyAdmissionPlan` 只包住一个 ready batch 的候选、偏序及 stock gates（`local_admission.rs:83-90`）。入口 `admit_ready_batch`（`:61-81`）保留单项/无依赖 fast path；其他请求在 `prepare` 按原候选扫描顺序先调用 `receipts.observe`，再建立 cash/account 与 shares/account+stock 资源 lane 和每股计数（`:92-157`）。`build_resource_edges` 按账户局部 receipt 排序（`:159-176`）；显式 quote predecessor 校验及边在 `add_quote_dependencies`（`:178-221`）；StockAdmission 并发登记实际 gate 到达顺序、检测锁 poison/依赖计数/cycle（`:223-262`）；`finish_layout` 合并 gate、资源和 quote 边后用原 `ready.pop()` 拓扑布局（`:264-309`）。撤单不占 cash/shares lane，只有显式同账户同股 Cancel→Place 才建立 quote 依赖。receipt 扫描发生在较后依赖校验之前，既有错误后游标推进及 overflow 先于 quote 错误的行为没有被重排或撤销。无冲突结果仍保留原 candidates 顺序，布局只约束因果和同资源 lane。

这是纯 owner 提取：相对 `b89afb3`，旧的函数局部 `pending/in_flight/sender/receiver` 被搬入 struct，各阶段代码随 owner 接收 `self`；admission 的候选索引、ResourceLane、gate 注册和拓扑处理被拆成 `prepare/build_resource_edges/add_quote_dependencies/run_stock_gates/finish_layout`。对照 diff 未发现新增业务分支、交易校验、余额 owner、错误吞没或 fallback。`review-stream-callers.md:27-29,35-36` 对 fast path、边和最小 owner 的原结论成立；实施记录 `:24,37-40` 同时诚实保留尚未统一执行测试与独立 diff review 的门禁。

## AccountBook、COW 与 restoreowner

股票/admission 新 owner 不更改 AccountBook。当前 `session/account_book.rs:17-34` 仍以 group/page Arc 与 `AccountPage` 保存账户，clone 页时重置 validation cache；`:38-54` 校验 StrategyState 后缓存结果，`:69-76` 的 `clone_for_shadow` 先逐页校验再浅 clone；写入口 `get_mut`、`values_mut`、`insert` 分别在 `:124-133`、`:110-117`、`:157-167` 做 Arc COW 并 invalidate。因而账户 shadow 仍共享未写页面且不把旧页缓存错误带入修改页。`AccountPagedMap` 亦保持 group/page/value 分层共享（`session/account_paged_map.rs:12-18,41-67`）；private tick 的并行变更按 group 拆分，在首次写入时 `Arc::make_mut`（`:117-162`）。这是复核相关 COW 事实，不代表本轮 stream diff 修改该 owner。

restoreowner 核对：公开 `GameSession::restore` 先做 `validate_save_slot(save)?`，再创建 session 并通过 AccountBook `values_mut/get_mut` 和 `restore_balances` 覆盖现金、持仓/T+1 事实（`session.rs:2716-2752`）。个人聚合 owner 由既有四个存档 map 重建，随后 reconcile、恢复 runtime v2（`:2962-2984`）。v2 路径先 `validate_runtime_v2`，再构造 ledger/receipt 和策略，随后对账户 `clone_for_shadow`；所有策略事实都转换成功后才在私有 AccountBook 中 `get_mut`、`restore_strategy`，最后一次安装 accounts/ledger/receipt cursor（`persistence/v2.rs:372-443`）。没有看到 getter/fixture setter 取代生产 restore；validation、原子安装和 AccountBook 页缓存失效边界仍实际存在。`caller-review.md:16-20` 与 `core-review.md:36,64` 对其各自限定范围的 caller/恢复结论未被当前源码反证。该复核不外推为对所有存档字段或 Account 所有 setter 的完整重审。

## 正式规则优先与旧修复复核

采用正式规则而非历史实施笔记作交易语义基线：ADR-0017 顶部 2026-09-24/25 修订及 ADR-0018 §7 将旧 `npc → player → plan_chain` 来源序、全局密封序作为优先级的历史文本废止；实际约束是账户真实资源冲突、同股实际受理顺序和显式计划依赖。ADR-0017 §1 阶段 3–5 规定 P3/P4 增量、worker 独占股票 shadow、完成即通知、调用协调者推进续行；正式 `docs/trading-rules.md` 约束 A 股领域规则和已声明简化。此次只重构 tick 私有状态生命周期，不改价格时间撮合、金额分/股份、T+1、费用、竞价或结算语义；未重新联网认证交易所资料。

旧问题及修复证据：stream owner 原拆分曾出现完成通知与 payload 生命周期审查点；现行代码明确 payload 发送成功才通知，且 coordinator 通过 `try_recv` 检查 payload，通知与 payload 断开/缺失和过时 root 有对应测试（`stock_stream.rs:548-609` 及后续测试）。local admission 原扫描顺序和 edge/gate 顺序现有回归样例包括 mixed phases、receipt 扫描后错误、overflow 优先、cash 与异股 shares 独立、损坏 cycle/atomic underflow（`:680-744` 与前序测试）。实施记录声称新增这些 owner 私有测试，但明确未执行；本次源码复核只确认测试存在，不报告通过。

旧独立审查中 `review-stream-callers.md` 的 S1 是另一处 SessionExecution participant 写回测试覆盖缺口，L45-47 记载补测后静态关闭；该问题不属于本次三份全文所限定的 stream owner 实现，未把它误列成新缺陷。三份文档都限定了审查/运行范围，core review 也明说静态结论不能替代测试，故不将此前 PASS 扩大解释。

## 结论与候选反证

- 三份目标材料均已读至 EOF，章节矩阵覆盖完整。stream coordinator 与 admission plan 有真实定义及生产 caller；旧逻辑到 owner 拆分的差异保持既有边界。
- 大 A 语义依据沿用当前正式规则，未发现来源优先级回归、股/分单位漂移、账户可用余额重建、T+1 变化或终结职责迁入。必要性对应 owner 封装，范围没有扩出交易业务。
- AccountBook 的 COW/cache invalidation 及 restoreowner 的先校验、私有构造、后安装链当前存在；未发现 stream/admission 迁移绕过该链。caller/core reviewer 的窄范围结论成立，范围外仍不作推断。
- **新候选反证：无。** 可复现代码路径上未发现通知丢失导致 payload 静默丢失、同股优先级被 layout 决定、无关账户/股票新增总序或恢复输入被 getter/fixture 路径放宽。实施记录中的测试尚待主代理统一运行，因此这是静态结论，不是行为通过声明。
