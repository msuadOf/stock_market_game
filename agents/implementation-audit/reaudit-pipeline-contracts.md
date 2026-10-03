# Session/pipeline 重构契约复核

## 范围与判定

- 对照已完整阅读的 [R02](coverage/r02.md)、[R10](coverage/r10.md)、[R15](coverage/r15.md)，沿当前 worktree 中 `GameSession`、阶段事务、protocol、存档恢复的正式 caller→state owner→consumer 链核查 P0/P1、玩家受理、失败事务、结算、日界、提交和存档。
- 本次只读检查源码与 `b89afb3..8cf34a1` 的 Session/pipeline 相关 diff，不运行测试/构建/网络操作，不修改游戏代码。对改动按旧实现→新 state owner→生产消费者核对。
- 判定口径：实现链存在不代表本轮测试通过；仅将有生产入口和实际状态消费者的行为算作实现。

后续 `8cf34a1..dddcc31` 中，CapturedExperienceObservation改为持有已经独立复制、观察后的Arc，完成阶段只移交它；原观察、风险计算及错误顺序保留。symbolic Highest测试改为先确认Sell实际受理后提交Buy，并增加反向受理负控；生产受理顺序没有被改成入队排序，K7完整artifact比较也未因此修复。该范围源码及断言已静态核对，未运行Rust测试；`ebfb68b`产品代码相同。

## 核查结论

**没有确认本次复核范围内存在从既有 Session/pipeline 契约遗失的生产功能。** 当前所有阶段仍由 `GameSession::step` 进入唯一的 `execute_authoritative_tick`，按阶段构造事务候选；成功后由 P9 对 authority 做单次 infallible swap。连续交易路径已显式串起 P0 后候选状态、P2 决策资源封存、就绪请求/P3 校验/P4 股票消费、P5 汇总与收尾、P6 结算、生命周期投影和 P9 提交。集合竞价/收盘路径也将 receipt aggregation、settlement、日界转换及事件 seq 写在同一候选事务中。

| 契约 | 当前正式调用链、所有者和消费者 | 判定 |
|---|---|---|
| P0/P1 占款与释放 | `GameSession::step` → 阶段 `plan_tick` → `plan_expiry` 在私有 `TickShadowPlan` 上令到期 NPC 报价生成 release receipt/event 并从 ledger 移除 → P1 `DecisionResourceSnapshot::seal` 从 live envelopes 汇总现金/股份预留，校验账户可用资源。见 `packages/engine/src/session/failure.rs:70-77`、`pipeline/quote_expiry.rs:86-187`、`pipeline/decision_resources.rs:74-124`。 | 生产链可见；资源视图和真实 live reservations 同步在候选状态上维护，未见绕过 P1 使用过期 authority 数值。 |
| 玩家/ NPC 受理及 P3/P4 | `enqueue_player_intent` 验证账户存在且类型为 Player 后存入 authority `pending_player`；连续事务在候选中捕获 NPC/player roots，随后 `ReadyStockStream` 将已就绪请求交 AccountValidatorDriver，再把通过校验的操作分派给股票 shard。见 `session.rs:2518-2532`、`pipeline/ready_ingress.rs:34-49`、`continuous_tick_transaction.rs:188-234`。 | 已接正式入口；受理不是只存在于 helper，订单事实由 P3/P4 和后续投影消费。未把来源标签提升为交易优先级。 |
| 失败原子性与 authority | 连续准备执行 `plan_tick`、完整影子交易、提交前校验后才得到 `PreparedTickPlanCommit`；candidate 准备失败不会执行 `commit_tick_shadow`。P9 先验证 candidate、receipt cursor 和账本 rebase，再以单一 infallible swap 替换 authority。`GameSession::step` 对致命错误记录 poison；protocol 层另对帧/发布失败回滚完整 checkpoint。见 `continuous_tick_transaction.rs:96-112`、`candidate_commit.rs:45-99`、`failure.rs:50-88`、`protocol/civil/session.rs:225-244`。 | 未见提交前局部状态写回 authority。`ProtocolSession` checkpoint 回滚会撤销失败调用内产生的 poison，允许从原健康快照重试；这是有明确测试意图的外层发布事务语义，不等同于直接 `GameSession::step` 的 poison 行为。 |
| 清算与结算 | 成交 worker 输出经 `apply_session_receipt_transaction` 聚合，然后 preceding receipts 与本 tick receipts 合并交 `apply_session_settlement_transaction`；账户 patch、retail/belief projection 和 seen cursor 先准备，全部成功后一起写入候选 owner。竞价收尾 caller 见 `auction_day_end.rs:1080-1113`，连续 caller 在 `continuous_tick_finalizer.rs`，事务实现见 `account_settlement.rs:41-63,153-224`。 | 已接生产消费链；结算不是孤立 API。买卖双方由收据 envelope/account/stock/side 表达，再按同账户同股顺序投影；失败由外层丢弃 candidate。 |
| 收盘清理与交易日界 | auction transaction 先设置候选 tick/seq，再调用 `TradingDayEndTransition`。其验证无 live order/envelope，按配置解锁 T+1 股份，结束 plan、清空父单与 NPC 生命周期，提交日 K、推进交易日并清空分钟线，最后发 `DayBoundary`。见 `auction_day_end.rs:1152-1183,2126-2183`。连续末 tick 同样由 `continuous_tick_finalizer` 调用该转换。 | 日界迁移仍消费实际候选状态；未见只发日界事件而遗漏解锁、历史提交或活动订单清理。自然日 `CivilUpdate` 与交易日 `DayBoundary` 分层。 |
| 公共帧、日终存档 | `ProtocolSession::step_frame` 在发布前建立 checkpoint；验证通过后才追加 facts/intraday。自然日日结更新成功后取得 game save，并登记 `day_end_save`/pending candidate；发生错误时回滚 checkpoint。`GameSession::save` 拒绝 poison 会话；restore 重新校验并恢复 pending intents、NPC batch、订单簿和 v2 runtime。见 `protocol/civil/session.rs:225-244,310-330`、`failure.rs:80-88`、`session.rs:2919-2921,2943-2984`。 | 调用边界符合既有公共日终档/内部 checkpoint 区分；恢复 owner 是新建后经验证恢复的 `GameSession`，不是以默认状态静默补齐保存事实。 |

## `b89afb3..8cf34a1` 差异复核

- `session.rs` 将逐字段持有的可提交数据收拢到 `CommittableSessionState`；旧的 `information`、`belief_books`、`watchlists`、`price_memories` 映射为 `belief_participants`，旧日K历史/活动日K映射为 `SessionCandleBook`，attention heap 映射为 `NpcAttentionScheduler`。`clone_for_shadow` 与 `commit_from` 均覆盖 state 声明的交易、队列、结算、游标、个体状态、市场历史和日历字段；确认没有因 owner 重组而从 tick shadow/P9 swap 漏字段。对照旧版 `clone_for_tick_shadow`/`commit_tick_shadow` 的同一批状态字段。
- `pipeline` 中多数访问变更是旧 authority 字段改为 `session.state.*`。算法型改写也按消费点复核：NPC lifecycle helper 提取到 `quote_expiry.rs`，P0 仍同样校验/应用 release receipt 并清 live order；连续股票 round 封装为 `ContinuousStockRoundProcessor`，保留 P3 identity、reject/receipt/terminal 应用及 complete evidence 校验；竞价股票 round/finish 变为 `AuctionStockShadow` 消费方法，最后仍在 drain 后执行统一尾段。
- `ContinuousDayEndLifecycleProjection` 和 `TradingDayEndTransition` 拆分原连续/竞价收尾逻辑。对照旧 `finalize_trading_day` 与日界调用顺序，仍然先在候选上清算、产生 release、核验无活动委托，再解锁 T+1、清理 parent/NPC lifecycle、提交日K、推进 day、清分钟线并产出单个 `DayBoundary`。新添的日界边界测试源码覆盖阶段边界、残留订单拒绝、溢出回滚和单次 candle boundary（`auction_refactor_tests.rs`）；未运行。
- `ReceiptSettlementPlan` 将原 `prepare_receipt_settlements` 拆为“记录实际 charged Fill”与“按账户并行准备”；仍跳过非 Fill/零量 Fill，按账户/股票 Buy-before-Sell 调 `apply_settlement`，全部成功后交出 patch。`account_settlement.rs` 将个人 BeliefBook patch 合并回已有 `BeliefParticipantState`，不替换同账户 watchlist、price memory、information state。
- `ProtocolSession` 将原来分散的 game/intraday/fact cursor/day-end save/runtime publication/checkpoint map 收拢到 `ProtocolState`。`try_clone_for_checkpoint`、`rollback` 保留完整 bundle；新增源码测试覆盖发布游标、保存候选冻结、关闭/周末 rollback、帧失败回滚和重试。
- 重要错误归因更正：重复 NPC quote lifecycle 的 panic **并非本重构新引入**。在 `b89afb3:packages/engine/src/session.rs` 的 `register_npc_order_lifecycle_at_quote` 中已有相同全局 `order_id` 检查与 panic（旧版约 `:2600-2608`）；区间内只是移入 `NpcOrderLifecycleBook::ensure_order_absent`。因此不把它列为该 diff 新缺陷，也不由 panic 的存在推断用户输入可触发。当前正常调用由 session 自增 order ID 的受理链提供身份；这里只认定该 invariant 检查被保留。
- `packages/engine/src/session/persistence.rs`、`persistence/v2.rs`、`snapshot.rs` 相关变化核对后，核心存档字段验证、quiet-point live-envelope/receipt cursor 校验、完整个人状态恢复和账户/市场订单簿交叉验证均迁入新的 state owner；`CumulativeFeeAuditV2` 将原 fee validator 改为具名对象和方法，边界公式保持一致。Web/API/公共存档语义没有在所查 engine 区间看到契约删除。

## 未确认缺失；保留的验证边界

- 未发现与上述核心契约对应的已确认生产缺口，因此不新增 G/Q 编号，也不建议在此审计记录中将“测试尚未运行”写成实现缺失。
- 既有 R02 提出的“多根计划先产生私有进展、后轮 checked overflow、确认 authority/hash/游标/事件不变”短集成场景仍是有价值的失败边界证据；源码中的统一 candidate/P9 回滚结构本身不能替代该具体场景断言。
- R15 已指出的正式 step 中“全成单撤单→typed `OrderAlreadyFilled`→同计划反馈”及“撤单成功后重复撤同 ID”的生产消费边界覆盖不足，本次不重复登记为新发现；这属于测试覆盖证据边界，不是已证实的实现断接。
- 该区间没有确认出新增的 P0/P1 或事务性生产缺口。差异复核仅确认重复 lifecycle panic 属于旧有 invariant 检查并已原样迁移，不视作本轮可由用户触发的新增缺陷。

## 大 A 与提交审计边界

- 所查代码路径涉及交易阶段、T+1 解锁、委托生命周期和日界状态，但本复核未对交易所规则做新的外部核验；沿用 R02/R10/R15 已记录的 ADR/交易规则边界，不据此扩张真实清算承诺。
- 本报告针对指定区间内与 R02/R10/R15 所列 Session/pipeline 契约相关的 engine diff；没有把区间内与这些契约无关的改动扩张成实现审计结论。未运行 diff 涉及的测试。
