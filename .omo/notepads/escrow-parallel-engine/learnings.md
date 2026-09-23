# Learnings — escrow-parallel-engine

Conventions, patterns, and successful approaches discovered during work on this plan.

_Auto-scaffolded by /start-work. Append new entries below - never overwrite._

## 2026-09-19 #9 卖单零现金预留

- 将 live cash reservation 集中到 `live_cash_reservation`：Buy 仍调用原有 buy helper，Sell 固定为零；名义 `sell_order_fee_reservation` 公式不改，不再用于接单、快照、预算、计划子单或 restore 的 live cash 验证。
- 冻结 B 断言的 current hash 必须用 `preserved-tests/core.mjs` 的 `rustItems`/`assertions` 实际计算；本次 4 个符号只有 6 条断言真实变化，`class_b_changes` 不声明未变断言。

## 2026-09-19 #9 gate repair

- rustfmt 改变 E9-B 断言换行但 verifier 的 lexical hash 不变；必须实际重算而非假定 mapping hash 要修改。名义卖方费用预留被放在 `cfg(test)` baseline seam，由 unit test 调用，避免 `allow(dead_code)` 且不回流到 live reservation。

## 2026-09-19 envelope constructor and hydration core

- Envelope 不能用无语义 constructor：tick 起点和 P3 新建均显式构造，并由 tagged `EnvelopeBasis` 携带各自守恒基数；validate 对 origin/basis 用穷举 match，拒绝不一致组合。
- hydration 先构造 candidate ledger 再比较/安装，因此 missing、extra、origin、resource、audit 任何不符均不会推进 receipt cursor 或改写已存在 envelope。
- 提取 test module 时必须按原 test 名逐项跑 fully-qualified `cargo test -- --exact`；短名称可以匹配 0 项而仍 exit 0。receipt fixture 保持 sealed-batch local key 和 P3-created envelope，才能覆盖 cursor 的 transactional 语义。

## 2026-09-19 live envelope mixed-book projection

- continuous 与 auction 都使用全局 `next_order_id`/`OrderId` 身份域，projection 按 key 排序后必须拒绝重复 key。mixed fixture 的正确断言是两个 envelope，并分别检查 continuous Buy 的现金预留和 auction Sell 的零现金/剩余股数，不能只检查集合长度。
- 现有 live order 不保存名义/实收费用组成，因此 hydration 如实投影零 fee components；不在 projection 阶段重算或虚构未来 P4/P6 审计历史。

## 2026-09-19 TickShadow / P9 internal milestone

- `GameSession` 的权威字段通过穷举解构进入 `clone_for_tick_shadow` 和 `commit_tick_shadow`；策略不克隆 trait object，而是经 `StoredStrategy::production_state` 与密封 `StrategyState::into_strategy` 重建。
- 兼容桥只在 shadow-owned `GameSession` 调用旧 tick 行为，P9 再把已完成 shadow 的逐字段权威状态移入真实 session；没有使用 SaveSlot、JSON 或 restore。
- 新增 `envelope_ledger` 和 `next_receipt_base` 已在 authority/hash inventory 中显式列出，当前均以空 ledger / 零 base 诚实初始化；P3-P6 才会产生、持久化并结算真实收据。

## 2026-09-19 Continuous cancellation fact seam

- Continuous cancellation can preserve clone-before-cancel ownership safety while extracting state mutation: cancel only the cloned market, reject wrong owner before inserting it, then commit causal termination/snapshot, lifecycle removal, parent clearing, and retail diagnostic recording. The primitive owns no `Event` surface and never calls `next_seq()`; the legacy wrapper maps typed errors and allocates exactly one sequence for every outcome.
- A typed cause must preserve all currently representable causal terminal reasons, not silently collapse `Reprice`, `Expired`, or `DayEnd` to voluntary. Direct wrong-owner coverage should prove both hash/seq invariance and a later valid owner cancellation.

## 2026-09-19 Atomic P0 expiry

- `EnvelopeLedger::apply` sorts receipts in place, so P0 must associate post-apply receipts back to lifecycles by `EnvelopeKey`, never by zipping with an independently sorted source vector. Multi-account same-stock expiry is the regression case.
- The compatibility bridge can still perform later legacy mutations after P0. Keep envelope/key membership tick-scoped and preserve only the global receipt cursor at commit, otherwise subsequent legacy changes cause exact hydration to reject stale projections.

## 2026-09-19 P0 stale lifecycle repair

- Lifecycle pruning must run after every phase that can route/fill a continuous order. The ordinary routing batch prune is insufficient because `run_decision_chain` performs later real plan routing; a full fill there removes the orderbook entry after the first prune and otherwise leaves P0 with a correctly fatal stale lifecycle.

## 2026-09-19 P0 no-expiry ledger checkpoint

- P0's ledger checkpoint is required even with no eligible lifecycle expiry: exact hydrate/validate must precede all no-op returns so P1 receives the complete shadow projection of both continuous and auction orders. Empty releases do not mean an empty ledger.

## 2026-09-19 P1 immutable allocation seal

- P1 availability is derived only once from the post-P0 live ledger: subtract `Envelope::live().cash` from authority cash and `Envelope::live().shares` per account/stock from T+1-aware sellable quantity. P0 expiry appears only by absence/reduction of its terminal live envelopes, never as a separate release addend.
- A snapshot read surface must reject unknown account and stock keys rather than defaulting to zero; zero is a valid availability outcome for a known account/stock with a fully reserved live order.

## 2026-09-19 P1 cutoff fixture repair

- To prove one-time P0 buy release, use an independent accepted-envelope reservation as the account cash value: pre-P0 availability must be exactly zero and post-P0 availability exactly that one reservation, not account cash by coincidence. The sell mirror must prove pre-P0 zero and post-P0 exact original sellable shares.
- Soft-budget grants are not persisted in `GameSession`; they exist only as local decision-chain `AllocationResult` values. `pending_plan_events` is the nearest real stored prospective plan state and P1 must remain invariant when it is present.

## 2026-09-20 NPC reconciliation planner ownership

- A reconciliation Replace is a causal link, not ownership transfer: the planner keeps the same changed target in `residual_intents`; the compatibility executor cancels only the old order; the legacy routing batch later routes that residual exactly once. Exact Keep alone consumes its matching target.
- The pure planning seam must preserve working-index decision traversal separately from residual desired-intent order. Locked auction suppression mutates only residuals under the existing reviewed/same-side/self-crossing rules.

## 2026-09-20 NPC generation extraction recovery

- A behavior-preserving NPC generator cannot be split by copying the visible decision loop alone. The parent materialization, reconciliation executor, and cash-cap branch must be extracted as one characterized ownership boundary before the legacy tick call site moves; otherwise a partial module both duplicates semantics and cannot establish exactly-once execution.

## 2026-09-20 NPC preparation rollback

- Even a narrow post-strategy helper should not survive on broad-suite green alone: it needs direct characterization of retail traces/watchlists, parent materialization, reconciliation residual ownership, and pre-cap account order. Without those red/toggle proofs, retain the exact inline branch.

## 2026-09-20 NPC decision batch seam

- `NpcDecisionBatch` now owns the complete legacy due-attention through post-reconciliation batch cash-cap boundary. It returns accepted due IDs for the later unchanged decision chain and post-cap `(AccountId, Intent)` output for the unchanged player FIFO append and router.
- The extraction retained the legacy per-NPC SplitMix64 formula, Rayon `par_iter_mut` evaluation, strategy take/restore timing, AccountId result sorting, reconciliation executor, and cash-cap helper without reimplementation. The seam itself does not take `pending_player`, route orders, settle, or invoke the decision chain.

## 2026-09-20 NPC decision seam verification closeout

- The strongest seam parity proof is a fixed-strategy normal `step()` fixture that pins public events plus affected business state, rather than a second generator invocation. Three small projections cover reviewed retail cancellation/experience, institutional parent child materialization, and two NPC capped buys before a queued player buy.
- Reversible source mutants should target observable contracts: counted strategy calls detect a duplicate decision call, attention-state advancement detects skipped rescheduling, and an exact cancellation/price event projection detects duplicate reconciliation. Hash the restored source after every mutant, not only at the end.

## 2026-09-20 Player candidate capture seam

- `PlayerCandidateBatch` is a typed ownership transfer of the existing `Vec<(AccountId, Intent)>`: `std::mem::take` preserves the queue's authoritative insertion order while leaving an empty queue, without assigning any routing, order, event, or envelope identity.
- The compatibility body must keep the three existing class boundaries explicit: obtain NPC batch, capture player batch, extend NPC intents with player intents, route, then invoke the unchanged plan chain. This preserves `npc -> player -> plan_chain` without claiming real P2 composition.

## 2026-09-20 Plan-chain candidate seam

- `PlanChainCandidateBatch` carries only owner, complete `Intent`, and the stable source-vector generation index. Its constructor is pure: it does not allocate `OrderId`/event sequence/receipt identity or mutate books. The only legacy consumer is the existing plan-child submit path, after NPC/player routing; adoption and cancellation/replacement retain their existing plan lifecycle boundary.

## 2026-09-19 Todo 3 preservation verifier repair

- 单 hunk 看到 `expect("healthy save")` 不能证明该 hunk 机械安全；Result API 与 rustfmt 会把一条链拆成多个 zero-context hunk。正确证明单位是完整 path+Rust item：只删两个精确 healthy expect suffix、空白与 trailing-comma rustfmt trivia 后，baseline/current token stream 必须一致。
- C exact records 必须绑定 path、symbol（必要时 baseline_symbol）和 hunk hash；B 必须从 sealed inventory runtime 读取并逐项比较 assertions identity/hunk anchor，而不是只比较四个 symbol 名。

## 2026-09-19 Todo 3 verifier independent PASS

- sealed manifest SHA 与 sealed commit-derived A hashes 是 mutable test inventory 外的必要 trust root。独立 review 已证实篡改 baseline、A hash、B effect/anchor、完整 item 的 numeric/scenario assertion 都被拒绝；当前 337 hunk 的 Result/rustfmt 适配仅在完整 symbol normalized equality 时通过。

## 2026-09-19 B assertion mapping

- B 的 sealed `hunk_anchor` 是原始 assertion substring（`assert...!(` 到 terminating `;`）SHA-256，不是 diff hash。future divergence 必须以 sealed identity/anchor 映射到 current lexical assertion hash；仅 metadata 或粗 token allowlist 不足以授权 #9。

## 2026-09-19 verifier final Oracle PASS

- 同一 Oracle 最终确认：A 必须测试 production verifier 而不是 hash literal；B 必须测试 nonempty declaration 经过 `classifyTracked` 而不只直接调用 helper。完成后 sealed 4 symbol/9 assertion mapping、当前 337 hunk 分类和所有 adversarial paths 均 PASS。

---

## Todo 2A 修复：证据目标切换

协议收据必须把 attempt-11 明确标为 superseded historical evidence，并把 attempt-12 标为当前唯一可复用目标；当前 manifest、corpus、harness、cleanup 哈希应与 DoneClaim 同步，避免历史收据被误当成最终复用入口。

## 2026-09-20 P7 P3 sealed identity validation

- P3 producer facts may use the sealed index only after validating it against the immutable
  canonical P2 batch binding for the same candidate key. Checking merely that result keys are
  unique and sealed indices are contiguous incorrectly accepts a key/index swap.
- This binding validates producer-owned identity; it must not be replaced with a worker output
  vector position or completion order. Formatting evidence for this Rust workspace must use
  edition 2021, not edition 2024.

---

## Todo 1 findings

- The approved plan requires ADR-0017 to be accepted before engine work. The ADR now freezes the P0-P9 phase sequence, `seal_allocation_snapshot`, shadow-only mutation before `commit_tick`, envelope resource equations, receipt ordering, event-source mapping, poison categories, and both state hashes.
- The nine divergence rows are in [0017-escrow-parallel-tick.md](/data1/baiyifan/workplace/stock_market_game/docs/decisions/0017-escrow-parallel-tick.md:74), with #9 at line 84. Seller nominal formulas, rates, and minimum-commission accumulation remain unchanged. Actual seller charge is capped per fill leg, split commission then stamp tax then transfer fee. Seller cash escrow and `spent.cash` are always zero.
- The seller cap is explicitly bounded as an in-game simplification and not exchange clearing. Normal trades are unchanged when the cap does not bind.

## Repair entry: 2026-09-17

- `ResourceLimit` must use `Session`/`Session` event-key dimensions because its current payload has no account ID; local order is the deterministic session emission order.
- Added the exact `Status: accepted` match surface without removing the Chinese status wording, then reran the required checks and manual Read QA.

## Final Session-ordinal repair: 2026-09-17

- `CivilDateAdvanced`, `CompanyDisclosurePublished`, and `ResourceLimit` share the same phase/entity/source tuple, so their `local_event_index` must come from one shared Session emission stream rather than three variant-local counters.
- The deterministic source is canonical P7/P8 outbox insertion order after phase processing; executor completion order is not a permitted ordering input. `ResourceLimit` remains Session because its payload has no account ID.

## TickFrame delivery contract: 2026-09-17

- The resolved delivery contract is one complete engine tick per distinct `TickFrame`, carrying the tick number, `Event[]` batch, time-series/auction payload, and seq coverage.
- High acceleration may buffer completed frames into an ordered `TickBatch`, but transport cannot compress boundaries, reorder ticks, or drop intermediate time-series data. A batch may include one authoritative full runtime snapshot after its final frame, while the existing events-before-final-snapshot and contiguous-seq protocol remains in force.
- The stable event key is `(phase_rank, causal_emission_ordinal, entity_tag, EventSourceIndex, local_event_index)`. Causal or legacy emission order comes before deterministic entity/source/local tie-breakers, so `OrderAccepted -> AuctionTick` and Fill-before-Settlement remain intact.

## 最终 TickFrame / TickBatch 协议修订：2026-09-17

- 后端逐 tick 完整计算、结算并提交，提交后才生成 `TickFrame { tick, events, timeseries_payload, seq_from, seq_to }`。高加速只在传输层把已完成帧按递增 tick 号装入 `TickBatch`，不得合并边界或丢弃中间时间序列点。
- `timeseries_payload` 明确承载该 tick 的时间序列、竞价和图表数据。前端不得依据 `Event[]` 顺序重建图表或权威状态，当前图表、提示、日志和自动下单的顺序依赖尚未声称完成迁移。
- 帧内 `Event[]` 是事实集合。`seq` 只支持覆盖、去重、断线检测、重连和稳定重放游标。稳定字节键 `(phase_rank, entity_tag, EventSourceIndex, local_event_index)` 仅是序列化实现细节，不能用来推断跨实体业务顺序。

## 2026-09-17 历史条目隔离说明

- 本文件前文的 causal/legacy emission order、`causal_emission_ordinal`、`OrderAccepted → AuctionTick` 和 Fill-before-Settlement 表述属于已废止的中间方案，只保留作工作历史。现行协议以帧内事实集合、四字段稳定序列化键和逐 tick 事件身份与 payload 多重集比较为准。

## 最终 TickFrame / TickBatch 协议修订：2026-09-17

- 后端逐 tick 完整计算、结算并提交，提交后才生成 `TickFrame { tick, events, timeseries_payload, seq_from, seq_to }`。高加速只在传输层把已完成帧按递增 tick 号装入 `TickBatch`，不得合并边界或丢弃中间时间序列点。
- `timeseries_payload` 明确承载该 tick 的时间序列、竞价和图表数据。前端不得依据 `Event[]` 顺序重建图表或权威状态，当前图表、提示、日志和自动下单的顺序依赖尚未声称完成迁移。
- 帧内 `Event[]` 是事实集合。`seq` 只支持覆盖、去重、断线检测、重连和稳定重放游标。稳定字节键 `(phase_rank, entity_tag, EventSourceIndex, local_event_index)` 仅是序列化实现细节，不能用来推断跨实体业务顺序。

## Todo 2B partial implementation: exact strategy parameters

- The sealed attempt-12 verifier passed with manifest SHA 4661da6b62cb2d706b0ad359122627f5802cc97f55f4c15b16ba14c3dfe62796 before edits.
- Factory-sampled InstitutionMomentum parameters expose a one-bit JSON float decode drift. Enabling serde_json float_roundtrip globally changes existing pinned SaveSlot hashes; that attempted dependency feature change was removed. The new StrategyState representation uses hexadecimal float bits locally instead.
- SaveSlot is not a full shadow clone: pending plan events are filtered and diagnostic collectors are omitted. Do not implement the future rollback seam by treating save/restore as an exact session copy.
- This entry records partial work only. Result/poison/phase/commit/hash/frame/host delivery remains unimplemented by this execution.

## 2026-09-17 Todo 2B-1 verification closure only

- Fresh final-code targeted/full engine tests, clippy -D warnings, all three host checks, WASM threading, scoped rustfmt and diff-check passed. LSP on all seven files timed out; no clean LSP claim.
- Real round-trip QA reported exact profile/family/JSON for all four variants; malformed nonfinite bits returned an explicit serde error. Identity regression now asserts the exact IdentityMismatch variant.
- Independent reviewer bg_a785a4d4 reviewed every scoped change and returned PASS, including unchanged A-share semantics and minimal scope. DTOs remain untrusted until validated into_strategy conversion; no session persistence guarantee is implied.
- Evidence and bounded DoneClaim: .omo/evidence/escrow-parallel-engine/task-2/todo-2b-1-verification.md. Only slice 2B-1 is verified; top-level 2B remains open, plan checkbox unchanged, Todo 3-8 deferred.

## 2026-09-17 AdversarialVerify repair of 2B-1

- Prior profile/family consistency checks were insufficient: external Strategy implementations could forge matching state. Four adversarial tests reproduced accepted impossible identities/forgery before changes.
- Decision Strategy remains extensible. ProductionStrategy now requires a private sealed registry implemented only for the four concrete production types. Factory and reconstruction retain this capability; custom export fails compilation with E0277 on sealed::Registered.
- Factory-derived allowed identity matrix excludes Belief ActiveTrader and requires InstitutionMomentum outer ActiveTrader plus inner Momentum. IdentityMismatch is distinct from numeric InvalidParameters; unknown implementations are a compile-time error, unknown wire variants a serde error.
- Fresh full engine suite (including compile-fail doctest), targeted 4+3+1 tests, clippy, three host checks, wasm threading, format and scoped diff-check passed. LSP unavailable by timeout. Independent reviewer bg_06f6a084 PASS. Receipt: task-2/todo-2b-1-adversarial-repair.md. Top-level 2B remains open; no later task started.

## Todo 2B-1 证据修复：封闭能力与存储能力不是同一完成条件

- 已重写 `todo-2b-1-verification.md`，删除开放投影实现的过时结论，明确生产代码此前确实变更了封闭能力、工厂返回类型、具体实现和校验；本轮自身仅编辑证据。
- 当前基础/身份对抗/doc 测试新执行结果为 4/3/1 通过；外部伪造拒绝原因是 E0277 私有 `sealed::Registered` 上界，不是 source mismatch 运行期错误。
- Account 仍保存 `Box<dyn Strategy>`，擦除封闭生产能力；不会重新开放伪造，却是 2B-2/P2 shadow hydration 的硬前置依赖。权威状态提取前须迁移 Account/session 生产存储为 `Box<dyn ProductionStrategy>` 或等价封闭 wrapper，测试自定义注入保留独立仅测试/非权威路径。本轮未实施迁移。

## 2B-2 implementation

- Desktop repair: closing the actor plus stderr loses original StepFatal context. Both auto-loop paths now emit one dedicated engine-failure (STEP_FATAL, full message, empty events) before shutdown. Real mock-Tauri listeners prove delivery and no later step, with two observed red-to-green tests.

- Sealed pre-refactor identity must be verified in reconstructed source, not the post-2B-1 live tree. Isolated attempt-12 verifier passed; live stale rejection is the negative test. Original artifacts unchanged and owned worktree cleaned.
- Account and temporary decision storage now retain StoredStrategy's sealed production capability. Non-authoritative fixtures execute but cannot export/save/hash as production state.
- Result step/save, stored poison, typed hashes and cfg(test) pre-mutation failure tests passed. Successful 3 x 60 tick full projections preserve pre-change bytes. Receipt: task-2/todo-2b-2-done.md. Top-level Todo 2 remains open.

## 2026-09-18 Web protocol CORE

- Web consumers must treat a TickFrame as a semantic tick boundary even when frame-local event/fact arrays are permuted. Canonical replay sorts within each frame but never flattens events/facts across frames.
- `CivilUpdate.refresh.intraday` is synchronization history, not a new stream of action effects. Only outer CivilUpdate facts plus explicit barrier effects may be published on its first application.
- The generated Rust wire permits `TickBatch.runtime_snapshot: null`; reducer application retains the previous authoritative snapshot in that case, while a supplied snapshot must exactly match the final frame.
- Current environment lacks Node 24.18.0 and TypeScript LSP. Local `tsc`, oxlint and focused Node 25 strip-types tests are evidence only for their actual commands, not a substitute for pinned Corepack verification.
- A cached Node v24.11.1 later became available for focused protocol runtime tests. It is closer to the pinned major line but still must not be reported as the required v24.18.0 / pnpm 11.19 gate.
- The final independent review passed after mirroring `SessionSetup::validate` StockSpec constraints and the producer's `AfterClose` history-start formula. The Web boundary now rejects a shifted close-day history rather than trusting producer-only setup validity.

## 2026-09-18 Web protocol core boundary repair

- JavaScript `{}` 不能作为不可信协议 map 的中间写入容器；即使 JSON 的 `__proto__` 是自有键，赋值也会
  改变原型并使键在后续枚举前丢失。null-prototype record 加写前危险键拒绝是该边界的必要条件。
- Map 的键冲突判断必须先记录原始键类型和归一化目标；仅检查 `Map` 自身无法发现数值 `1` 与字符串 `"1"`
  这类转换后冲突。
- 宽度解析应紧贴 Rust 声明：u8/u32/u64/i64 不是可互换的“安全 number”。但 StockSpec 的业务关系
  不属于传输解析，未来 Rust 支持的证券组合不能被过期 Web 前缀表拒绝。

## 2026-09-18 Web host protocol integration

- 浏览器 WASM 包必须在 Rust `EngineUpdate` binding 变化后重建并复制；旧 `wasm-pkg` 会继续返回 `Event[]`，应由严格 parser 显式拒绝，不能在 Web 端兼容旧 flat shape。
- ProtocolCoordinator 是唯一 generation/cursor/replay 入口。baseline 必须清空 accepted replay 历史；exact retry 不通知 runtime；错误需以 code、where、message 到达 UI。
- `CivilUpdate.refresh.intraday` 只能重建图表与权威状态，不能重复触发历史 facts 的通知、成交或自动单。连续/竞价点均来自 timeseries，而非 Event[] 顺序。
- Worker restore 必须先交付并缓存新 generation baseline，再完成 restore request，否则 App 紧随其后的 snapshot 读取可能回到旧会话。
- 暂停偏好在 sessionStorage hydration 完成后才创建宿主，避免初始化闭包用默认 false 覆盖持久 true；默认仍是 false/false，未引入隔夜委托。
- Lightweight Charts v5 attribution anchor 满足该库 NOTICE 的用户可见链接要求；不得为视觉清理移除它。无数据图表的标记应按许可证与依赖约束保留。
- direct WASM restore 必须先记录 `wasRunning` 再 clear timer；用清空后的 timer 判断会令已运行会话读档后静默停住。
- 完整默认 authority save 超出 localStorage 原始 JSON 配额；在浏览器存储层 gzip/base64 后保留同一严格 JSON schema 和 legacy raw JSON 读取。WASM `restore_json` 接受该规范 JSON；直接 `serde_wasm_bindgen` Map restore 会将部分 i128 金额改为浮点而失败，不能替代 JSON restore。

## 2026-09-20 Resumable plan-chain repair

- A static request vector cannot preserve revision-dependent quotes. Account and BTreeMap stock continuations must complete before account budgeting; plan quote cursors retain PlanId and resolve synchronized current plans one at a time.
- Restructure, replace, and working-order cancellation now share private owned route continuations. Adoption remains a consumption-time non-routing operation. Fully filled legacy limit routes require the isolated Trade slice plus the newly appended matching Accepted fact because no OrderAccepted event is emitted for a zero remainder.
- Five reversible mutants were killed and restored with exact SHA-256 checks. Fresh full engine, strict Clippy, diagnostics, formatter, preservation (4 active B / 9 assertions), WASM threading, and diff gates passed. Independent scoped APPROVE: ses_f43ae341fffeW3VApzz3J69Lt6. Evidence supersedes task-3/plan-chain-candidate-seam-20260919.md historical claims.
- 完整默认 authority save 超出 localStorage 原始 JSON 配额；在浏览器存储层 gzip/base64 后保留同一严格 JSON schema 和 legacy raw JSON 读取。WASM `restore_json` 接受该规范 JSON；直接 `serde_wasm_bindgen` Map restore 会将部分 i128 金额改为浮点而失败，不能替代 JSON restore。

## 2026-09-20 P2 composition investigation (not implementation)

- NPC reconciliation currently executes cancellation wrappers before cash capping; moving the existing generator unchanged into P2 would allocate event sequences and mutate books. The pure reconciliation planner is the seam to separate from compatibility consumption.
- Plan-chain sealed resource propagation must cover execution cash/sellable budgeting and lifecycle held-quantity/equity sizing. AllocationSnapshot currently carries only available cash and sell shares, so passing it solely to the NPC cap is insufficient.

## 2026-09-20 Todo 3 preservation residual and aggregate checkpoint

- The apparent `restore.rs` assertion deletion is a paired `save() -> Result` formatting adaptation: the required restored `filled_qty == 200` assertion remains as a formatted replacement. The exact-C verifier metadata had a 65-character current hash; correcting it to the computed 64-character lexical hash and keeping the dedicated deletion mutant makes the production verifier fail closed for removal and pass for the retained assertion.
- `EnvelopeLedger::validate_conservation` sums each envelope's existing tagged basis and the same `spent + released + live` terms in separate `ResVec` dimensions. `apply` validates the candidate ledger before installation, so a failed aggregate/per-key equation cannot advance receipt state.

## 2026-09-20 Todo 3 exact-C assertion preservation repair

- Exact-C hunk identity alone cannot preserve an assertion when a historical zero-context deletion hash is allowlisted. A narrow required-assertion record must bind the containing Rust item, sealed baseline assertion identity/anchor, and repaired current normalized assertion hash.
- The repaired `restore.rs` entry binds assertion 7, the restored-session `filled_qty == 200` check. Deleting it now yields `EXPANDED` even if the supplied historical hunk hash matches.

## 2026-09-20 Todo 3 restore exact-C correction

- Independent review established that the historical zero-context deletion was Git diff pairing, not missing behavior: both `filled_qty == 200` assertions are present in the current restore item and its complete normalized Result body equals the sealed baseline.
- Therefore the restore exact-C exception and its required-assertion supplement were removed. Semantic deletion is rejected by ordinary full-item production classification, while the current Result/rustfmt-only item is generic mechanical.


## 2026-09-20 Todo 3 dual-book/receipt audit (read-only)

- Present core is partial: `Envelope::apply/validate` proves one resource equation against a single constructor basis, `EnvelopeLedger::apply` is transactional and sorts by `ReceiptLocalKey`, P0 emits real PreSeal releases, and P1 sums post-P0 live cash/shares. P3-created insertion, sealed receipts, retained per-tick journals, explicit dual-book equations, cross-receipt audit continuity, and aggregate-from-identical-per-key equations are not implemented; P2-P8 remain compatibility tokens.
- Fresh focused compilation exposed the current hard blocker: `cargo test -p engine session::pipeline::ledger_tests --lib -- --nocapture` fails E0599 because `ledger_tests.rs::conservation_aggregate_sums_the_same_cash_and_share_equations_per_key` calls absent `EnvelopeLedger::validate_conservation`. No green claim is valid at this state.
- The current receipt check is insufficient for the plan chain: `apply_one` only rejects qty regression/value regression and checks total charged delta. It does not require receipt before-fields to equal `EnvelopeAudit`, does not update audit after application, does not validate component-wise nominal/charged continuity, kind/journal legality, seller zero cash/spent.cash, or buyer reservation/release equations. Terminal removal also discards the envelope state, so later aggregate proof cannot be reconstructed unless a validated per-key conservation row or retained receipt journal is emitted before removal.
- Minimal model seam: make journal-specific transitions produce one `ConservationRow` per `EnvelopeKey` containing `tick_start_live`, `p0_released`, `p1_live`, optional `created`, sealed `spent/released`, and `commit_live`. Validate cash and shares separately per row; derive account/global totals only by checked summation of those already-validated rows. Existing envelopes use both equations (`tick_start=P0+p1`, then `p1=spent+released+commit`); P3 envelopes use only `created=spent+released+commit`.
- Receipt identity must be validated before mutation: canonical sort by `ReceiptLocalKey`, reject duplicate local keys, allocate contiguous global indices, reject duplicate `(envelope_key, receipt_index)`, then validate per-envelope chain in canonical order. Chain equality must include qty/value, remaining quantity, nominal and charged components, and resource live-before/live-after; a new source may restart its ordinal but may not restart state (auction rollover after-fields must equal DayEnd before-fields).
- Divergence #9 boundary belongs in receipt construction/validation, not envelope scalar conservation: Sell envelope cash, every Sell `spent.cash`, and terminal Sell `released.cash` are zero; charged delta is capped and split commission -> stamp tax -> transfer fee while preserving the existing cumulative nominal fee functions. Buy Fill uses the exact existing `buy_order_reservation` helper: 200 shares at limit 10.00 starts 2005.02; 100 at 9.00 spends 905.01, leaves 1000.01, releases 100.00 immediately.
- Conflict-free post-gate lanes: (A) `pipeline/envelope.rs` + `envelope_tests.rs` for journal-aware per-key rows/resource equations; (B) `pipeline/receipt_key.rs` + a new `receipt_key_tests.rs` for ordering/identity only; (C) a new pipeline fee-transition module + its tests, with only a narrow visibility change to the existing `session.rs::fee_delta`/`buy_order_reservation` helpers; (D) after A-C APIs freeze, `pipeline/ledger.rs` + `ledger_tests.rs` integrates transactional apply, chain state, row aggregation, and created-envelope insertion; (E) P0 adaptation stays isolated to `pipeline/p0_expiry.rs` + P0 tests. P3/P4/P5 integration in `pipeline/mod.rs` is dependent work, not a safe concurrent lane.
- Verification seams: focused unit tests must prove existing-vs-created equations, cash-vs-shares cross-leak rejection, P0 double release, checked negative release, duplicate local/pair identity, noncontiguous index, ordinal/state-chain break across SealedIntent/Auction/DayEnd, seller three-leg 1/1/10 fee cap with spent.cash=0, and buyer 2005.02/905.01/1000.01/100.00. Then run the plan-named `conservation*` tests, full engine suite, strict Clippy/fmt, preservation verifier, WASM threading, and diff-check; do not claim LSP clean unless separately observed.

## 2026-09-20 Todo 3 receipt-key subtask B

- `ReceiptLocalKey` validation now deliberately distinguishes canonical local-key order and contiguous `(ReceiptSource, EnvelopeKey)` ordinals from global index identity. A new source can reset its ordinal, but a gap inside one source/envelope domain is fatal.
- `ReceiptIndex` and `IndexedReceiptKey` are the stable handoff API for lane D: validate canonical local keys, local-envelope equality, unique `(EnvelopeKey, ReceiptIndex)`, and contiguous index assignment before transactional ledger mutation. This does not validate receipt state chains, journal-kind rules, resources, or fees.
- The initial focused receipt-key TDD run failed as intended for ordinal gaps and canonical-order mutations; the final targeted engine run passed ten tests. An intermediate retry was blocked by concurrent `transition.rs`/`ConservationRow` work, then became compilable without this subtask changing it. Rustfmt and no-excuse checks passed; all Rust LSP requests timed out.

## 2026-09-20 Todo 3 production mutation fixtures

- A production verifier mutation must be generated from, and written back to, the exact repository-relative current file under the detached baseline worktree. Copying only the verifier/inventory makes a live-checkout string replacement vulnerable to a no-op; require both `replace` success and a nonempty `git diff <baseline> -- packages/engine/tests` before asserting CLI rejection.

## 2026-09-20 Todo 3 subtask C pipeline fee transition

- The minimal reusable seam is `pub(crate)` visibility for existing `fee_delta` and `buy_order_reservation`; the pipeline transition module calls those helpers rather than recreating commission, stamp-tax, transfer-fee, or buy-reservation formulas.
- Seller nominal fee state is reconstructed from the authoritative cumulative helpers and actual collection is separately capped, then allocated commission → stamp tax → transfer. The pure transition leaves sell cash `spent`, `released`, and `live_after` at zero; it is intentionally not ledger or settlement wiring.
- For this gate, only `company_scenarios/restore.rs` and `company_scenarios/constraints.rs` need current-fixture overlays. The Class-B negative mutates inventory metadata, so it needs no `session.rs` overlay. The clean verifier remains `class_c_entries=3`; the redundant restore exact-C entry stays absent and both Rust `filled_qty == 200` assertions remain unchanged.

## 2026-09-20 Todo 3 envelope conservation-row subtask A

- `ConservationRow` keeps the dual-book bases explicit: TickStart rows contain `(tick_start_live, p0_released, p1_live)`, whereas P3-created rows contain only `created`. Validation keeps cash and shares in `ResVec` checked arithmetic and rejects a non-zero P0 contribution for P3-created rows.
- This atomic API does not move fee, seller-zero-cash, receipt-key, ledger, P0, P3/P4, or aggregate integration responsibilities. Seller cash and `spent.cash` boundaries remain preserved by not introducing an alternate fee formula or scalar accounting path.
- The focused post-edit cargo execution is presently blocked by an unrelated concurrent `pipeline/mod.rs` E0365 private transition-type re-export. Both changed Rust LSP diagnostics timed out after 30 seconds, so no LSP-clean claim is made. Formatting, diff-check, and ast-grep no-unwrap checks passed; the manual CLI/cleanup record is `task-3/envelope-conservation-row-subtask-a-20260920.md`.
- Independent review identified a negative-resource hole in standalone `Envelope::validate`: a matching negative basis/live state could conserve arithmetically. A red regression now proves it, and the validator rejects negative basis, spent, released, and live vectors before conservation math. The concurrent transition visibility issue was resolved outside this lane; final envelope tests pass 9/9.

## 2026-09-20 Todo 3 ledger subtask D

- Ledger application now builds a canonically sorted private candidate, assigns `ReceiptIndex`, and calls `validate_indexed_receipt_keys` before it mutates ledger state or returns sorted/indexed receipts to the caller. Failed input leaves the caller slice untouched.
- Terminal envelopes move to a separate retained map so P1/live projection iteration stays live-only while `ConservationRow` aggregate validation includes terminal rows until `reset_tick_state`. Hydration rejects a projected key that overlaps retained terminal evidence.
- Receipt checks consume the provided audit/fee/resource facts and do not recompute nominal fees. Seller receipts require zero cash envelope/spent/released resources and exact nonnegative `deliver_cash + charged == gross`; P3-P6 construction and settlement remain separate work.

## 2026-09-20 Todo 3 envelope responsibility split

- The real boundary is reusable resource/conservation model versus mutable live envelope state. `conservation.rs` now owns the one canonical `ResVec`, `ReceiptDelta`, `FeeComponents`, `ConservationBasis`, and `ConservationRow`; `envelope.rs` owns only origin/audit/state transitions. Parent pipeline re-exports preserve all pre-existing production paths, while conservation rows remain non-root internal API.
- Focused baseline and final suites are identical: envelope 9, receipt-key 10, transition 4, ledger 8. Public projection consumers also pass 4/4 plus hydration 1/1. `envelope.rs` is 112 pure LOC and `conservation.rs` 148; the manual CLI and cleanup receipt is `task-3/envelope-responsibility-split-20260920.md`.
- Rust LSP diagnostics timed out after 30 seconds for all changed Rust files. Formatting, scoped diff check, and ast-grep no-unwrap scan passed; no clean-LSP claim is made.

## 2026-09-20 Todo 3 ledger size refactor

- Preserve the root ledger API by making `ledger.rs` a public mutable-state facade and moving only private candidate normalization, receipt validation, and row aggregation into siblings. This avoids duplicate accounting concepts and leaves P0 call sites unchanged.
- Split ledger tests into fixtures plus state, receipt, and conservation contracts. The exact 16 contract names remain behavior-level regression coverage; every source/test sibling measured below 250 pure LOC.
- The preservation verifier still passes because this is source modularization with no protected existing-engine test assertion weakening. Rust LSP timed out for every moved module, so verification relies on focused suites, strict Clippy, formatter, preservation, and independent review rather than a false clean-LSP claim.

## 2026-09-20 P2 candidate-batch prerequisite

- The P2-to-P3 handoff can encode the approved `npc -> player -> plan_chain` class order without
  any mutable session reference by using an exhaustive source enum plus a tagged key whose local
  domains are `(AccountId, npc_local_index)`, `player_queue_index`, and
  `chain_generation_index`.
- Canonical batch deserialization must validate sorted unique keys instead of re-sorting input:
  source adapters can normalize before the boundary, while transfer/replay consumers fail loudly
  if a purported sealed batch is reordered.

## 2026-09-20 Player candidate nonempty side-effect regression

- A nonempty `PlayerCandidateBatch` capture now has a direct regression asserting one transferred tuple, an emptied authoritative queue, unchanged `next_order_id` and `seq`, and byte-identical public account/market snapshot. It preserves the existing `std::mem::take` ownership transfer; there is no P2 integration or Todo 3 completion claim.
- A duplicate-candidate source mutant was restored to SHA-256 `d3397969fd067d24789f9775c6ee98fd248ee631247b63fefebe9ebf8b98a1cb`, but the intended mutant test compilation was blocked by a concurrent missing `pipeline/p2_candidates` module. The pre-blocker focused regression passed; LSP diagnostics timed out.

## 2026-09-20 P2-to-P3 typed handoff checkpoint

- `P2P3Handoff` owns the canonical P2 batch, immutable DecisionResourceSnapshot, validated post-P0 ledger context, tick-start ID cursor, and immutable config. It never accepts GameSession or TickShadow, preventing draft validation from mutating authority, books, events, envelopes, receipt state, or counters.
- P3 consumes per-account P1 cash and T+1-aware sellable budgets in P2 key order. Buy demand reuses `buy_order_reservation`; seller cash demand is absent, so #9 remains zero-cash while sell quantities still consume independently tracked shares. Accepted placements only receive prospective OrderIds and unkeyed EnvelopeDraft values.

## 2026-09-20 P2 source-composition checkpoint

- Composition consumes only the three source-owned batch values. NPC local indexes are per account in source output order; player indexes preserve captured FIFO; plan-chain retains its checked generation index. Feeding the vector directly to `P2CandidateBatch::from_canonical` rejects duplicate or unordered source output instead of normalizing it.
- The adapter has no GameSession/TickShadow input and therefore cannot route, allocate global identity, write envelopes/receipts/events, or rerun strategy/reconciliation/continuation code. It feeds P2P3Handoff unchanged.
