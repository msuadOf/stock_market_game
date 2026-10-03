# 隐藏扫描批次 159（owner 4）

## 来源与完整性

- 按 scan-plan，本批指定 3 篇历史模块评审；均已从主工作区首字符连续读至 EOF。计划路径、SHA-256 与行数实测完全相符：共 214 行，详见配套 `batch-159.json`。
- 来源基线标记为 `43b1aa5`。本轮按任务未执行 Git 命令；因此只确认当前来源字节与计划 manifest 相符，不独立验证该 baseline commit 中的历史字节。
- 本轮只读当前代码与 implementation-audit 证据，没有运行测试、构建或外网交易规则核查；未修改产品文件、Git 状态或 G/Q 总账。

## 历史来源与当前调用核对

- `engine-pipeline-09.md` 将自身标为“候选设计，未实施”，主张保留 NPC state projection、P0 expiry、ReadyIngress/ReadyStockStream、PreOpen transaction、价格决议和 receipt aggregation 的既有边界。当前代码里，`GameSession::step` 的生产路径消费 quote expiry、NPC projection、就绪流及阶段事务；PreOpen、连续交易和集合竞价 transaction 均真实构造 `ReadyStockStream` 并调用 `drive_stock_stream`。来源文本描述的 shadow/P9、P0/P1 资源截点、typed 身份关联与非优先级身份边界与当前 pipeline contracts 复核一致。历史所列测试范围仅是来源声称读到的用例，本批没有运行测试或重新核对其完整测试清单。
- `engine-pipeline-11.md` 也属于未实施的候选评审。其唯一生产候选 `StockStreamCoordinator<S>` 在目标基线中已由 `stock_stream.rs` 实现：`drive_stock_stream` 创建该 coordinator，后者持有每 tick 的 available/pending/in-flight、通知及结果接收状态。连续、竞价、PreOpen 事务是当前调用者；finish/finalizer 仍在 stream drain 后由阶段 transaction 执行。该设计没有把 coordinator 提升为常驻 actor，也没有赋予输出身份交易优先级。故不能将历史“候选未实施”转述为当前缺实现；也不把已实现的局部候选当作全 pipeline 验收完成。
- `engine-session-01.md` 中唯一明确迁移候选 `CommittableSessionState` 亦已实现：当前 `GameSession` 持有单个 `state`，`clone_for_tick_shadow`/`commit_tick_shadow` 委托状态容器的 shadow clone 与提交；poison 和测试故障注入仍在 facade。当前 caller/owner 复核还确认 `TickShadow`/`TickShadowPlan` 继续承担候选及 outbox，`enqueue_player_intent`、`civil_clock_mut` 与独立 `end_civil_day` 事务保留各自权威入口。不能将 P9 描述为所有 Session 变更的唯一入口。`AccountBook` 仍由 GameSession state 持有并承担页级 COW/cache；未见来源所述 candidate 导致交易顺序或 A 股单位漂移。
- 当前主要调用证据：`packages/engine/src/session/pipeline/stock_stream.rs:271-301`；阶段 consumers `continuous_tick_transaction.rs:218-234`、`auction_tick_transaction.rs:201-218`、`pre_open_transaction.rs:227-245`；NPC/P0 consumers `npc_tick_preparation.rs:66-106`、`quote_expiry.rs:86-124`；状态 owner 与 P9 说明见 `session.rs` 及 `agents/implementation-audit/reaudit-pipeline-contracts.md`。

## 语义、总账与范围

- 三篇来源没有提出新的 A 股规则。本批只核对现有描述与 ADR-0017、`trading-rules.md` 已记录的边界：现金以分、数量以股，P0 到期释放先于 P1，撤单依赖关系不等同于身份优先，实际同股受理顺序仍由入口事实决定。未重新查询官方规则，不能把历史来源中的规则摘要当成本轮独立的官方核验。
- 当前静态消费者核对未发现历史方案引入了未接线生产对象，或旧边界在当前调用链中丢失。`StockStreamCoordinator` 与 `CommittableSessionState` 候选现在已有真实 owner/caller；这些局部实现不足以关闭实现审计中的相关全局缺口。无新 G/Q 候选，未更新总账。
- 结论仅覆盖三篇指定历史评审及上述关键 owner/caller/consumer 对照；未重做完整 pipeline/session 契约审计、没有执行运行时验证。
