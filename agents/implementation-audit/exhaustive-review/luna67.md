# Luna67：server、WASM 与 auction 全文复核

- 日期：2026-10-03；基线 `b89afb3346743a4b4fccf26c9ac9ff108595f696`，目标产品提交 `08e4fc7`（当前 worktree 的 merge commit 同产品树）。
- 只读范围：三份指定实施/复核记录全文至 EOF；对照当前生产调用链、目标提交差异、正式 A 股交易规则、ADR-0017/0018。未修改产品文件，未运行测试、长测或 Git 写操作。

## 全文与章节矩阵

| 文件 | 行数 | 章节/区间 | 实现主张及实际核对 |
|---|---:|---|---|
| `hosts/review-server.md` | 63 | 1–6 范围；8–17 R-SERVER-01；19–52 三项门禁；54–63 验证结论 | 旧复核覆盖七文件 diff；独立重核 `WsPublisherConnection` 与 `run_ws` 状态机、StaticAssetRoot 路径边界、ServerFixture/ApiTestSession 生命周期，以及 flush `all` 断言的增量修复。 |
| `hosts/server/status.md` | 57 | 1–7 总状态；9–15 六动作表；17–25 协议边界；27–35 验证/残余；37–46 短测清单；48–53 修复过程；55–57 root 验证 | 逐项映射 registry、静态服务、WS 发布、API/WS fixtures 到其 caller；核查收尾验证段与上一 reviewer 的结论是否一致。 |
| `pipeline/auction/implementation.md` | 53 | 1–6 状态/基线；8–25 owner/caller；27–35 语义；37–40 跨簇；42–53 验证 | 逐方法追 shadow/coordinator、股票 shard finish、candidate 投影、竞价边界 capture 与日终 transition；以 `docs/trading-rules.md`、ADR-0017/0018 为规范基线。 |

## 真实调用链

### Server / WASM

- `SessionRegistry::create/restore` 先完整构造 `ProtocolSession`，成功后才 `register`；`restore` 与 `restore_json` 共用该恢复入口。`step_update` 仍按 `civil_day_ready → end_civil_day_update` 或 `step_frame → tick_batch` 推进，并在 fatal 上补 `at_session`。生产 `create_session`、`restore`、`restore_json`、step 及其余句柄导出均经同一 thread-local registry；仅测试内构造访问 registry。未知 access/step 显式失败，remove 对未知句柄保持幂等。
- 对照基线直接 `HashMap` 插入/取出与当前代码，提取 receiver 未改变 WASM 导出签名、解码路径、错误形状或协议步骤。恢复失败发生在注册之前，不遗留半构造 handle。
- `NEXT.fetch_add` 仍为 `AtomicU32` 且没有 wrap 检查。旧复核/status 已明确保留耗尽/wrap-around 行为；序号回绕后仍可能复用旧 handle，这是历史残余，不是本次 Registry 抽取新增回归，也不能写成已解决。
- `StaticAssetRoot::validate_package` 在启动校验 canonical root/index/assets；每个真实 `serve_file` 再按字面路径、GET/HEAD、canonicalize、root containment/hidden component、metadata、`ServeFile` 顺序检查。`static_router` 与 `deployment_router` 的路径接线仍使用该入口。未发现请求路径缓存或新增 symlink/TOCTOU 承诺；原复核关于不声称完整 Axum task / 任意取消清理保证的边界仍成立。
- `WsPublisherConnection::ingest` 实际顺序为 failure → generation → resync barrier → baseline-covered → buffer push。`run_ws` 订阅后拿 baseline；发送失败关闭连接；failure 在成功发送后 latch；MetadataTransition flush 一帧、Push capacity flush backlog，均在 socket 发送成功后才接受 incoming update；Pull capacity 与 Lagged 清 buffer 并要求 Resync。Resync 仍 subscribe → clear → 取 baseline，只有成功发送后更新 barrier/failure 状态；`SubmitIntent` 可在 resync barrier 期间入队，`CommandQueued` 只是入队回执。与旧结论及 status 一致，未发现重构改变这些协议事实。
- `ApiTestSession` / `ServerFixture` 是测试资源 owner，不是产品 session owner。前者正常和已完成身份构造后的 fixture/restore 错误显式 shutdown；后者 abort+await listener，再 remove session 并请求 actor shutdown。结论不扩展到所有 panic/取消、Axum 子 task 或跨进程清理。

### Auction / 日终

- `auction_tick_transaction` 在命令流排空后 capture `AuctionTickBoundary`，`IncrementalAuctionStockCoordinator` 分区并在 Rayon worker 调 `AuctionStockShadow::apply_round`，逐股票保留输入操作次序；receiver 的 `apply_operation` 仍更新该股票 ledger、completion、receipt 与 lifecycle。coordinator 统一选择 worker 首错并在全体成功后安装；consuming `finish` 经 `stock_stream::finish_auction_shards` 执行 worker tail。
- `apply_finished_candidate` 仍按 worker finalizer 校验、receipt aggregation/settlement、lifecycle projector、计划同步、market/candle/order state、order cursor/tick 次序推进。projector 的 parent/pending/retail 缓冲仅在 lifecycle 检查成功后安装。
- 日终真实 callers 是本文件 auction 收尾与 `continuous_tick_finalizer`；两者调用 `TradingDayEndTransition`。该 transition 先拒绝残留 live order/envelope，再按 `t1_enabled` 解锁 T+1、同步/sweep plan、清 parent/NPC lifecycle、提交 candle、checked day+1、清 minute history、产生一次 `DayBoundary`。它不保证内部逐步 rollback；上层丢弃候选状态维持 authority 原子性。此边界和旧复核一致。
- 正式规则优先采用 `docs/trading-rules.md`：集合竞价阶段/撤单窗口、开盘余单进入连续簿、收盘竞价清算和余单日界失效均有明确条款；深市最终距离仍统一参照昨收是文档标注的简化。ADR-0017/0018 要求来源身份/sealed index 不充当交易优先级。代码只把 sealed identity 关联 typed facts，操作仍按对应股票实际传入次序执行；本批抽取未变更清算算法、费用、T+1、证券类别或存档契约。旧的 `review-auction.md` 对完整 auction diff/调用者及上述规则边界已有独立复核，未发现相反证据。本次没有重新访问交易所官网，不声称当日重核官方制度。

## 旧结论复核与新候选

- R-SERVER-01（flush 测试未固定 `all`）修复有效：当前 status 记录 `expected_all` 分别固定 `false/true`，旧 reviewer 的问题关闭；这是测试边界加强，没有产品改动。
- 旧 reviewer 关于 server 协议顺序、错误发送后状态更新、fixture 清理范围、静态路径次序及“仅静态复核、不代表测试通过”的结论均由真实 caller 核实，未发现反证。Registry 构造/恢复失败不注册、`step_update` 屏障同样有实际生产路径支撑。
- **新候选 D1（低，复核记录状态表述过时）**：`review-server.md:56–60` 仍以“运行门禁待父 agent 统一执行”作为结尾；而同主题 `hosts/server/status.md:55–57` 与其 `status.json` 已记录 root build/check 和选定 case 的最终代表性验证。前者可保留“本 reviewer 未运行”的个人范围声明，但最终批次状态应指向 status 的 `final_validation`，不能让结尾看起来仍待执行。此为文档一致性发现，不是产品缺陷；由文档维护者决定修订。
- `NEXT` wrap-around 是可解释的历史残余，但基线已有同一 `AtomicU32` 分配且 status 明示未修复；不登记为本批新缺陷。
- 除 D1 外未确认本复核范围内新有效发现。静态核查不替代已记录的运行证据，也不把未选择执行的旧用例、完整 suite、真实矩阵/E2E/性能验收写成通过。
