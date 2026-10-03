# stream 与剩余 caller 独立复核

## 范围与证据

- 日期：2026-10-03；基线：`b89afb3346743a4b4fccf26c9ac9ff108595f696`。
- 复核者未参与产品实现；本次只写此复核记录，不改产品文件、不派 subagent、不运行 Cargo、编译或测试。
- 权威动作：`assigned-actions.json` 的 `engine-pipeline-11-A01` 与 `pipeline-R2-N08`。caller 仅限新的 Session state、Account、ParentOrderPlan、CandleBook、BeliefParticipantState 接口迁移。
- 已完整阅读下列 35 个实际变更文件相对基线的 diff。stream、local admission 和生产 caller 源码阅读全文；测试文件核对完整 diff、fixture、原断言和相关执行上下文。额外阅读全文核对 `ready_ingress.rs`、`ready_stock_stream.rs`、`stock_stream/root_ready_tests.rs` 与 participant owner，并核对 settlement patch 的构造和采纳路径。
- 依据：AGENTS、principles、architecture、open-questions、ADR-0017 顶部 2026-09-24/25 修订、ADR-0018 §7，以及 `trading-rules.md` 已登记的 A 股与游戏简化边界。本批没有新增或修改交易制度，未重新访问官方规则网页，不把现有文档依据声称为 2026-10-03 的重新官方认证。

### 文件清单

路径均相对 `packages/engine/src/session/pipeline/`：

- `stock_stream.rs`、`stock_stream/auction_tests.rs`、`local_admission.rs`。
- `account_validation_context.rs`、`candidate_commit.rs`、`continuous_matching_adapter_tests.rs`、`continuous_tick_transaction_tests.rs`、`executor_perturbation_tests.rs`、`institutional_experience_projection.rs`、`npc_state_projection.rs`、`npc_tick_preparation.rs`、`pre_open_transaction_tests.rs`、`quote_expiry_checkpoint_tests.rs`、`receipt_aggregation.rs`、`session_execution_transaction_tests.rs`、`stock_auction_adapter_tests.rs`、`stock_execution_transaction.rs`、`tests.rs`。
- `account_validation_context_tests.rs`、`account_validation_driver_tests.rs`、`authoritative_tick_tests.rs`、`candidate_commit_tests.rs`、`commit_evidence_tests.rs`、`continuous_matching_adapter.rs`、`continuous_tick_transaction.rs`、`continuous_trade_acceptance_tests.rs`、`incremental_auction_round_tests.rs`、`npc_state_projection_tests.rs`、`npc_tick_preparation_tests.rs`、`pre_open_transaction.rs`、`receipt_aggregation_tests.rs`、`session_execution_transaction.rs`、`shadow.rs`、`stock_auction_adapter.rs`、`stock_execution_transaction_tests.rs`。

`/tmp/pipeline_remaining_0.txt` 与 `_1.txt` 中没有 diff 的文件不属于这次实际改动；`retail_projection_persistence_tests.rs` 按任务指派由另一名独立复核者审查。

## 大 A 语义与时序

1. `stock_stream.rs:360-365` 保留 worker 先发送 payload，成功后再发送 Stock notification。不同 worker 的两个 channel 可以交叉排序，notification 不携带股票身份；每个 Stock notification 消费任一已完成 payload，累计数量守恒足以保持一一消费，不要求两个 channel 同序。该迁移没有把 worker 完成顺序、股票 key 或输出排序升格为交易优先级。
2. `stock_stream.rs:396-405` 保留完成股票的 in-flight 登记校验、单次回收和原有错误传播；出现异常时整个私有 tick 被放弃。receiver 已关闭后的 worker 不能经该 channel 传递新 fatal，原来的丢弃行为符合 A11 明确契约。
3. `stock_stream.rs:430` 保留 `rayon::in_place_scope`，coordinator 仍在调用线程；单 worker 的 `yield_now` 也保留。外部多宿主不会因这次提取全部迁入 Rayon pool 并占满 worker。
4. `stock_stream.rs:440-445` 的 PlanRootReady 分支只轮询计划，不读取股票 payload。生产 `ready_stock_stream.rs` 对此调用 `ready_batch_without_waiting_for_roots`，因此过时 root notification 不会提前终结 stream、吃掉 Stock payload 或等待未完成 root。既有 root_ready 测试仍覆盖 1/2 worker 的过时通知完整驱动路径。
5. `local_admission.rs:68-77` 的短批和无冲突 fast path 与旧函数等价；`prepare:100-126` 的 receipt 扫描位置保持，后续坏依赖不撤回已推进的 plan ordinal，overflow 也先于 quote dependency 校验。外层抛弃 tick，而非为这个 plan 新增 retry 原子承诺。
6. resource edges 仍分别为同账户 Cash 与同账户同股 Shares；普通 Cancel 没有新增账户资源边，quote replacement 只保留明确 Cancel→Place 依赖。股票 gate 仍记录实际并发受理，拓扑合并的 `ready.pop` 顺序保留。无关账户、股票没有新增交易总序。现有同股 gate 的等待范围并非本批新引入，未据此宣称项目已完成全部受理平权或整轮并行优化。
7. caller diff 保持金额为分、股份为股、T+1、已预留资源、交易时段、类别与价格保护参数。测试中的 `t1_enabled=false` 仍只作为内部 Settlement seam，原有对外 A 股 T+1 限制不变。DTO 和存档事实字段没有被误改为 Session state 字段。
8. `npc_tick_preparation.rs:121-125` 新 `next_scheduled_tick()` 仍读取原 heap 的队头 tick，保持 O(1) 到期判断；没有扫描或制造新的账户优先级。
9. `session_execution_transaction.rs:89-98` 先取出既有 participant，仅写 `belief_mut()`，再放回原 account key；information、watchlist、price_memory 不重建、不置空。`institutional_experience_projection.rs` 仅从既有 participant 的 belief 克隆经验 patch，missing participant 的 typed error 发生在 session 容器安装之前，因此新增 `expect` 对应的是已检查不变量，而非新增可达 typed failure。

## 必要性与最小范围

StockStreamCoordinator 只拥有本 tick 的 available、pending、in-flight 和 channels；callbacks 仅在一次 drive 内，最终 finish、ReceiptAggregation、Settlement 与 P9 留在原 owner。ReadyAdmissionPlan 的候选索引、边和 gates 共同消费，没有引入持久计划、共享余额、actor 或新的 trait 层次。caller 修改均为封装后的访问适配；`incremental_auction_round_tests.rs` 的 `.apply_round` 适配已迁 owner 方法，仍保留原错误样本与首错断言。未发现无关产品行为、断言削弱或新依赖。

## 发现与待补验证

### S1：session execution 的 participant 写回缺少直接回归覆盖

- 位置：`session_execution_transaction.rs:89-98`，所属测试 `session_execution_transaction_tests.rs`。
- 类型：边界测试缺口；没有发现当前实现丢失成员的代码错误。
- 原 4 个所属测试覆盖空 Fill 的成功安装、seq overflow、split cursor、unknown stock；它们不会产生非空机构 belief_patch。因此只跑这些测试不能直接证明这条新的 participant 写回循环保留 information、watchlist、price_memory。
- 修复建议：真实机构成交经 `apply_session_execution_transaction` 产生非空 belief_patch，核对 belief 经验确实更新，另外三个成员保持原值，另一机构四成员保持不变。fixture 应给这些成员非空事实，避免清空它们也能通过。
- 状态：已修复并再次静态复核。已重新全文阅读 `session_execution_transaction_tests.rs` 相对同一基线的完整更新 diff 和源码。
- 新用例 `institutional_fill_updates_belief_and_preserves_participant_members_and_other_account`（该文件第 91 行起）为 information、watchlist、price_memory 设置非空事实，通过真实 stock adapter、P3/P4 与 SessionExecutionTransaction 形成机构成交，断言两腿收据、Trade 事实、belief 经验真实更新、买入 T+1 锁定 100 股、其他三个成员保持不变，另一机构的四成员与账户现金、持仓、策略保持不变。该断言能够识别重建或清空 participant 成员的回归。
- 共享 helper 新增 buyer 参数并从真实 adapter 捕获已挂单 envelope；旧 4 个用例仍使用 AccountId(0)，空簿行为及原断言保持，没有削弱错误路径。本复核者未执行该测试；运行证据由主代理另行记录。

## 当前结论

完整 diff 静态复核未发现新增的交易规则偏差、notification 竞态或 caller 状态丢失错误；必要性与范围符合 assigned-actions 和本次 caller 接口迁移。S1 覆盖缺口已由未实施该改动的本复核者重新审查更新 diff，静态闭环通过，没有未解决的有效发现。本复核没有执行任何测试，不能替代主代理的编译与定点短测试结果，也不宣称本批全部验收已完成。

## 编译 02 后的 getter 收口复核

- 主代理的编译 02 暴露了前次静态审查未检出的 `TradingPlan` 私有字段 caller 遗漏；原 caller 实施者补迁 `continuous_tick_transaction_tests.rs` 和 `pre_open_transaction_tests.rs`，本复核者没有参与修复。
- 本次重新完整阅读两个文件相对同一基线的最新 diff，并利用前次保留的完整 diff 重建已复核文本，逐项比较此次新增修改；截断输出已补读。新增内容仅为 `TradingPlan::active_child_order_id()`、`filled_qty()`、`status()` 的读取及相应格式调整。
- 依据源码：`plans/mod.rs:338` 的 `PlanBook::plan` 返回 `&TradingPlan`；`plans/state.rs:214`、`:230`、`:258` 三个 getter 分别返回原 `filled_qty`、`status`、`active_child_order_id`，不改变状态、不推进时钟、不产生新校验或错误。
- `session.rs:480` 的 `SaveSlot.plans` 仍为 `PlanBook`，因此 `first.plans.plan(...)`、`saved.plans.plan(...)` 的返回对象同样必须使用 getter。这是存档中已有对象的 API 适配，未改变 SaveSlot 序列化字段或 Snapshot DTO 的公开字段。
- 已核对原子回滚、拒绝替换保持旧 child、重复 Submit 业务等待、同一 child 部分成交后存档恢复、50 股余量不增挂买单、同账户两股续行及 PreOpen 拒单不安装 child 的断言。原来的 Some/None、100/200/350 股、Completed、同一 OrderId 与资源回写断言保持原值；Snapshot 中 `.positions[code].qty` 等 DTO 读取继续保留字段访问。
- 结论：本次收口符合必要接口迁移范围，没有新增大 A 语义漂移、断言削弱或其他有效发现；此前遗漏的 caller 编译适配已从静态源码闭环复核。本复核者仍未运行 Cargo、编译或测试，不能据此报告编译 02 的重跑结果。

## 编译 03 后的 Position 事实比较最终复核

- 再次全文阅读 `session_execution_transaction_tests.rs` 当前源码和相对基线的整个 diff，核对机构 Fill 用例的最终修复。未参与修改产品文件，也未执行 Cargo、编译或测试。
- 最终机构用例直接比较 buyer 的 information、watchlist、price_memory 与另一 participant 的四成员，经验真实更新、两腿收据、成交身份、价格与 T+1 断言继续保留。序列化比较改为类型事实比较后，没有丢掉原来验证的成员。
- Position 没有 PartialEq；新增 `position_facts` 将每个 StockCode 映射至 `(qty(), t1_locked(), invested_cents(), recovered_cents())`，同时保留完整股票 key 集。`account.rs:609-618` 的 Position 正好只有这四项事实，因而该比较没有遗漏账务字段、改成单纯长度比较或削弱断言，也没有为测试扩充生产 derive。
- 原 4 个错误/空簿安装用例及 helper 的 buyer/真实 adapter 适配没有再扩大；没有新的交易规则偏差或有效发现。S1 与最终编译修复从静态源码复核闭环，已可交给主代理进行执行验证。
- 最终文件：`packages/engine/src/session/pipeline/session_execution_transaction_tests.rs`；Git blob SHA：`5a0d761a72a96b43630c383231d94cad48a0ed18`；SHA-256：`06b675be990c1e0e2eb5661a942022d769f5ba03264618122077b94f7a597d84`。这两项是已复核文件指纹，不是编译或测试成功证据。

## 最终 SHA 绑定门禁

- 绑定人：`/root/implement_pipeline/review_stream`；本人没有实施本批产品或测试修改，只有本具名复核记录与复核 manifest 的写入权限内操作。本轮没有由 manager 代写或刷新复核结论。
- 对 32 个实际改动 caller，用最初保留的完整 diff 和基线源码重建最初已审文本，再与最终当前源码逐文件比较。差异仅出现在已完成编译 02 getter 收口的两个测试文件，以及已完成 S1/编译 03 再复核的机构测试文件；其余 29 个 caller 与最初已审文本逐字一致。机构测试的最终 SHA-256 仍与上节绑定相同，没有新的漂移。
- 对 `stock_stream.rs`、`stock_stream/auction_tests.rs`、`local_admission.rs` 再核对当前完整 diff，未见超出原独立复核范围的新改动；后两者 Git blob 仍为初审 diff 中的 `475f34e`、`978fd53`。本轮绑定的是实际当前内容，不拿历史基线 SHA 冒充当前审查。
- `review-stream-callers-manifest.json` 列出 35 个实际变更文件的最终 SHA-256、Git blob、基线 blob、单文件完整 diff 指纹与具名 review 记录链接。完整最终 diff 与最初 caller diff、getter/S1 实际增量证据保存为同目录 review 工作文件，可离线复核绑定关系。未改动的 `stock_stream/root_ready_tests.rs` 另列为已读的所属测试源码证据，不混入 35 个变更文件计数。
- manifest 发布前再次读取所有文件，逐项核对 SHA-256 没有采集期间漂移；失败应显式停止而非自动替换已审 SHA。此清单仅确认静态独立复核覆盖，主代理的运行验证由其自己的证据负责，本复核者没有运行或代签 Cargo、编译、测试结果。
- 结论：具名静态复核及有效发现修复闭环覆盖 manifest 中的最终当前 SHA；无未解决发现，ready。
