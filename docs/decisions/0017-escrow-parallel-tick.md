# ADR-0017: escrow 并行 tick 模型、数据流契约与分歧台账

- **Status: accepted**；**状态：** accepted（已接受）
- **日期 (Date):** 2026-09-17
- **决策者 (Deciders):** msuad，依据 2026-09-17 用户决策记录

## 上下文 (Context)

引擎需要在保持现行沪深 A 股游戏语义边界的前提下，把一个 tick 拆成可验证的阶段，并让账户与股票处理在阶段边界使用并行执行。现有委托会同时涉及账户现金、可卖股份、订单簿、集合竞价、结算收据和策略决策。若没有单一的分配截点、显式收据来源、资源守恒方程和失败回滚边界，并行化会让同 tick 可见性、费用累计、事件顺序和存档状态产生隐式差异。

本 ADR 只规定游戏内引擎契约。托管 envelope 是游戏内资源预留与审计模型，不是交易所的真实清算机制。现行 A 股的 T+1、集合竞价、价格笼子、涨跌停、价格时间优先和费用名义函数仍以 [trading-rules.md](../trading-rules.md)、[ADR-0009](0009-call-auction-and-intraday-axis.md) 与 [ADR-0014](0014-closing-call-auction.md) 为基线。

## 决策 (Decision)

### 1. 单一并行实现与阶段边界

采用一条阶段化并行实现，不建立独立串行参考引擎，也不以不同实现路径作为正确性依据。预算为 1 的并行运行只是调度对照条件，不是第二套实现。native 与 WASM 保持同一 rayon 并行代码路径，WASM 保留 `wasm-bindgen-rayon`。引擎不引入 channel、actor、跨实体业务锁或阻塞发送。

权威状态只有 `commit_tick` 可以改变。其余阶段都写 shadow 状态和局部 outbox，失败时不把部分结果写回权威状态。

阶段顺序如下：

1. **P0 过期 shadow**：移除报价过期的挂单，释放其 shadow envelope，产生封前账本收据。报价过期是本 tick 分配前可见的唯一释放例外。日界清簿属于密封批，不在 P0 提前释放。
2. **P1 分配截点**：建立不可变 `seal_allocation_snapshot`。预算使用 P0 之后的 live 存量，且不另加独立释放项：`P1_available_cash = tick_start_cash - post_P0_live_buy_cash`，`P1_available_sell_qty(account, stock) = tick_start_sellable - post_P0_live_sell_qty`。卖单现金占用恒为 0。P0 的释放已体现在 post-P0 存量中，不得重复计入。
3. **P2 决策 shadow**：从 `DecisionSnapshot` 读取状态。所有策略先通过封闭的 `StrategyState` 枚举重 hydrated 到影子竞技场，P2 只操作影子策略。
4. **P3 账户校验与 envelope 草案**：按总密封序逐账户处理候选 `Place`，使用私有剩余预算产生未键控 `EnvelopeDraft` 和 validated/rejected 掩码。同批释放不回补 P3 预算。以 tick 起点 `next_order_id` 对接受数做 checked 分配，溢出返回 `StepFatal::InvariantViolation`，丢弃本 tick 所有草案、操作、收据和事件，不产生键控 envelope 或订单簿变化。
5. **P4 股票处理**：按 `StockCode` 并行，每只股票是从 post-P0 shadow orderbook 出发的确定性顺序状态机，按总密封序应用操作。这里执行笼子、涨跌停、连续撮合、竞价、撤单、替换和日界终结。worker 只写本股票 shadow 与 outbox。
6. **P5 收据聚合**：按显式 `ReceiptLocalKey` 聚合并校验收据，分配连续的全局 `receipt_index`。收据先经过唯一性、双账本方程和 journal 标记校验，再交给结算。
7. **P6 结算 shadow**：按账户分组，正数量 Fill 增量形成 `SettlementTotals`，同账户同股票按 Buy 再 Sell 的生命周期顺序应用。结算只消费收据中的实收费用增量，不重算 P4 的费用或成交 delta。
8. **P7 派生与审计**：生成诊断、状态投影和存档候选。失败仍停留在 shadow，不能改变权威状态。
9. **P8 双哈希检查**：比较 `business_state_hash` 与 `session_state_hash`，确认提交前状态满足回滚判据。
10. **P9 `commit_tick`**：单点提交全部 shadow 状态、计数器、RNG 游标、封闭枚举 `StrategyState`、envelope 账本和事件。提交成功后，快照立即反映释放与入账，存档只允许从这一合法静默点产生。

### 2. 数据流契约

权威状态包括 accounts 的 cash、positions、T+1，markets 与订单簿、竞价队列、plans、envelope 账本、RNG 游标、`next_order_id`、`seq`、`next_receipt_base`、诊断，以及每个策略实例完整的封闭枚举 `StrategyState`。`StrategyState` 是策略持久状态的唯一权威；展示用 profile 必须从它派生，不能与另一份策略对象或 profile 副本并列为权威。`DecisionSnapshot` 是 P2 的唯一输入快照，NPC、玩家和 plan chain 都只能读取它。

envelope 键为 `(account_id, stock_code, order_id, side)`，资源向量为 `ResVec { cash: Money, shares: u32 }`。P0 后的封前账本按 envelope 满足 `tick_start_live = P0_released + P1_live`。密封批账本满足：已有 P1 envelope 为 `P1_live = ΣP4 spent + ΣP4 released + commit_live`，P3 新建 envelope 为 `created = ΣP4 spent + ΣP4 released + commit_live`。cash 与 shares 分别守恒，不能混成一个标量；账户聚合必须是逐键方程的逐资源求和。

每张收据至少携带 `receipt_index`、journal、`ReceiptLocalKey`、envelope_key、kind、成交前后数量与金额、限价、`spent`、`released`、`live_after`、三项费用增量、`deliver_qty` 与 `deliver_cash`。卖方还携带 nominal 应计增量、实收增量及 `charged_before/after` 审计字段。`deliver_*` 是净结算交付，不进入 envelope 守恒方程。

买单现金腿保持现有名义费用和逐次释放语义：`spent.cash = gross_delta + commission_delta + transfer_delta`，成交价不高于限价；`live_after.cash` 按现有买单预留 helper 重算；`released.cash = max(0, live_before.cash - spent.cash - live_after.cash)`，价格改善差额逐次释放。终结转移统一把 `live_after` 置零，`released = live_before - spent`，使用 checked 运算。

卖单现金腿按分歧 #9 执行：nominal 佣金、印花税、过户费公式、费率和最低佣金累计口径保持不变，但实际收取总额按每一成交腿封顶。令 `F` 为累计 nominal 应计费用，`charged` 为累计已实收费用，则本腿 `charged_delta = min(F_after - charged_before, gross_delta)`，`deliver_cash = gross_delta - charged_delta >= 0`，`spent.cash = 0`，卖方 cash envelope 分量恒为 0。每单分别持久化三项费用的累计应计和累计实收。本腿先算三项应计增量，再按固定顺序 **佣金 → 印花税 → 过户费** 分配封顶额度，每项实收增量为该项应计未收余额与剩余总额度的较小值。这样每项增量非负、不超过其应计余额，三项实收之和恰等于总实收增量。若早期成交腿因所得不足形成 `charged_before < F_before`，后续较大成交腿在剩余封顶额度内追收历史欠费。此规则是游戏内简化，不等同于交易所真实清算。

### 3. 事件变体表

#### 自然日更新屏障（2026-09-18 用户确认）

传输使用有序 `TickBatch | CivilUpdate` 联合。自然日日结继续走现有
`end_civil_day`，不伪造市场 tick，也不移入撮合提交。`CivilUpdate` 保持 tick
不变，覆盖其实际产生的全局 seq；后续 TickBatch 必须从这一 seq 游标继续。
同一 TickBatch 内 tick 连续递增、seq 覆盖连续；跨自然日屏障时，全局 seq
连续性由两个更新类型共同维护。消费者必须先完整应用 CivilUpdate，再应用后续帧。

CivilUpdate 显式区分 `AfterClose`、`BeforeOpen` 与确有需要的普通自然日前进。
同一次日结同时完成收盘刷新并抵达下一交易日时，可明确携带两个屏障标记，
不得为了多发一个屏障而重复日结或分配虚假事件 seq。收盘刷新必须保留完整当日
分时与竞价序列，并携带权威日 K、当前 K 线和行情；开盘前刷新还需提供证券资料、
公开信息版本或披露 ID，使重连客户端不依赖旧事件数组即可同步。

`pause_after_close`、`pause_before_open` 是宿主调度偏好，默认均为 false，
保持自动推进。启用时先交付完整屏障再暂停，显式恢复不得重复日结或重放屏障。
用户希望的隔夜委托不在任务 2 实现，不改变当日委托有效期、撮合或集合竞价规则。
本节是实现约束，不表示三宿主与前端迁移已经完成。

后端严格一次完整计算、结算并提交一个 tick。提交成功后，才生成一个 `TickFrame { tick, events, timeseries_payload, seq_from, seq_to }`，然后才能开始下一个 tick。`timeseries_payload` 明确包含该 tick 的分时线、竞价和绘图数据，是该 tick 时间序列数据的协议来源；前端不得依据 `Event[]` 数组顺序重建图表数据或权威状态。高加速只允许传输层把多个已完成的 `TickFrame` 按 tick 号严格递增装入有序 `TickBatch`，必须保留每一帧，不得压扁语义 tick 边界、重排 tick 或丢弃中间帧的时间序列点。`TickBatch` 可以只在最后一帧之后附带一份完整 runtime snapshot，作为批次结束后的权威最终状态。前端可以逐帧处理，也可以先吸收多帧后只渲染一次；渲染合并仅属于前端内部行为，不是协议或业务语义。现有 frontend protocol 继续保持 events-before-final-snapshot 与严格连续 seq。

帧内 `Event[]` 是事实集合。数组位置与 `seq` 都不表达业务因果或执行先后，消费者不得据此推断跨实体业务顺序；例如 `OrderAccepted` 与 `AuctionTick` 的数组排列互换，单独不能构成业务分歧。仍须保留同一实体内的 FIFO、订单 ID、收据身份和权威状态语义，但这不允许重排 tick 或丢弃事件。现有前端中依赖事件顺序的图表、提示、日志和自动下单路径，后续必须迁移到 `TickFrame.timeseries_payload` 或按事件类型分组处理；本 ADR 不声称这些迁移已经完成。

后端仍为确定性稳定字节输出使用唯一键 `(phase_rank, entity_tag, EventSourceIndex, local_event_index)`，键碰撞即 `InvariantViolation`。该键是序列化实现细节，消费者不得把它解释为跨实体业务顺序。`entity_tag` 的全序为 `Stock(code) < Account(id) < Session`。`EventSourceIndex` 是 tagged 枚举，不使用标量哨兵：`Sealed=0`、`P0=1`、`PriceTick=2`、`DayEnd=3`、`Session=4`。

| 事件变体 | phase_rank | entity_tag | EventSourceIndex 来源 | local_event_index 派生 |
| --- | ---: | --- | --- | --- |
| `Trade` | 4 | `Stock(code)` | `Sealed` | 该股票密封序内成交腿序 |
| `AuctionTick` | 4 | `Stock(code)` | `PriceTick` | 该股票竞价 tick 序 |
| `AuctionCompleted` | 4 | `Stock(code)` | `PriceTick` | 该股票竞价完成序 |
| `PriceTick` | 4 | `Stock(code)` | `PriceTick` | 该股票价格记录序 |
| `DayBoundary` | 5 | `Session` | `DayEnd` | 日界操作序 |
| `CivilDateAdvanced` | 6 | `Session` | `Session` | 共享 Session 发出流序 |
| `CompanyDisclosurePublished` | 6 | `Session` | `Session` | 共享 Session 发出流序 |
| `IntentRejected` | 4 | `Account(id)` | `Sealed` | 该账户拒绝操作序 |
| `SettlementError` | 4 | `Account(id)` | `Sealed` | 仅旧事件形状；按分歧 #5 迁移为 `IntentRejected` |
| `ResourceLimit` | 6 | `Session` | `Session` | 共享 Session 发出流序 |
| `OrderCanceled` | 4 | `Account(id)` | `Sealed` | 该账户撤单操作序 |
| `OrderAccepted` | 4 | `Account(id)` | `Sealed` | 该账户接受操作序 |

`CivilDateAdvanced`、`CompanyDisclosurePublished` 与 `ResourceLimit` 共享 `(phase_rank=6, entity_tag=Session, EventSourceIndex=Session)`，因此三者必须共用一个确定性的 Session 发出流序号域，不能分别从各自变体计数。该共享 `local_event_index` 依 canonical phase processing 后 P7/P8 outbox 的插入顺序派生；P7/P8 只按已确定的阶段与实体规则追加 Session 事件，不由 executor 完成顺序选择。一个单调递增的 index 覆盖全部 phase-6、Session-tagged 事件；任何重复键均为 `InvariantViolation`。这里的稳定序号只服务于确定性序列化、去重和回放，不赋予跨实体事件业务先后。

`ResourceLimit` 的当前 payload 只有 `seq`、`resource` 和 `limit`，没有 account ID，不能派生出 `Account(id)`；因此该变体必须使用全会话唯一的 `Session` entity tag、`Session` EventSourceIndex，并使用上述共享 Session 发出流序号，不添加产品字段或虚构账户 ID。

上述是当前 `Event` 枚举的固定来源映射，实施时用 Rust 穷举 `match` 覆盖全部变体。`SettlementError` 是现有事件形状的迁移锚点，按分歧 #5 不作为新的业务失败路径，目标路径统一产生 `IntentRejected`。释放与 rollover 仍通过收据 `kind` 表示，不凭空增加未列入 `Event` 枚举的事件变体。收据使用 `ReceiptLocalKey = (journal_rank, ReceiptSource, envelope_key, transition_ordinal_within_source)`：`PreSeal=0 < SealedBatch=1`；PreSeal 只有 `P0Expiry=0`；SealedBatch 为 `SealedIntent=0 < Auction=1 < DayEnd=2`，随后按 payload、envelope_key、source-local ordinal 升序。`SealedIntent` 用 sealed index，P0 过期按股票和 order id 编址，竞价完成与日终影响的前 tick 挂单用确定性 envelope 序数。密封成交中每个参与 envelope 独立编号，成交腿为 0..k，终结转移为 k+1。

### 4. 毒化与双哈希

`step(&mut self) -> Result<Vec<Event>, StepFatal>`。`StepFatal` 只有 `InvariantViolation(描述与定位)` 和 `Internal(状态哈希不等)` 两类，分别表示类型化失败或已检出的不变量违规。业务拒单是 `IntentRejected` 事件，不是 fatal。panic 是进程级故障，不承诺恢复，也不使用 `catch_unwind`。poisoned 会话的后续 `step` 与 `save` 必须显式返回错误。

`business_state_hash` 覆盖 accounts、markets、plans、envelope、RNG、订单与策略业务状态，但排除 poison 和错误元数据。任何失败 tick 前后必须相等。`session_state_hash` 覆盖完整会话状态，只允许 poison 字段和错误元数据发生预定差异。字段枚举必须固定，不能用“忽略其它字段”替代枚举。`commit_tick` 之外不得修改权威状态；毒化发生时所有 shadow、计数器、键控 envelope、订单簿变化和事件均丢弃。

### 5. 九条分歧台账

| # | 分歧 | 采用行为与边界 | 依据/参考 |
| ---: | --- | --- | --- |
| 1 | 决策链读取快照 | 决策链改读不可变 `DecisionSnapshot`，类序仍为 `npc → player → plan_chain`；只改变链所见状态，不改变现行类序。 | `session/decision_chain.rs:124-143`，计划 Todo 1 |
| 2 | 分配截点与释放可见性 | P0 报价过期释放在截点前可用于本 tick 决策；成交、撤单、拒单、竞价完成和日界释放属于密封批，次一密封才可分配；P9 提交后快照立即反映释放。 | `seal_allocation_snapshot`，计划 Todo 1 |
| 3 | worker 内拒单 ID | P3 接受的 Place 先取得密封序，P4 被拒仍消耗其预分配 ID；P3 校验拒绝和 Cancel 不分配 ID。 | 计划 Todo 1 |
| 4 | 跨 envelope 同 tick 撤单 | 取消跨 envelope 同 tick 撤单，只允许既有可取消委托按阶段规则处理；同 envelope 内的操作语义保留。 | 计划 Todo 1，ADR-0009 |
| 5 | SettlementError 位置 | 由托管预拨确保的下单前约束迁移到 `IntentRejected`；结算阶段的类型化不变量失败仍是 `StepFatal`，不能静默吞掉。 | `account.rs` 结算边界，计划 Todo 1 |
| 6 | 全局 seq 与比较 | 全局 `seq` 仅用于覆盖范围、去重、断线检测、重连和稳定重放游标，不表示业务因果或执行顺序。新旧语料比较按 tick 分界，以稳定事件身份和业务载荷做多重集比较；仅数组重排不构成业务分歧。仍须保留同一实体 FIFO、订单 ID、收据身份和权威状态语义；不得重排 tick、丢弃事件或丢弃中间时间序列点。新 `seq` 必须满足覆盖范围要求，并在协议允许的范围内连续唯一。 | 计划 Todo 1 与 Todo 2 的逐 tick 比较契约 |
| 7 | 存档版本 | 存档升为 v2，旧档显式拒绝，不提供迁移器。合法存档点只有成功 `commit_tick` 之后。 | ADR-0009 §2026-09-08 修订，计划 Todo 1 |
| 8 | 笼子校验位置 | 现金、可卖、整手零股和委托数量上限在账户阶段校验；价格笼子与涨跌停在股票处理阶段校验。P4 拒绝产生事件并消耗已分配 ID。 | `trading-rules.md:21-23`，计划 Todo 1 |
| 9 | 卖单费用实收封顶与零现金预留 | 用户于 2026-09-17 明示裁定：卖单 nominal 费用函数、税率、最低佣金累计口径不变，实际每腿实收总额为 `min(F_after - charged_before, 本腿成交额)`；分项按佣金、印花税、过户费顺序拆分；`deliver_cash >= 0`，卖单 `spent.cash` 和 cash escrow 恒为 0。正常交易在封顶不触发时不受影响。旧预留大于可用现金时旧引擎拒单、新引擎接受；仅旧预留为 0 的卖单适用零现金接受相等断言。差异是游戏内简化，不是交易所清算规则，也不是一般券商规则。 | 可跟踪的用户决策记录：`.omo/plans/escrow-parallel-engine.md` 的 “#9 决策记录（替代批准门禁）”；旧实现 `session.rs:1142-1168,2717-2748` |

除 #9 明确批准的卖单实收封顶和卖单零现金预留外，本 ADR 不修改 nominal 费用函数、费率、最低佣金累计口径、买方费用语义、T+1、集合竞价、涨跌停或自成交政策。

## 备选方案 (Alternatives Considered)

- **独立串行参考引擎**：不采用。用户已明确只接受单一并行实现，预算为 1 的并行执行承担调度对照职责。
- **跨实体锁、channel 或 actor 管线**：不采用。它们会把实体间等待引入撮合核心，模糊阶段可见性与单写者边界。
- **把卖单费用作为现金预留解决**：不采用。用户裁定卖单现金 escrow 恒为 0，实际费用由每腿所得封顶并按固定分项顺序实收。
- **把托管写成真实交易所清算**：不采用。envelope 只服务于游戏内预留、收据和守恒审计，不能对外宣称真实清算机制。

## 后果 (Consequences)

- **正面：** 阶段可见性、资源守恒、收据顺序、失败回滚和存档静默点均有可审计契约；账户与股票阶段可以在不共享可变权威状态的前提下并行。
- **负面：** shadow、双账本、事件键和策略重 hydration 增加实现与测试复杂度。有限语料、跨预算和扰动检查只能提供受覆盖条件下的证据，不构成整个系统正确性、绝对无死锁或固定性能的证明。
- **后续需要做的：** Todo 2 依本 ADR 冻结数据流与比较键；Todo 3 至 9 实现守恒、校验拆分、收据结算、状态机、失败模型、存档和确定性证据；宿主显式映射 fatal 错误，不得静默吞错。

## 关联 (Related)

- [ADR-0008](0008-gpu-and-compute-offload.md)：rayon 与跨股票并行方向。
- [ADR-0009](0009-call-auction-and-intraday-axis.md)：开盘集合竞价阶段与顺序。
- [ADR-0010](0010-unified-host-protocol-and-local-refresh.md)：三宿主统一语义与错误交付。
- [ADR-0014](0014-closing-call-auction.md)：收盘集合竞价阶段。
- [`trading-rules.md`](../trading-rules.md)：A 股规则基线与游戏简化边界。
- [`open-questions.md`](../open-questions.md)：Q13。
