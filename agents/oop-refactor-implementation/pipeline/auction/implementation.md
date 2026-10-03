# Auction 实施记录

- 基线：`b89afb3346743a4b4fccf26c9ac9ff108595f696`。
- 负责动作：`engine-pipeline-03-A02`、`engine-pipeline-03-A03`、`pipeline-N02`、`pipeline-R2-N02`，以及 `pipeline-R2-E01` 局部受理语义说明。
- 当前状态：实现及精确 rustfmt 完成；cargo 编译、产品测试及独立复核由 root 统一执行，本子任务没有运行或声称通过这些门禁。
- 新增测试先于实现写入，但遵守 parent 禁止启动 cargo 的指令，未取得运行失败的红灯证据。不得把类型缺失或静态阅读当作 TDD 红灯。

## 方法与 caller

| 旧入口 | owner 方法 | 真实 caller / 责任 |
|---|---|---|
| `apply_auction_stock_round` | `AuctionStockShadow::apply_round(self, operations)` | `IncrementalAuctionStockCoordinator::apply_round` 的 Rayon worker；两首错测试直接调用 |
| `apply_auction_operation` | `AuctionStockShadow::apply_operation(&mut self, operation)` | shadow 的 apply_round；本股 ledger、completion、receipt 与 lifecycle 同步变更 |
| `finish_auction_stock` | `AuctionStockShadow::finish(self, tick_after, finish_auction, finish_day)` | coordinator 的 consuming finish；保持最低 worker tail 参数与 guard |
| `validate_incremental_operation_identities` | `IncrementalAuctionStockCoordinator::validate_identities(&self, operations)` | coordinator apply_round，保持重放与序列化 identity 首错 |
| `apply_auction_lifecycle_projection` | `AuctionLifecycleProjector::apply(self, candidate, workers, facts, receipts, finish_day, consumed)` | apply_finished_candidate；成功后统一安装 parent、pending plan 与 retail 缓冲区 |
| `clear_parent_child` | `AuctionLifecycleProjector::clear_child` | operation cancel 与 DayEnd cancel 投影；具体 child 不变量委托 ParentOrderPlan checked 方法 |
| `push_pending_plan_event` | 移除，直接追加 projector.pending | 没有独立 fallible 行为，保持缓冲追加先后 |
| `auction_tail_boundaries` 及测试 seam 同构公式 | `AuctionTickBoundary::capture(&GameSession)` | transaction 在 stream 排空后 capture；prepared finish 位置再次 capture；两个测试 seam 同步使用 |
| `finalize_trading_day` | `TradingDayEndTransition::new(candidate).apply(self)` | 本文件 apply_finished_candidate；continuous_tick_finalizer 由 sibling 在原位置迁入 |
| `checked_sweep_decision_chain_day_end` | `TradingDayEndTransition::sweep_owned_plans` | consuming apply；保持 mem::take、同步、active plan sweep 与写回失败时点 |

`AuctionTickBoundary` 为 Copy、字段私有；getter 为 `tick_after/finish_auction/finish_day`。
`stock_stream::finish_auction_shards(shards, boundary)` 的 caller 接线由 sibling 实施。
`for_test(tick_after, finish_auction, finish_day)` 仅在 cfg(test) 下可用，检查 `finish_day ⇒ finish_auction`，不进入生产 API。

## 保留的语义

- `ADR-0017` 顶部 2026-09-24/25 修订及 `ADR-0018 §7` 是本次受理语义基线：每股实际局部受理及适用价格时间规则决定冲突；sealed identity 只关联 typed facts，输出排序不形成交易优先级。
- 不增加或修改交易制度、费用、配置、存档格式或证券类别模型。竞价撤单窗口、market rejection、开盘余单 FIFO 与收盘 DayEnd 沿用现有算法。
- `docs/trading-rules.md` 记录官方规则内容最近核对日期 2026-09-22、同股受理规则复核日期 2026-09-25；2026 版沪深交易规则自 2026-07-06 起施行。本子任务没有重新联网访问或宣称规则当日复核。
- coordinator 本轮暂存到全部 worker 与 identity 校验成功才安装；worker 首错按 stock identity 选择，Rayon 分区和本股操作传入次序不变。
- 日终保持清簿/envelope 检查 → 按 setup 解锁 T+1 → plan 同步/sweep → 清 parent/NPC lifecycle → 提交 candle → checked day+1 → 清 minute history → DayBoundary。
- TradingDayEndTransition 不提供内部逐步 rollback；day overflow 前的 T+1 解锁和 candle 提交仍留在 discardable candidate，由外层丢弃 candidate 保护 authority。
- diagnostics 对 candidate 的写入仍发生在原时点；projector 统一安装的范围是 parent、pending plan、retail 缓冲区，没有扩大内部原子性承诺。

## 跨簇兼容

本簇六个文件的 GameSession 字段读取/写入迁至 state；不改变 facade 方法。
ParentOrderPlan 构造、getter、checked submission/fill/cancel；TradingPlan getter；CandleBook 读取；Account fixture/getter；Market apply_auction_price；PlanChainFactConsumption.contains_operation；CausalCollector.record_auction_fill；NpcOrderLifecycleBook.clear 均已迁入对应 owner API。

## 验证台账

已执行：只对六个授权文件运行 `rustfmt --edition 2021 --config skip_children=true`；授权 tracked diff 的 `git diff --check` 通过；静态检查未发现本簇遗留 GameSession facade 字段直读。

待 root 运行的短 lib filter（默认及 `simulation-diagnostics` 两组；每条普通测试命令 10000ms deadline，测试线程显式多核）：

- `auction_refactor_tests`：9 例新增边界，phase/capture 首错、三类 live order 前置、day overflow 部分变更、plan 同步错误顺序与 authority 隔离、phase 共用 DayBoundary、duplicate lifecycle identity、zero/regressing fill、child quantity mismatch 缓冲区回滚。
- `auction_day_end_tests`：保留竞价受理/撤单、余单 FIFO、same-tick plan/retail、partial fill/DayEnd、多个失败层 authority rollback。
- `auction_tick_transaction_tests`：保留真实 transaction、market 拒绝预算、continuation、cross-stock fact reordering 与完整 tick rollback。
- `incremental_auction_round_tests`：保留 identity 重放、跨股 sealed identity 乱序、detached unknown stock、worker 失败后重试与两个 worker 首错。

独立复核：待 root 安排未参与实施的 reviewer；本子任务没有派新代理。
