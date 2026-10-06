# Read-only start 后继续排查记录（2026-10-06）

当前已提交 `382c1871`，正式候选 `219.6x`，仍未到 1000x。本记录只基于当前代码静态审查和既有 68db9414 阶段诊断，未开始新改动。

## 1. CPU 占用与调度结构

- 16 worker candidate 平均 CPU 约 5.98 核（user 约 4.04 + system 约 1.93），没有吃满 16 核；此前已证明 Rayon 更忙等只会退化为 123x。
- 每个市场 tick 的 coordinator 是 server actor 的一个 Tokio task；tick 内多个 Rayon 屏障导致 worker 阶段性空闲。
- 下一批不再单纯“占用更多 CPU”，而是减少 coordinator 线程串行段或把互相独立的准备重叠。

## 2. 可并行但尚未重叠的独立段

1. **NPC next-tick queue**（`queue_npc_for_next_tick`）发生在 `candidate_commit_prepare` 的尾部，串行调用 `capture_decision_snapshot` + `stream_npc_decisions`。decision snapshot 中 attention/retail experience/retail analysis 都使用并行 page map，但消费者 `receive_private_intent` 与 projection 单账户处理是串行。
   - 可先做只读决策计算与投影意图的纯值部分，接收/`ingress_receipt_cursors` 保持原串行顺序。
2. **`capture_decision_snapshot`** 是内部 mutation：pop attention、更新 attention/experience、working order capture。不能和 seal/hydration 并行；需要拆成只读输入捕获与 candidate 内独立 mutation 段。
3. **`prepare_stock_input` 的 owned `Envelope`/`Market` clone 与 hydration fee 投影读取同一订单行**。已并行启动，但两段仍各自全量重算费用/资源。可探索共同借用 facts 在一次扫描中同时产出 hydration projection 与 stock owned snapshots，保留两套门禁和错误顺序。
4. **read-only start 已覆盖 hydration/resources/stock inputs**，但 `build_account_validation_context` 和 `ReadyIngress::capture_sources/capture_roots` 仍在 DecisionShadow 内。它们均为 `&GameSession` 只读，理论上可提前并行；需要保持 ingress/context 首错优先级。
5. **`continuous_shards`/from_post_expiry** 在 stock input 完成后创建每股私有 ledger，并调用 `validate_initial_snapshots`。旧诊断合计约 1.9s wall + 1.3s CPU，且之前“initial snapshot 索引”与“整股缓存”均无收益。下一批不应重复这些方案。

## 3. 新发现的显性串行成本

- `frame_after_step` 每 tick 先 `runtime_snapshot()`。虽然 runtime snapshot 只发布玩家账户，但它仍遍历全部 NPC resting orders 来累计 reservations，再丢弃 NPC 行（`snapshot_inner(... include_npc_accounts=false)`）。默认 20,007 NPC 下这是明显的无谓 O(live orders) 扫描。
- `tick_batch_delta` 再次 `public_runtime_state()`，又完整扫描全部订单计算玩家 reservations。
- `Frame.validate()` / `TickBatch.validate()` 反复调用 `canonical(event)`：frame 构造时生成一次 canonical payload，协议 `prepare_frame` 又逐事件验证一次，publisher 再验证一次。同一事件至少两次 canonical JSON 序列化。
- `ProtocolSession::step_frame` 每 tick `checkpoint()`；`clone_for_tick_shadow` 需要遍历所有 account pages 做 strategy validation，再加上大量 map clone。这是 defensive rollback 的成本，不能简单删除；可评估失败率与外部 host 语义后做受控 failpoint 测试，但不能为性能削弱失败原子性。
- `intraday.push(Arc<TickFrame>)` 目前克隆整个 frame 再放入 Arc。每 tick 克隆 facts/events/timeseries payload； AppendOnlyHistory 只是让 rollback 共享 chunk，不解决首个 push 的 owned clone。应先测帧 clone/协议成本，再决定是否改为 `Arc::new(frame)` / move 或批量构造。
- Fastest batch 每 tick `checkpoint` + 一次 `tick_batch_delta`；checkpoint 是每 tick 必付，不能批量共享，因为失败必须回滚本批。协议帧验证也不能直接删。

## 4. 优先候选（尚未实现）

1. **玩家 reservations 索引化 / 按账户过滤扫描**：为 runtime snapshot 建立只读玩家订单索引或用 orderbook live_by_id 按玩家取行；必须仍覆盖 continuous + auction、partial filled value、duplicate 等语义，不能只遍历一侧簿。目标是消除每 tick 两次全 NPC 扫描。
2. **协议 canonical 单次构造/验证复用**：为 `EventFact` 构造与 `validate_facts` 提供同一批 canonical 结果，或让 batch validate 使用已认证 facts，避免两次 serde_json。
3. **NPC queue 只读决策/投影与串行 receive 分离**：先并行产生纯 projected intents，再按原顺序 `receive_private_intent`。
4. **共同 read-only facts 扫描**：hydration 与 stock input 共享一次费用 facts 计算，避免同一 order 两次 math/clone。

## 5. 已完成并撤回的小实验：intraday frame 额外 clone

- 假设：`intraday.push(Arc::new(frame.clone()))` 除公开返回值外额外完整克隆一次 frame。
- 实现：提交 frame 先放入同一个 `Arc`，历史保存 `Arc::clone`，公开 owned API 保留一次返回克隆；仅 11 行。
- 结果：candidate 223.040x/217.658x，相邻 382c1871 220.021x/223.969x，均值约 -0.74%，波动内无收益。
- 已逐字节回退；详情 `frame-clone-count-findings.md`，patch `frame-clone-count-experiment.patch`。不要重复该实验。

## 6. 结论

以上是下一步假设；必须先做可撤回插桩或小实验，禁止为了减少校验而删业务门禁。当前优先级最高的静态发现仍是 runtime snapshot 的 NPC order 全量扫描，但需新插桩证明实际成本后再改。
