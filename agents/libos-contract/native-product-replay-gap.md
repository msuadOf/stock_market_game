# Native 产品实跑：恢复后周末与周一事实重放缺口

在 dot 云端，以同一刚编译的 Native Server、同一会话的 HTTP/WS 真入口、两个 NPC Group / 两个 Desk 运行 `scripts/libos-product-parity.mjs --mode native`。日终 SQLite 写入、真实撤单、双市场成交、两组 NPC 活动、周末及读档均经过断言。未生成成功报告。通过临时 Node load hook 只打印断言前的六个原始帧，不改夹具条件或 Rust；原始记录为本地 `.tmp/libos-acceptance/product-replay-inspected.log` 的 `REPLAY_WITNESS`。

| 权威面 | 连续运行 | 从同一周五日终档恢复 |
|---|---|---|
| Checkpoint | tick 60 / seq 912 | tick 60 / seq 912 |
| 周六、周日结算 | seq 912→913→914；Session 域 `local_event_index` 1, 2 | 相同日期、状态、seq；索引 0, 1 |
| 周一第一拍 | tick 61，seq 914→922；6 条 `OrderAccepted` + 2 条 `AuctionTick` | tick 61，seq 914→919；3 条 `OrderAccepted` + 2 条 `AuctionTick` |
| 上海 `AuctionTick` | 1232 分，预演撮合 800 股 | 1232 分，预演撮合 200 股 |

同一次 fixture 中，周五存档后以及重放过程中均未提交玩家意图，周末两个闭市步骤与周一第一次单步相同。新增或缺失的挂单均为 NPC；未将预演量误报为真实成交。

- **已定位：同 tick 的 Civil 事实续号。** `packages/engine/src/session/protocol/civil/session.rs` 的 `restore_with_topology` 调用 `from_game`，后者总以 `SessionFactCursor::default()` 初始化。 `packages/engine/src/session/protocol/facts.rs` 中 `begin_tick` 只在 tick 改变时清理流域；`assign_session_facts` 从已提交域的上一索引续号，空游标从 0 开始。`SaveSlot` 保留全局 seq，但未携带该域游标。若要求跨恢复仍是同一事实身份，需要单独明确/版本化其持久化契约；不能从全局 seq 或周末天数猜测，行政披露也占用 Session 域。若产品选择世代隔离身份，应明确区分与当前严格重放门禁，而非在夹具里过滤身份。
- **已定位：跨 Desk 发布次序依赖线程到达。** `runtime/coordinator.rs::converge_round` 的 `DeskReport` 分支按到达顺序追加 `desk_lifecycle`、`desk_receipts`、`desk_auction`；结算末尾仅 `retail_fragments` 已按 stock 固定顺序。`runtime/world.rs` 逐轮原序追加，`runtime/drive.rs` 据此调用 `project_lifecycle_events` / `project_auction_events` 分配 seq。本次 `AuctionTick` 的代码顺序在连续运行是 0,1，恢复是 1,0；并非 fixture 重排。这能解释发布顺序和 seq 归属差异，不能解释多出的 NPC 订单或成交预演量。下一步先保持交易部内部因果顺序，定义跨 Desk 的稳定发布合并，再在分配 seq / EventStableKey **之前**验证，不在 JS 重排已提交事实。
- **仍待定位：周一 NPC 行为实质差异。** `runtime/drive.rs::profiles_from_session` 已从 `runtime_attention_state` 读取既有 due/RNG；`runtime/world.rs` 组种子 XOR 和 `runtime/group.rs::EchoAttention/Sparse` 反向投影在同名册/同两组下对称，不能仅凭结果说“存档没有 RNG”。同一 tick 的真实决策还依赖 due heap、`runtime/group.rs::run_round` 的共享市场信号（快照）、个人策略和状态。缩小复现：对失配属主 31、20、4294967298 比对两条路径的当前 due/RNG、开盘投影 signal、候选是否观察及其入 Desk 前操作；以最先失配的可持久事实定位，而不是先扩大存档 schema。原正式用例保留。
