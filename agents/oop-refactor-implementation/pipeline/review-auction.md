# Auction 批次独立复核

- 复核日期：2026-10-03。
- 复核者：`review_auction` subagent；未参与本批实现。
- 基线：`b89afb3346743a4b4fccf26c9ac9ff108595f696`。
- 权威范围：`assigned-actions.json` 中 `engine-pipeline-03-A02/A03`、`pipeline-N02`、`pipeline-R2-N02`，以及 A02 的受理语义修订 `pipeline-R2-E01`。
- 已逐段读至 EOF：`auction_day_end.rs`（2283 行）、`auction_tick_transaction.rs`（325 行）、`auction_day_end_tests.rs`（1267 行）、`auction_tick_transaction_tests.rs`（1010 行）、`incremental_auction_round_tests.rs`（320 行）、未跟踪的 `auction_refactor_tests.rs`（339 行）。逐段读取基线 `auction_day_end.rs` 至 EOF，并核对其余 tracked 文件完整 diff。
- 另查真实接线：`stock_stream.rs::finish_auction_shards`、`continuous_tick_finalizer.rs` 的日终调用点，以及 `ParentOrderPlan` checked 方法、`record_auction_fill`、`apply_auction_price`、`PlanChainFactConsumption::contains_operation`。
- 本复核未执行 Cargo、编译、测试或外部网络核验；没有编辑产品文件。

## 最终门禁新增源码覆盖证据

2026-10-03 按最终门禁要求重新以 `cat` 完整读取新增未跟踪源码 `packages/engine/src/session/pipeline/auction_refactor_tests.rs`，从首行 `use super::*;` 读至第 339 行最后一个测试的结束括号与 EOF；工具输出未截断。当前文件共 **339 行**，SHA-256 为：

```text
a7b268ab50c16f13cf35bc8825eff96e8e3d6aa2abc9a968804f7471460f90e3
```

本次完整复读覆盖全部 9 个 `#[test]`、共享 `session_at` fixture 及全部断言，与前述 boundary、live 状态、candidate 部分进度、lifecycle buffer 和 DayBoundary/plan failure 结论一致，未发现新增有效问题。此 SHA 绑定本次完整阅读的源码内容；此后若文件变更需重新复核。此次补证仍未执行 Cargo 或测试。

## 最终六文件内容绑定

2026-10-03 最终证据门禁由同一独立复核者 `review_auction` 亲自完成。此前五个 tracked 文件没有单独持久化 SHA，不能仅凭当前行数相同推定没有漂移；本次重新分段读取这五个文件相对上述 baseline 的**完整最终 diff**（合计 3153 行，读至最后一个 diff 的 EOF，没有截断），与已全文读取至 EOF 的源码及前次行为结论对照。未发现新增有效问题。新增 untracked 文件的当前 SHA 与前次完整复读签定的 SHA 一致，其 339 行全文与 9 个测试的阅读证据见上一节。

最终当前内容绑定如下，机器可读映射见 [review-auction-manifest.json](review-auction-manifest.json)。每个 SHA 由本复核者读取文件原始字节计算，绑定本记录而非 manager 代签；文件后续变化时本绑定失效。

| 文件（目录均为 `packages/engine/src/session/pipeline/`） | 行数 | SHA-256 |
| --- | --- | --- |
| `auction_day_end.rs` | 2283 | `306eb1faec499f99a1fab984eb2058951c17f3eb344cc8a3bcc44f1652fe7d21` |
| `auction_tick_transaction.rs` | 325 | `b0cf42e1a3f896857ed758c1980e06d1bd4e6f7c494e30d8bfbe042b51cc7f93` |
| `auction_day_end_tests.rs` | 1267 | `71ea0e23f83ce00eb258b03fd17b0972c151a592e909c26ac5e11d9f4e1bfb73` |
| `auction_tick_transaction_tests.rs` | 1010 | `97fc4addd525b1a62ab62e63c69194b118612d5244ceda6a47e3c34da26f278c` |
| `incremental_auction_round_tests.rs` | 320 | `03ebf581447434c40e5c50bb9c2e1aa9730d28f46b704ecef2b53f87f85c21f8` |
| `auction_refactor_tests.rs` | 339 | `a7b268ab50c16f13cf35bc8825eff96e8e3d6aa2abc9a968804f7471460f90e3` |

本次补证只修改本记录与 manifest，不修改产品源码或测试，不执行 Cargo/测试。主任务报告已完成运行验证；对应结果由其验证记录负责，本复核者不将该结果署为自己的运行结果。

## 结论

本批静态审查未发现需要修复的有效问题。不能将此结论表述为测试通过、完整 runtime 验收通过或整个并发受理架构已完全落实。

## 大 A 语义与依据

已读仓库 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`、`docs/architecture.md`，以 [ADR-0017 顶部修订](../../../docs/decisions/0017-escrow-parallel-tick.md)、[ADR-0018 §7](../../../docs/decisions/0018-long-running-immutable-timeline.md#7-交易约束撮合顺序与并行) 和 [trading-rules.md](../../../docs/trading-rules.md) 为现行项目契约。不能使用 ADR-0017 历史来源类序或 sealed index 作为交易优先级。

`trading-rules.md` 引用沪深交易所 2026 年修订规则（2026-07-06 施行），记录阶段、撤单窗口、数量及集合竞价条款于 2026-09-22 核对；文档明确中国结算费用表当次访问返回 HTTP 404，过户费依据沿用 2026-09-08 记录。本次未重新在线核验，未修改制度、费率或清算算法，因此不新增“官方已再次核验”的声明。深市竞价仍统一以昨收参照、卖方费用按成交所得封顶等现有游戏简化继续以正式文档的标注为准。

- `AuctionStockShadow::apply_round/apply_operation` 保留传入操作先后及原 `AuctionOperation` 的到达序；没有按 candidate key、OrderId 或 sealed index 重新排序。更新注释明确 sealed identity 仅关联 typed facts。
- `stage_opening_remainders` 仍按 `arrival_seq` 安装开盘余单，数量、累计成交金额和原数量的计算不变。
- 集合竞价市价拒绝仍为 P4 业务拒绝，消耗已分配 OrderId；拒绝收据释放 envelope，但不回补密封的 P3 预算。09:20 后开盘撤单和全部收盘竞价撤单仍交给原 phase 状态机拒绝。
- `process_auction_stock` 的沪深配置、竞价完成、实际费用 receipt、清簿及日终 Release 路径不变。`Market::apply_auction_price` 仅写原先 `set_last_price` 写入的 `last_price`。
- `apply_finished_candidate` 仍在统一 ReceiptAggregation、Settlement 后投影 lifecycle；日终时才调用 transition。`TradingDayEndTransition::apply` 保留 T+1 解锁相对 Settlement 和 plan sweep 的位置，不提供本 tick 再交易入口，也没有改变 `t1_enabled` 既有语义。

## 必要性与范围

| 动作 | 审查结果 |
| --- | --- |
| A02 | 操作、轮次及 consuming finish 移到拥有 shadow 状态的 receiver；coordinator 继续负责分区、Rayon 执行、首错选择和统一安装。没有新服务、持久状态或 API。 |
| A03 | projector 拥有 parent 克隆、pending 和 retail 缓冲区；成功后一起写回。空转的 `push_pending_plan_event` 被直接追加替代，未扩大职责。 |
| N02 | 私有字段的 checked `AuctionTickBoundary` 表达三个派生事实；生产两次 capture 保留原位置，`finish_auction_shards` 解包值给原底层 API。 |
| R2-N02 | 借用 private candidate 的短命日终 receiver；两个 production finalizer 保留原调用点。没有缓存第二份权威账户或 PlanBook，没有提前计算 next day。 |

`session.state`、Parent/Plan 的 getter 和 fixture 构造迁移属于相邻权威字段封装的必要 caller 接线；逐个核对后没有发现字段含义、单位或原测试断言的改变。删除旧日终自由函数后，Continuous caller 已改为 transition；未发现遗留旧函数调用。静态接线检查不能替代最终编译门禁。

## 错误顺序、部分写入与边界

- coordinator 仍先查 identity，再 checked 操作计数，再构造 worker；按 StockCode 对 worker result 排序选择首错，结果交付扰动不影响选择。stock 更新、detached facts、seen identity 和 count 都在全部 worker 及 round identity 校验成功后安装。
- shadow receiver 的校验、created envelope 写入、价格解析、业务拒绝、receipt 和 lifecycle 追加顺序与基线一致。失败只丢弃该局部 shadow，finish 仍消费所有权。
- boundary 的 phase、tick overflow、结束日必须同时结束竞价、day overflow 检查保留原错误类型、文字和 location；生产第一次 capture 仍在命令流排空后，第二次仍在 finish 投影入口的 order cursor 检查后。
- Parent checked submission/fill/cancel 方法保持基线的先后检查和错误文字；fill 在全部数量检查成功后才改本地 parent。`contains_operation` 保留同一身份集合判断；没有再次投影 continuation 已消费的 Accepted/Canceled。
- parent/pending/retail 缓冲区仍在全部 lifecycle 检查后安装。simulation diagnostics 仍可能先修改可丢弃 candidate，这是基线行为，并非新 projector 内部事务承诺；因果 fill receiver 保留 gross checked subtraction、tracked value 对账、写 tracked、记录 Filled 的顺序。
- 日终仍先拒绝 live order/envelope，再 T+1 unlock，再 `mem::take` 与计划同步/sweep，随后清 parent/NPC、提交日 K、checked day+1、清分钟历史、产生一个 DayBoundary。计划失败仍可能留下已解锁且 plans 被 take 的 candidate；day overflow 仍发生在日 K 提交后；外层丢弃 candidate 保持 authority 原子性。没有把这些局部部分写入误称内部回滚。

## 测试审查与限制

三个原测试文件的完整 diff 没有删减或弱化断言：变更为字段、getter、fixture 和 receiver 接线，注释中文化不改变断言。原用例继续覆盖余单 FIFO、撤单窗口、市价拒单预算、跨股事实重排、replace 先撤后下、worker 首错、后轮失败及 authority 各状态族回滚、竞价 completion/fill 恰一次和收盘 partial fill/DayEnd。

新增测试静态覆盖 named boundary 四种正常位置、phase/tick/day 首错、三种 live 状态前置拒绝、day overflow 的原 candidate 部分进度、重复 lifecycle identity、零/倒退 Fill、child 数量不匹配的 parent/pending 缓冲区丢弃、跨 phase DayBoundary 一致，以及 plan 同步错误先于 day overflow 且不改变 authority。

仍需主实现者完成本批适用测试与编译门禁。新增跨 phase 测试直接调用共同 transition，属于 receiver 行为证据；它本身不等于两条完整 production 路径的 runtime 验证。范围内未发现因本批迁移新增而必须补的边界缺陷；T+1、费用和 diagnostics 的完整执行结果仍应由对应现有套件验证，不能由本静态复核推定通过。
