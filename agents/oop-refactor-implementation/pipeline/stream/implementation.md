# stream 实施记录

## 范围与依据

- `engine-pipeline-11-A01`：`StockStreamCoordinator<S>`。
- `pipeline-R2-N08`：`ReadyAdmissionPlan`。
- 权威范围来自 `../assigned-actions.json` 及 challenge-2026-10-03 的 action-index、relationships。
- 已读取 AGENTS、principles、testing、architecture、open-questions、ADR-0017 顶部修订及 ADR-0018 §7；两个实现文件、所属 root_ready/auction tests 已全文读至 EOF。另读取 ready_ingress 与 ready_stock_stream 的生产关系。
- 本批不改变 A 股数量、金额、T+1、价格时间与交易时段规则；只迁既有私有工作生命周期。原领域规则依据仍来自现有 trading-rules 与 ADR，未声称本批重新认证官方制度。

## owner 与生产调用

| 目标 | owner 方法 | 已迁生产 caller |
| --- | --- | --- |
| StockStreamCoordinator | new、enqueue_ready、dispatch_ready、poll_progress、receive_completion、return_idle、drive | drive_stock_stream 创建实际 owner；continuous/auction transaction 的原调用保持 API |
| ReadyAdmissionPlan | prepare、build_resource_edges、add_quote_dependencies、run_stock_gates、finish_layout | admit_ready_batch 创建即消费实际 owner；ready_ingress::validate_available_ready 原生产入口沿用 |

StockStreamCoordinator 独占 available、pending、in_flight 及 payload/notification channel，callbacks 限定于一次 drive；Rayon in_place_scope 保留调用线程。worker 成功发送 payload 后才通知；通知失败仍忽略，payload receiver 已关闭则放弃私有工作。过时 PlanRoot 只轮询计划，绝不消费股票 payload。finish 与最终结算均不迁入 owner。

ReadyAdmissionPlan 只持有批内 candidates、successors、incoming 与 gates；resource_groups 在 build_resource_edges 后释放。短批/无冲突快路径保留；observe 仍在原扫描位置，后续错误不撤回已有 plan receipt 游标推进。资源边、quote 边、gate arrival 边及 ready.pop 顺序不变。

## 验证交接

本 agent 遵照协调限制没有运行 cargo 或产品测试。先补既有行为 characterization tests，再迁实现；等价重构的这些行为在旧实现本就应成立，不能宣称取得失败断言的 red evidence。新 owner 私有损坏路径测试已补，尚待统一执行。

指定 rustfmt（edition 2021、skip_children=true）与本文件簇 git diff --check 已通过。

短测试 filters，default features（另可由主代理检查 verification-harness 编译）：

- `session::pipeline::stock_stream::tests::`：保留四个原行为测试，新增 notification 缺 payload、notification 断开、payload 断开、过时 root 保留 payload、重复完成、coordinator drop 关闭 receiver。
- `session::pipeline::stock_stream::root_ready_tests::`：两个原 root 通知测试保留。
- `session::pipeline::stock_stream::auction_tests::`：两个原 finish overlap/首错测试；已使用共享 AuctionTickBoundary::for_test。
- `session::pipeline::local_admission::tests::`：原五项保留，新增混合 request phases、错误后 receipt 推进/overflow 先于 quote 错误、现金/异股 shares fork、损坏 cycle、atomic underflow。

统一执行应设置 `RAYON_NUM_THREADS=4`，普通测试每命令与 case 10 秒内；external coordinators 测试依全局 worker 数创建线程，避免无意采用整机预算。所属测试均为短 fixture，不启动完整验收。

## 未完成门禁

- AuctionTickBoundary 已接入 finish_auction_shards 和所属 auction tests；底层 shard.finish 仍按原 tick_after/finish_auction/finish_day 参数拆解，保持一次终结与稳定首错。
- 编译、测试与未实施者完整 diff 独立复核由主代理统一执行；本记录不宣称已完成这些门禁。

## 后续六文件 caller 迁移

按协调者追加任务，全文读取并迁移 adaptive_plan_chain_tests.rs、npc_lifecycle_projection_tests.rs、initial_candidate_round_tests.rs、quote_expiry_tests.rs、quote_expiry_checkpoint_tests.rs、decision_resources_tests.rs。最后一个 checkpoint 文件接任务时已用新 API，核对后无额外语义迁移。

- GameSession 私有事实全部通过 state；attention_queue 改 NpcAttentionScheduler::clear/enqueue。
- Account 读取使用 cash/strategy/positions getters；fixture 赋值使用 fixture_set_cash/kind/strategy/positions 与 fixture_insert_position。
- 原 id 与 AccountBook key 不一致负向 fixture 用 Account::new 显式构建不同 id，复制原 kind/cash/positions/strategy，保留 key 不一致的验证目的。
- 原 qty=0/t1_locked=100 损坏或边界 fixture 复制单 Position 并通过 fixture_insert_position 写回，未降低断言。
- ParentOrderPlan、TradingPlan 使用 active_child_order_id、active_child_remaining_qty、filled_qty getters；Snapshot DTO 事实字段保持原表示。
- 混合 root fixture 仅覆盖 belief_participants 条目的 belief_mut，保留原模板替换仅改 BeliefBook 的语义。
- PlanChainFactConsumption 的 operations/receipts 断言改 operation_count/receipt_count；零长度仍断言等于 0。
- 四处 Market setter 使用 cfg(test) fixture_set_last_price。

仅运行六个指定文件 rustfmt（skip_children=true）及该文件簇 git diff --check，均通过。没有运行 cargo 或产品测试；主代理仍需编译及独立复核。精确短测试 filters 为 adaptive_plan_chain::tests::、initial_candidate_round_tests::、quote_expiry_tests::、quote_expiry_checkpoint_tests::、decision_resources_tests::；npc_lifecycle_projection_tests 为 adaptive_plan_chain tests 内嵌模块。测试声明与断言数量保留。

编译 02 后续修正：decision_resources_tests 四处 Position 私有字段写入遗漏已改为 from_restored_parts（更改原 qty 或 t1_locked，其他三个 getter 事实保持）；随后仍由 fixture_insert_position 写回。已读取 engine-build-02-pipeline.log。指定 rustfmt 通过，未自行重新编译或测试。
