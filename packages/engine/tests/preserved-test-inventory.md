# 保留测试台账（Todo 3）

测试基线是 sealed attempt-12 的 commit `7041d35dc362ca74f4f3313e6804db9499f0679a` 加 `overlay.json`。verifier 读取 overlay 而不硬编码；当前 overlay 不含 `packages/engine/tests/**`，所以现有测试字节等于该 commit。

## (a) 未改动保护

未出现在 diff 的保护符号必须保持 baseline 字节。它们不是 hunk，不得伪造成变更。

## (b) 冻结 #9，严格四项

| 文件 | 符号 | effect id |
| --- | --- | --- |
| `packages/engine/src/session.rs` | `planned_sell_fee_is_reserved_before_a_later_buy` | `E9-A_SELL_CASH_RESERVATION` |
| `packages/engine/tests/session.rs` | `sell_order_is_rejected_when_cash_cannot_cover_fee_shortfall` | `E9-B_SELL_ACCEPTANCE_FLIP` |
| `packages/engine/tests/session.rs` | `sell_order_reserves_fees_for_a_possible_small_partial_fill` | `E9-B_SELL_ACCEPTANCE_FLIP` |
| `packages/engine/tests/session.rs` | `buy_and_sell_orders_share_one_cash_reservation_budget` | `E9-A_SELL_CASH_RESERVATION` |

`company_scenarios/constraints.rs:90-94` 是 plan soft-budget `available_cash`/`allocated_cash=500`，不是订单卖方 `reserved_cash`，明确排除。直接 accepted resting seller cash assertion anchor 为无。机器源为同目录 `preserved-test-inventory.json`：A=2、B=4、exact C=4、additive C=12。

## (c) 精确 Todo 2 合同改写

`step/save -> Result` 的 `expect("healthy step/save")` unwrap 及其 rustfmt 相邻片段允许；禁止借此改变 reservation、available cash、acceptance/rejection、fee、matching、plan input 或 decision output。

| 文件 | 符号 | normalized SHA-256 | 理由 | 禁止扩张 |
| --- | --- | --- | --- | --- |
| `packages/engine/tests/account.rs` | `reexport_from_crate_root` | `e988cf9ae659f38ba8074c60ef13eb27ba9a7117ad38ee80ca5f948b93a8c850` | Todo 2B-2 non-authoritative strategy production export capability boundary。 | reservation, available-cash, acceptance/rejection, fee, matching, decision-output |
| `packages/engine/tests/company_event_contract.rs` | `announcement_event_follows_successful_immutable_library_insertion` | `82fbe6405eec88e3ecd5fc3003780a00438f46e088ae0e09bc2a78f53b378ffe` | Todo 2B-4 immutable announcement public index。 | trading, reservation, acceptance, decision |
| `packages/engine/tests/company_event_contract.rs` | `civil_report_refresh_validates_and_reconnect_resolves_publication` | `44703e83b60214b0c16e257a56ca7f78c459f6098f70bb62de19f13aee657eb1` | Todo 2B-4 CivilUpdate reconnect/API。 | trading, reservation, acceptance, decision |
| `packages/engine/tests/company_scenarios/restore.rs` | `live_partial_fill_restores_and_continues_identically` | `dfd70220f09e77227a08fc98e4eb57c843db7b7752d3cb649b6509e03553c10d` | Todo 2B-2 `save() -> Result` rustfmt-split adaptation。 | reservation, available-cash, acceptance/rejection, plan input, matching, fee, decision-output |

最后一项额外强制保留 `uninterrupted` 与 `restored` 两个 `filled_qty == 200` 断言。

12 个 untracked Todo 2 contract 测试按 `scripts/simulation/verify-preserved-tests.mjs` 中的路径及 SHA-256 allowlist 接受；同路径 replacement 或任何未列入文件均拒绝。

## 已批准的交易分歧测试迁移

### 分歧 #4：同 tick 新建订单不可撤

`packages/engine/tests/auction.rs::auction_order_can_be_canceled_during_the_first_third`
原来把 Place 与 Cancel 放在同一次公共 `step()`，与 ADR-0017 已批准的“同 tick
新建订单不可撤”相冲突。迁移严格限于：把合法的首三分之一撤单场景拆为两个 tick，
保留并加强成交所无关的订单身份、剩余量和买方预留释放断言；另增
`same_tick_auction_place_and_cancel_is_rejected_and_preserves_the_order`，锁定同 tick
场景必须产生 `SameTickOrderNotCancelable`、不得产生 `OrderCanceled`，且原订单与
`1_000_510` 分买方预留保持存活。未改变竞价可撤窗口、费用公式、撮合、T+1、
笼子或涨跌停语义。

机器源为同目录 `preserved-test-inventory.json` 的
`approved_divergence_changes`。该条记录包含基线/当前符号哈希、六个精确 hunk
哈希及允许变换；B5 校验器必须按该独立类别消费，不能将其误归 #9 的 (b) 或
API-only 的 (c)。

### 分歧 #6/#7：事件稳定排序与存档 v2 表示

`extraction_replay.rs` 与 `c434f1d` 的双侧真实输出已做结构化比较。两侧均为
1,429 个事件；删除展示 `seq` 后排序，完整事件多重集 SHA-256 同为
`550dc6bd1fdf6198af7bfbb436db04d03bc49f46c575b92e0f9c219e46e650b7`。
21 个位置差异全部是同 tick 内 `AuctionTick/AuctionCompleted` 或
`OrderAccepted/OrderCanceled/PriceTick` 的跨实体稳定重排，未删除事件、未移动 tick、
未改变订单身份或业务载荷，严格归分歧 #6。

复核返修保留了 720 tick 的双侧原始分组输出，并逐 tick 比较事件数量、
`phase-tag + entity` 身份多重集、仅删除顶层展示 `seq` 的完整 payload 多重集及同实体
FIFO；全部相等。`OrderAccepted/OrderCanceled` 身份事实也逐序相等。9 个 tick 共 21 个
展示位置发生跨实体重排，逐位置所有者记录在工作区
`.tmp/b1-complete-module/evidence/d6-d7/structured-comparison.json`；该报告同时记录 8 个
原始输入的字节数与 SHA-256，并以独立规范编码得到双侧相同的业务多重集 SHA-256
`96109015dc4a4980085996cd2a87bd9973c4a2229bbd9b27d0850ca36df97f5c`。

两个存档删除 `schema_version`、`runtime_v2`、旧 `strategy_profiles` 并统一 policy
字符串后，其余字段逐字段相同；中途/末尾公共字段 SHA-256 分别为
`615ea983643d7a97c21b6425194b88610b6e1c98b22ca403318c0d5a6316acd9` 与
`dfedaeaae2ed7bc0fcbb805ef7ac27d0bd34ee300f464e1dc244c3ea72ec7197`。
从完整 `StrategyState` 派生的 profile 与旧 profile 亦精确相同，归分歧 #7。
因此只重钉 FNV，不扩大允许字段。

明确字段变换清单为：增加顶层 `schema_version=2`；policy id 从 v1 改为 v2；删除顶层
`strategy_profiles` 并增加 `runtime_v2.strategy_states`；另增加
`runtime_v2.poisoned/live_envelopes/next_receipt_base/retail_projection_seen`。中途值依次为
`false/空/0/空`，末尾为 `false/空/2/2 项`；两处均有 16 个 StrategyState，且逐账户派生
的 16 个旧 profile 精确相等。应用且仅应用上述变换后，其余权威字段规范 SHA-256
分别为 `506460fea072c5b4f18fc8b6e1da52be4ec2d92f6026d17619e7bd5fe5f7ac30`
与 `726ba581f9a164af7daebffd60af784246fc92d057f2a64dd9e94e551e382857`，双侧逐字段相等。

`step_skeleton.rs::characterization_complete_projections` 是零参与者、零订单场景，
摘要包含 SaveSlot；其三个摘要仅随 policy v2 与 `runtime_v2` 表示重钉。价格、tick、
日界与事件仍由相邻独立 characterization 精确断言。两个文件的允许/禁止范围和
完整 SHA-256 记录在机器源的 `approved_divergence_changes`；`step_skeleton.rs` 的
Todo 2 additive 文件 SHA 同步更新为受控的新值。
