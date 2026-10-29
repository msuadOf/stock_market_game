# Session 五项集成失败修复记录（2026-10-30）

## 范围与原始红证据

- 独占 worktree：`.worktree/last-integration38-session-2026-10-30`；分支 `codex/last-integration38-session-20261030`。
- 只改 `packages/engine/tests/session.rs` 与本记录，不改生产实现、交易制度或官方规则依据。
- 原始完整失败输入：主工作区 `agents/branch-completion/validation-scheduler-2026-10-29/integration-unreached-e1af5aaa-2026-10-30/failed-cases.json`，SHA-256 `01667fb4ffe730745022229688e9f24ce0cb21dec177f384f155fd08d63c855a`，已亲自读取对应五条完整错误。
- `dated_candles_bridge_virtual_prehistory_and_real_sessions_across_weekend` 的完整 left/right 经 token 比对：所有业务字段相等；只有 `mirror_origin` 的 `authority_epoch`、`install_sequence`、`source_generation`、`day_end_stage`、`non_tick_revision` 不同。`source_tick=20`、`mirror_phase=Continuous` 两侧相同。原运行来源为 `(1,24,20,22,RetainedPublished,3,Continuous)`；恢复未创建 World 来源为 `(0,0,20,0,Tick,0,Continuous)`。

## 分类与实现

1. `save_restore_preserves_rng_and_strategy_price_history`：旧驱动直接跨交易日继续 `step()`，违反独立自然日权威。复用既有 `advance_civil_to_session_start`，保留原继续运行断言，并新增恢复瞬间 `rng_state`、`price_history`、全部 `npc_attention` 精确相等。
2. `day_boundary_clears_daily_orders_and_releases_reservations`：清簿后先完成自然日日结，再提交下一交易日买单。保留原清簿与成功受理断言，新增自然日 `2030-01-03`、`tick=2/day=1` 精确检查。
3. 周末日 K：保留 362 根 K、日期、虚拟/实际统计及空活跃 K 的全部原断言。完整 `SaveSlot` 使用 `GameSession::save(&restored)` 重新投影比较，避免只比较 ProtocolSession 缓存；原会话与恢复会话各自七个来源字段完整断言，然后完整比较全部 Snapshot 业务字段。
4. `all_stocks_produce_trades_multistock`：原 fixture 从合法初始流通盘账户手工注入旧全局 `OrderId`。不改账户、序号、输入迭代顺序、资金或费用。原成交合同显式走既有 `step_with_commit_evidence().0` serial 入口；同一 SaveSlot 另外验证 native 精确拒绝与完整状态不变。
5. `restore_preserves_sparse_same_price_fifo_before_matching`：原 `OrderId(1/2)` 对应 sparse `seq=9/3`，AccountId(2) 必须先成交，不能用重排或重编号改成 desk 身份。保留 `[3,9]` 恢复顺序、AccountId(2) 成交 100 股 @1000 分与 AccountId(1) 余单断言；原 matching 显式走 serial，另以同档验证 native 精确 FIFO 拒绝与完整状态不变。

## 旧簿与公共加载合同的历史依据

- `docs/trading-rules.md:644–656` 的完整有限 handover 条款来自既有提交 `cd6e88c2aa3f9247049a2c912ed42c708f605a70`，提交时间 `2026-10-28 05:24:35 +0800`；本轮未修改该条款。其明确登记完整合法旧簿 handover 尚未实现，第二股旧全局 ID 无法原值编码、旧身份与同价 FIFO 冲突等情形会在 spawn/authority 替换前拒绝；不得重编号或改变 FIFO。
- 同一提交首次加入 `packages/engine/src/runtime/drive/static_projection.rs:21–53` 对上述身份、FIFO、公开 cutoff 的显式门禁，当前仍沿用。不是根据本轮失败临时反推 unsupported。
- `docs/decisions/0025-day-end-only-persistence.md:30–33` 第 8 条（2026-10-01 既有决策）要求公共加载只接受完整日结，拒绝活动委托、冻结 envelope、待处理请求；`docs/architecture.md:91` 明确 native 恢复是日终静点重建，活动委托不进档。
- `packages/engine/src/session/protocol/civil/session.rs:252–277` 的公共 `ProtocolSession::restore` 以 fault3013/3014落实上述日级合同；本次两项旧簿 fixture 直接走低层 `GameSession::restore`，属于内存旧簿合同，不是可公开持久化的日终档。
- native 负控显式调用 `step_via_runtime`，核对完整 `StepFatal::InvariantViolation` 的 `location` 与 `description`；拒绝前后比较完整 `SaveSlot`、完整 `Snapshot` 与 `business_state_hash`，覆盖 OrderId、FIFO、账户、游标、费用、T+1、日期与随机事实。负控只在 native 编译，不让 wasm serial 测试强制创建 World。
- 沿用 `docs/trading-rules.md` 已核验的交易所价格时间优先及日历依据，本批没有新增交易制度，未声称重新联网核验全部官方原件或已实现完整 native 旧簿接管。

## 验证与独立复核

- 已有五个精确 case 的失败记录作为红证据；实现者未自行运行 Cargo、metadata、target 或测试 binary。
- `rustfmt --edition 2021 --check packages/engine/tests/session.rs` 与 `git diff --check` 已通过；源码 SHA-256 `13624176bcd901ac1b39bd454c99171af540509fe0997c8917a85dadac6ef02a`。
- 唯一验证调度器 `/root/sol_integration_29` 只接收净接后的 dev HEAD；本 worktree 不自行运行 Cargo 或测试 binary。当前先提交未运行 Rust 的聚焦 patch，由未实施者独审、唯一 dev writer 净接后统一运行七套件，不能把静态检查称为实际测试通过。
- 五个精确 case 名同本记录前文；普通验证须遵守每命令与 case 的 10000ms 外部进程树期限，独立 case 可并行。
- 定向绿证据与未实施者完整 diff 独立复核：待补，不以本记录宣称完成。
