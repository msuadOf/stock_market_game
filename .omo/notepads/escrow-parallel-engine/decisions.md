# Decisions — escrow-parallel-engine

Architectural choices and rationales discovered during work on this plan.

_Auto-scaffolded by /start-work. Append new entries below - never overwrite._

## 2026-09-20 P2 NPC cash-cap sealed-budget source

- NPC projection cash caps consume the immutable P1 `DecisionResourceSnapshot.available_cash`; they must not recalculate availability from mutable post-capture shadow cash or order books.
- Structured Cancel/Replace decisions do not release reservation back into the same sealed batch. This is ADR-0017 divergence #2, not a legacy-equivalence defect. Residual NPC intents consume the sealed budget in canonical source order.
- Raw NPC keys survive only exact lineage. Synthetic or rewritten parent-order intents may not borrow a key merely because stock and side match.

## 2026-09-20 Auction component-adapter handoff boundary

- The production-contract auction component adapter (not yet wired into the production phase path) returns an adapter-owned per-stock wrapper: tick-start `AuctionCompletionInput` plus the complete ordered `P3ValidatedOperation` stream. This preserves sealed-index holes and Place kind until a later worker applies operations and then completes the auction.
- The adapter does not pre-apply operations or discard cancellation facts/receipts. Auction `arrival_seq` remains the original `OrderId` and is checked against the JavaScript-safe persistence boundary.

## 2026-09-19 TickShadow / P9 兼容桥决策

- 在实际 P0-P6 业务迁移前，采用完整 typed `TickShadow` 作为一次性兼容桥：捕获 production authority，在 shadow 内运行既有 tick，P9 单函数按显式字段清单提交。该桥禁止回落到 SaveSlot/JSON/restore，且真实 session 在 P9 前不变。
- 非权威测试策略保留既有可执行测试语义，但不能导出 `StrategyState`、不能保存、不能参与 production shadow capture；production 路径只接受 sealed strategy state。

## 2026-09-19 用户授权合并 Todo 3-6 wave

- 用户接受 Oracle 的 `PLAN_BOUNDARY_CONFLICT`，授权将 Todo 3 envelope ledger、Todo 4 validation split、Todo 5 receipt settlement 与 Todo 6 stock state machine 合并为一个实现波次。此授权只解除原任务边界，不扩大 A 股交易语义、#9 费用裁定或 sealed preservation corpus 的可变范围；plan checkbox 仍由 coordinator 维护。

## 2026-09-19 Continuous cancellation fact boundary

- The continuous cancellation primitive returns a typed cancellation fact and typed outcome, while the existing wrapper remains the sole public event/sequence boundary. The primitive retains current state updates and diagnostic terminal cause but does not allocate `seq` or construct/push `Event`; non-mutating rejection is guaranteed by market clone-before-cancel and commit only after owner validation.

## 2026-09-19 Atomic P0 expiry boundary

- P0 operates only on the captured TickShadow and stages its own candidate before installing into that shadow. It releases the exact pre-P0 `Envelope::live()` resources through PreSeal receipts before state-only cancellation, then P9 remains the sole authority commit. The compatibility bridge accepts a typed one-purpose flag and skips only its initial NPC expiry if P0 emitted releases.
- Envelope membership and seen receipt keys are tick-local working state after P9, while the receipt cursor remains global. This prevents stale legacy-body projections from being treated as current envelopes while retaining deterministic global receipt indices for P0 batches.

## 2026-09-18 Rust host automatic-cycle transaction repair

- The transaction boundary is the complete unpublished automatic cycle, including every
  fastest frame and civil preparation. Later failure restores pre-cycle SaveSlot and
  retained intraday before fatal-only publication. Earlier published cycles are retained.
- Pending player intents accepted before checkpoint are part of that checkpoint; mpsc
  commands cannot interleave with synchronous preparation. Scheduler fields are updated
  only after fallible preparation succeeds; terminal failure then stops scheduling.
- Restoring the pre-cycle game requires a separate host fatal latch (server) or closed
  command receiver (desktop), so rolling back engine state cannot accidentally resume it.
- Pause preference mutation requires owner bearer and canonical generation; generation
  is checked inside the actor, not merely at HTTP lookup time.
- Publisher exact retries are rejected as CursorMismatch, requiring baseline/resync.
  No change to consumer ReplayGuard retry semantics and no Web consumer migration here.

## 2026-09-17 Todo 2 composite baseline identity

- The authoritative pre-refactor behavior baseline is the exact live behavior at Todo-2 start, represented fail-closed as committed HEAD plus the byte-exact pre-existing engine/runner patch and per-file blob hashes.
- A detached worktree must apply that patch and run the corpus there. The additive harness has a separate hash and is not treated as pre-existing business behavior.
- No pre-existing dirty change may be committed, reverted, or silently attributed to Todo 2. HEAD, patch, source file list, and harness must all verify before reuse.

## 2026-09-17 Todo 2 baseline input closure and overlay format

- Baseline identity covers the transitive inputs actually consumed by `cargo run/test -p engine --example escrow_baseline_corpus`: workspace Cargo/toolchain inputs, `packages/engine/**`, and corpus run configuration.
- Unreferenced app hosts, host-parity/release-contract/Wayland/K7 tools are excluded from this engine-behavior corpus identity; they receive their own later source-bound gates.
- Pre-existing tracked modifications and applicable untracked inputs are represented by a content-addressed overlay archive, not Git patch alone. Its manifest records sorted relative path, file type, mode, byte length, and SHA-256; archive hash and post-apply per-file hashes are fail-closed.
- Todo-2-created harness bytes remain a separate `harness_identity` and are not attributed to old business behavior.

## 2026-09-17 TickFrame buffering and event ordering

- Engine progression remains one complete tick at a time. Every tick produces a distinct TickFrame containing that tick's time-series/auction payload and events.
- High acceleration batches multiple completed TickFrames for transport only; tick boundaries and ascending tick order remain mandatory, and intermediate time-series data may not be dropped.
- A batch may carry only the post-final-frame full runtime snapshot. Events remain strictly seq-ordered for current host compatibility.
- Within a frame, causal/legacy emission order precedes entity tie-breakers. `OrderAccepted → AuctionTick` is preserved when the auction frame observes the accepted order; independent cross-entity events have no business-order claim but receive stable deterministic tie-breakers.

---

## Todo 1 decisions

- Accepted ADR-0017 as the sole contract for downstream implementation. No serial reference engine is introduced; budget 1 is only a parallel scheduling comparison condition.
- Resolved Q13 in `docs/open-questions.md`. The existing Q12 external-cash-flow question remains open.
- Preserved the pre-existing C06/external-calibration section in `docs/trading-rules.md`; it was already present before this task and was not touched.

## Repair entry: 2026-09-17

- Adopted the verifier-required `ResourceLimit` mapping: `Session` entity tag plus `Session` EventSourceIndex and session emission-order local index, with no invented account ID or product field.

## 2026-09-17 Todo 2 canonical event delivery resolution（已被下方最终决策取代）

- Engine computes one complete tick at a time and emits one distinct `TickFrame` per tick with tick number, events, time-series/auction payload, and seq coverage.
- Transport may buffer multiple completed frames as an ordered `TickBatch` at high acceleration. It may not merge tick boundaries, reorder ticks, or omit intermediate time-series data. Only one full runtime snapshot after the final frame may be included, and it is authoritative final state; frontend event-before-final-snapshot and strict contiguous seq behavior stays unchanged.
- The stable key is `(phase_rank, causal_emission_ordinal, entity_tag, EventSourceIndex, local_event_index)`. Causal/legacy emission order is primary, with deterministic entity/source/local keys as later tie-breakers. Causal ordinals come from sealed operation order or canonical P7/P8 outbox append order, never worker completion timing.
- This closes the Todo-2 canonical-order blocker without introducing a Stock-first divergence. The shared Session-stream ordinal repair and collision-as-`InvariantViolation` remain part of the contract.

## 2026-09-17 最终 TickFrame / TickBatch 协议决策

- 后端始终逐 tick 完整执行和结算；只有已提交的 tick 才形成 TickFrame。加速倍率只改变网络批量大小，不改变模拟步进。
- 每帧显式携带分时线/竞价数据。TickBatch 必须按 tick 递增保留全部帧，批次最后可附一份权威 runtime snapshot。
- 前端可把多个帧一次写入图表并只渲染一次。该选择是前端内部优化，不是前后端业务协议。
- 帧内 Event[] 是事实集合；数组位置与 seq 不表示业务因果。seq 仅用于覆盖、去重、断线检测和稳定重放。稳定排序是字节确定性的实现细节。
- 基线比较以 tick 为边界，按事件稳定身份与业务载荷进行多重集比较；`OrderAccepted → AuctionTick` 与反向排列不再构成业务分歧。

## 2026-09-17 最终 TickFrame / TickBatch 协议取代旧顺序表述

- 后端必须逐 tick 完整计算、结算并提交，之后才生成 `TickFrame`。每帧携带该 tick 的 `timeseries_payload`，高加速只允许把完整帧按递增 tick 号放入 `TickBatch`；可在最后一帧后附一个权威完整 runtime snapshot。
- 前端逐帧处理或吸收多帧后渲染一次都可以，但渲染合并只是前端内部行为。不得把它解释为协议或业务语义，也不得从 `Event[]` 顺序重建图表数据或权威状态。
- 帧内事件是事实集合，数组位置与 `seq` 不表达业务因果。基线按 tick 以稳定事件身份和 payload 做多重集比较，`OrderAccepted` 与 `AuctionTick` 的数组重排不新增分歧；同一实体 FIFO、订单 ID、收据身份、tick 边界和事件完整性仍受保护。
- 后端稳定字节仍使用 `(phase_rank, entity_tag, EventSourceIndex, local_event_index)`，冲突是 `InvariantViolation`。该键仅服务确定性序列化、去重和回放，不授予跨实体业务顺序。

## 2026-09-17 历史决策隔离说明

- 本文件前文标注为“已被下方最终决策取代”的 canonical event delivery 条目，以及其中的严格 seq 排列、causal/legacy emission order、`causal_emission_ordinal` 和 `OrderAccepted → AuctionTick` 约束，均为废止的中间决策，不得作为现行协议依据。

## 2026-09-17 最终 TickFrame / TickBatch 协议取代旧顺序表述

- 后端必须逐 tick 完整计算、结算并提交，之后才生成 `TickFrame`。每帧携带该 tick 的 `timeseries_payload`，高加速只允许把完整帧按递增 tick 号放入 `TickBatch`；可在最后一帧后附一个权威完整 runtime snapshot。
- 前端逐帧处理或吸收多帧后渲染一次都可以，但渲染合并只是前端内部行为。不得把它解释为协议或业务语义，也不得从 `Event[]` 顺序重建图表数据或权威状态。
- 帧内事件是事实集合，数组位置与 `seq` 不表达业务因果。基线按 tick 以稳定事件身份和 payload 做多重集比较，`OrderAccepted` 与 `AuctionTick` 的数组重排不新增分歧；同一实体 FIFO、订单 ID、收据身份、tick 边界和事件完整性仍受保护。
- 后端稳定字节仍使用 `(phase_rank, entity_tag, EventSourceIndex, local_event_index)`，冲突是 `InvariantViolation`。该键仅服务确定性序列化、去重和回放，不授予跨实体业务顺序。

## Todo 2B-1 证据修复：延后存储边界决策

- 现行导出权由封闭 `ProductionStrategy` 授予；普通 `Strategy` 保留决策扩展能力，不提供可覆写 state 导出。四个基础测试、三个已注册身份对抗测试与一个外部伪造编译失败测试组成当前验证集。
- Account 当前保存 `Box<dyn Strategy>` 并擦除生产能力。这不破坏封闭注册表，但在 2B-2/P2 shadow hydration 从权威存储提取状态前，必须先让 Account/session 生产存储保留 `Box<dyn ProductionStrategy>` 或等价封闭 wrapper；该迁移是硬依赖，不可通过可信自定义投影绕过。
- 测试专用自定义 Strategy 注入走独立仅测试/非权威路径。本轮不实施存储迁移，不修改生产/测试代码或计划复选框。

## 2B-2 decisions

- Desktop fatal repair uses dedicated engine-failure instead of changing healthy EngineEventPayload. Injection remains cfg(test) inside desktop actor only; engine production API unchanged. Stop immediately after synchronous failure emit and do not drain queued commands after fatal shutdown.

- Use a private-variant StoredStrategy wrapper with sealed production and explicitly non-authoritative decision storage. Save/hash reject non-authoritative state rather than forging StrategyState.
- Keep healthy SaveSlot schema and current step behavior unchanged behind Result gates. Failure injection is cfg(test) and pre-mutation only; full phase rollback remains Todo 7.
- Use fixed-width length-delimited deterministic FNV-1a StateHash with exhaustive field inventories and complete market/orderbook private-state projections. This is not a security digest. No per-tick whole-session transaction clone/serialization.

## 2026-09-18 2B-3 实体分类勘误

- 已接受的 ADR-0017 与最终 TickFrame 事实多重集协议具有现行效力。计划任务 2 将 OrderAccepted、OrderCanceled 列入 Stock 的旧句已同步为 Account(id)，不是新增业务分歧。
- 封存 attempt-12 采集器与生产 EventStableKey 原本均使用 Account(id)。不添加第二身份域、LegacyComparisonEventKey 或转换层，不修改封存字节。
- 重建只证明字节身份与完整性；语义依据来自 ADR 逐变体表、穷举映射测试和封存采集器复核。共享测试夹具分别驱动生产键及采集器原有穷举匹配，覆盖全部十二个变体和三个阶段 6 的 Session 共享序号。

## 2026-09-18 2B-4 自然日屏障用户决策

- 用户明确选择独立 CivilUpdate：自然日更新消耗全局 seq，但不推进市场 tick；与 TickBatch 组成有序传输联合，消费者先完成屏障，再接受下一批 tick。
- 收盘屏障保留完整分时与竞价序列并进行权威收盘刷新；开盘前屏障同步行情、证券、公开披露与 K 线，重连客户端不依赖此前事件顺序。
- 收盘后及开盘前暂停是宿主调度偏好，默认均关闭；先交付屏障，再暂停，恢复不重复日结。隔夜委托明确延后，不修改委托有效期和交易规则。
- 前次 preflight blocker 已由该决策解除。该决策记录不表示 2B-4 实现与验证完成。

## 2B-4 foundation repair 记录

- 本轮仅修改引擎协议、测试、生成绑定及证据，不迁移宿主。公开索引改为报告和公告的检查式并集；原报告专用接口保留，避免改变既有业务调用者。
- EventFact 携带已确认稳定键和完整规范 JSON 载荷，ReplayGuard 比较已接受游标的规范更新；它不是签名或安全认证，不把 seq 当业务因果。
- 竞价改为保留指示及完成点的向量；连续图显式携带每股点。自然日 kinds 从结算前日期/阶段与结算后状态精确推导。
- 最终全引擎测试、严格 clippy、全仓 fmt 检查通过；但 fail-first 和若干对抗矩阵证据不足，详见 foundation-repair-receipt.md，不宣称四项完整验收或任务 2 完成。

## 2B-4 foundation 最终验收补齐

- optional imbalance 使用 JS-safe Option<u64> serde，保持 TS number | null；记录 2^53 先红后绿。事实对应改为 seq→规范载荷的精确映射，拒绝重复游标、重复键和不等重数。
- 生产 project_timeseries 被真实 step_frame 和合成多竞价测试共同调用；真实公告屏障经存档重连查询，公开索引与 NPC 是否获知无关；真实报告测试沿用并通过。
- 补齐连贯篡改 ReplayMismatch、tick→civil→tick、四类真实日历边界及 kinds/date/status/phase/history 对抗矩阵。永久 protocol_probe 输出游标、ExactRetry、篡改拒绝和三个竞价点。
- 最终源字节完成后全 engine suite、严格 clippy、全仓 fmt、53 个绑定导出通过；LSP 超时如实记录。独立最终复核无范围内阻塞，不作签名认证声明；历史先红不足不伪造。
- 仅 foundation 验收闭合，任务 2 仍开放，宿主迁移仍须父代理授权；未修改交易语义、暂停调度、隔夜委托或计划复选框。

## 2026-09-18 Web protocol core 阻断修复

- 协议边界对象与 Map 归一化容器固定使用 null prototype；`__proto__`、`prototype`、`constructor`
  和空键在写入前拒绝。Map 用原始键身份检测归一化冲突，不能让 `1` 与 `"1"` 覆盖。
- Web 传输解析只负责 Rust 生成类型的结构、枚举、整数宽度与无损十进制边界；不复制
  `SessionSetup::validate` 的 A 股代码前缀、交易所、板块、涨跌幅、最小价位、价格或股本业务政策。
  引擎仍是这些业务语义的唯一权威。
- `EventStableKey.phase_rank` 明确为 Rust `u8`，`Report.report_revision` 明确为 Rust `u32`。
  此项是 protocol core 修复，不表示三宿主或 React consumer migration 已完成。
