# Pipeline stream/state callers EOF 独立复核

## 基线与阅读范围

- 对象：产品 `08e4fc7`（当前审计 worktree 的 `a7c7ce3` merge 包含该产品提交）；依据当前源码，不将当前提交的文档提交误称为 pipeline 实现变更。
- 全文至 EOF 阅读 `review-stream-callers.md`（77 行）、`state-callers0.md`（44 行）、`state-callers1.md`（71 行）；另核对 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`，并检索实际入口、owner、state clone/commit、hash、调用者和测试反证。未运行测试、编译、长测、Git 写操作或产品文件修改。
- 交易语义按本次代码迁移性质（私有状态/接口适配和执行流协调）评估；没有新交易规则主张，未重新访问交易所法源。正式约束以 ADR 与项目规则为准，不以历史 review 的 PASS 覆盖最新状态。

## 原文章节矩阵与复核

| 原文与行数 | 章节/主张 | 当前代码交叉证据与复核 |
|---|---|---|
| `review-stream-callers.md` 1–20 | 范围、基线、权威动作、35 文件清单及未纳入项 | 当前生产链确有 `ReadyIngress`、`ReadyStockStream`、`StockStreamCoordinator`。`retail_projection_persistence_tests.rs` 属另一批 reviewer 的范围，不可由本篇替代。文中 2026-10-03 基线 `b89afb3` 是历史改动审查基线；目标产品 `08e4fc7` 的后续完整 diff/最终 SHA 需主审 manifest 绑定，本记录不宣称逐个重做35文件完整 diff SHA 门禁。 |
| `review-stream-callers.md` 21–32 | stream 时序、局部受理和领域语义 | `stock_stream.rs` worker 先送 typed completion payload 再发通知；协调者收到 Stock wakeup 后取任一完成 payload、验证 `in_flight`、每股簿只回收一次。两 channel 允许跨 worker 重排，notification 不承载股票身份，不能据此推导交易优先级。PlanRoot wakeup 只重轮询计划，不消费 stock payload。`ReadyStockStream` 对 P4 round 的 candidate key、股票身份与 pending 身份做校验，再送 plan typed outcomes/projection。A股 T+1、费用/单位及阶段由既有 P3/P4/Settlement 保持，本次 state getter 不更改这些规则。 |
| `review-stream-callers.md` 33–36 | 最小范围 | `StockStreamCoordinator` 只持本 tick available/pending/in-flight/channels；不持账户、结算、session authority。P3、plan chain、receipt、settlement 的 owner 留在外围事务，符合窄职责；没有证据显示这次 stream 封装引入共享账户余额或交易总序。 |
| `review-stream-callers.md` 37–51 | S1 belief participant 写回发现、修复和当前结论 | 修复描述可由实际写路径佐证：`session_execution_transaction.rs` 持有 `belief_participants`，沿既有 participant 写 belief；复核测试用非空 belief patch 并比较 information/watchlist/price_memory。旧 review 明确未运行此测试。当前代码链支持其静态结论，不能把源码断言当本轮执行结果。 |
| `review-stream-callers.md` 53–60 | 编译02暴露 TradingPlan caller 后 getter 补修 | `continuous_tick_transaction_tests.rs`、`pre_open_transaction_tests.rs` 的 `TradingPlan` 访问使用 getter；`SaveSlot.plans` 经 `PlanBook::plan` 仍返回 `&TradingPlan`，与正文说明相符。此发现是实际反证：初次静态扫描漏了私有字段 caller，之后据编译信号补迁并复核；不可把最初“无旧访问”结论单独视为充分。 |
| `review-stream-callers.md` 62–68 | Position 事实比较最终复核 | Position 事实核对包含每股 key、qty、T+1 locked、invested/recovered cents，未扩展生产类型 derive。SHA 与 blob 是该记录描述的历史被审文件指纹，不是本次重算值或运行证据。 |
| `review-stream-callers.md` 70–77 | 最终 SHA 绑定门禁 | 该章明确 35 个变更文件有 manifest，32 个 callers、stream/local admission 有增量核对，测试源码另列。本次按用户要求只审三份工作记录及执行链，未重算 manifest 所有哈希；故沿用为上一 reviewer 的具名历史证据，而非本审计的新签署。 |
| `state-callers0.md` 1–14 | 范围与 GameSession/Account/Plan/Candle/attention/participant caller 迁移 | 当前 `GameSession` 将权威事实集中在 `CommittableSessionState`。搜索生产 caller 可见 `belief_participants`、`attention_scheduler`、`candle_book` 由新 owner 访问；`SaveSlot`/Snapshot/receipt DTO 仍是独立契约字段。原文特别说明 `retail_projection_persistence_tests.rs` 转交 projections，属于边界说明，不是 caller 漏迁。 |
| `state-callers0.md` 16–32 | 15 个实际修改文件清单 | 与当时工作批次的范围声明一致；清单没有声称覆盖其他 owner 的文件。后续 `review-stream-callers.md` SHA 门禁将本批总 caller 改动按32/35统计，是不同归属/口径，不应简单拿“15”与“35”判为矛盾。 |
| `state-callers0.md` 34–44 | 语义、格式、未测边界和 getter 补修 | 原文明示未运行 Cargo/测试，且由父任务负责统一验证。独立复核补修精确针对 `TradingPlan` 私有字段；当前源码使用 `active_child_order_id()`、`filled_qty()`、`status()`。检查文中对金额/股数/T+1/Receipt/P9 的断言属于静态迁移主张，不是正式规则重新认证。 |
| `state-callers1.md` 1–5 | 范围、分配清单和依据 | 明示36文件分配、已读架构与 ADR 路径、无新增依赖/规则；属于实施记录，当前三篇 EOF 不提供当时每个文件的独立 diff 证明。 |
| `state-callers1.md` 7–14 | state/getter/fixture/participant/TickShadow 迁移 | `CommittableSessionState::clone_for_shadow` 解构并 clone 权威字段，`commit_from` 将 candidate 的 owner 字段整体替换回 authority；participant、ledger、projection seen、receipt/order/tick/day/seq/civil clock 均在两侧明确出现。`belief_participants` 的复合 owner 与 hash 投影需保持旧四事实成员，不因 getter 适配而改变存档/API 字段。 |
| `state-callers1.md` 16–20 | rustfmt、扫描、无 Cargo/测试声明 | 历史静态验证边界表达清楚；本次也未运行测试或编译。不将“静态扫描未发现”提升成行为已通过。 |
| `state-callers1.md` 22–61 | 最终文件行数/sha 清单 | 这是当时 36 文件范围的静态版本指纹；当前任务只审 EOF 与调用链，未对全部 SHA 重采样。可用作调查导航，不是 `08e4fc7` 的新鲜最终哈希绑定。 |
| `state-callers1.md` 63–71 | belief_patch 缺口的补充测试及未执行声明 | 对真实机构买方及另一个机构进行 participant 成员保留核对，和 `review-stream-callers.md` S1 修复方向互相印证。文中仍明确新增测试待独立 diff 复核及统一执行；`review-stream-callers.md` 后续记载完成静态复核，不代表测试执行通过。 |

## 实际受理、状态 owner 与 hash

1. **受理路径。** `ReadyIngress::capture_sources` 从候选 session detach NPC/player 请求并捕获来源账户；`capture_roots` 装入计划 root 通知与 `AdaptivePlanChainCoordinator`。`first_ready_batch` 组合初始与 plan ready candidates。`validate_available_ready` 经 `admit_ready_batch` 做同账户 Cash 或同账户同股 Shares 资源 lane 偏序、quote replacement 依赖及股票 gate，再给 `AccountValidatorDriver`。非冲突快路径仍执行 receipt observe。它没有把来源身份当作跨账户优先级。
2. **逐股票并发及反馈。** 通过 P3 的 operation 被 `ReadyStockStream` 以 candidate key 暂存，并由 `StockStreamCoordinator` 按股票分批 dispatch。worker 只持单股 shard，调用线程持计划/validator/session。完成时核验 candidate identity、sealed index、股票 key，推动对应计划，刷新未完成路线，再取可立即执行的 ready batch。`finish()` 要求无 pending accepted candidate；外层再 finish plan chain 和 validator，然后 aggregation/finalization/settlement 都作用于事务 candidate。
3. **唯一权威状态。** `GameSession.state: CommittableSessionState` 是影子提交集合；账户、行情/盘口、请求队列、计划、NPC attention、participant、envelope ledger、projection cursor 与时钟均归其 owner。`clone_for_shadow` 对可变集合复制、公司/经营共享只读 Arc；`commit_from` 替换对应状态，未见单独 state owner 在 stream worker 内被提交到 authority。提交入口在候选准备和验证后，不是 worker 完成时。
4. **hash 投影。** `business_state_hash()` 逐字段 hash 确定业务投影，participant 的 information/belief/watchlist/price_memory 分别映射回四个旧语义事实；状态 getter 本身不改字段。`session_state_hash()` 叠 poison 与诊断缓存。Business hash 特意不包含 `last_retail_decisions` / `last_retail_order_events`，而 Session hash 包含；这不是 owner 漏 hash。hash 为非安全 FNV-1a JSON 指纹，不能用作加密完整性或证明自由 worker 调度确定。未发现本次 caller迁移改变 hash 字段集合的证据。

## 旧结论复核、检索反证与遗留

- `agents/implementation-audit/reaudit-pipeline-contracts.md` 对旧实现链的判断（玩家/NPC ingress→P3/P4→settlement→P9）与当前正式 caller 可交叉确认；它也明确其未运行测试、不覆盖部分短边界。它是较早基线记录，不能代替当前提交的完整 diff 或实测。
- 旧 review 的 S1 是有效发现，原因是空 Fill 用例不能证明非空 belief patch 写回保留其余 participant 成员；后续新增真实成交路径和静态再审把这个特定缺口闭环。它仍未提供本轮运行证据。
- 旧 review 的编译02 getter 遗漏也是真实 caller 反证，已经定点补齐并由静态源码交叉确认。它说明 state migration 的错误面在测试私有字段同样存在，需依靠最终编译核对而不止 grep。
- 搜索反证：`AccountReceipt` 仅用在 `local_admission.rs` 就绪批内；生产股票接收处通过 `in_flight` 验证并将 shard 放回 coordinator；`StockCompleted` 被连续/竞价 caller 消费；state hash 对 participant 成员有完整四映射。对 `belief_books`、`attention_queue` 等旧 owner 名称的检索结果属于历史文档、测试或 ADR 时，应逐个确认，不可仅凭 token 命中报漏迁；当前正式字段是 `belief_participants`、`attention_scheduler`。
- **未确认生产缺陷。** 没有在这三篇 EOF 范围中找到实际受理断链、participant 丢失、hash 状态遗漏或本次 A 股语义漂移证据。
- **仍需主审/验证负责。** 当前产品 SHA 完整 diff 的统一 manifest 绑定、最终编译/定点测试执行、其它 OOP owner 批次及 `retail_projection_persistence_tests.rs` 不在本复核范围。这里不替代 AGENTS 要求的全批独立复核，也不宣称任务整体验收完成。

## 结论

指定三份记录已全文读至 EOF。当前 stream 受理者、worker coordinator、单一 `CommittableSessionState` owner、P9 candidate 安装及业务/会话 hash 边界彼此一致；两项历史有效反证（S1 participant 写回测试缺口、TradingPlan getter caller 遗漏）有后续静态修复证据，未发现新的有效生产缺陷或大 A 语义变化。没有重新认证全部官方交易规则，也没有运行测试、编译或全量文件 SHA 清单；本结论仅是当前源码静态复核。
