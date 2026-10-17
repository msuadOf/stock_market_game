# step_frame 接轨实现方案（第 16 任，2026-10-18）

目标：step_frame 生产入口（server :1165/:1636、desktop :847/:1176）走
std-thread runtime；TickFrame 投影/validate/失败回滚在 runtime 路径等价
承载；两路径对照全等；双模式回归绿。

## 现状缺口清单（调研实证）

1. **Continue 早轮成交同拍投影双计**（prerequisites ①，阻断级）：
   world.run_ticks 把 Continue 链全轮 desk_trades 合并进 TickRecord；
   组账户已随 Continue 落账早轮实体 → drive 的
   `apply_same_tick_fill_projection`（现金/持仓/T+1/成本）与
   `apply_same_tick_plan_fills*`（执行腿）以全量为输入做"组回写后叠加"
   = 早轮双计。生产 continuation 恒 false 故暂不可达。
2. **seq 起点 off-by-one**：旧管线 collect_events 第一条 = state.seq+1；
   driver `adopt_session_seq` 后第一条 = state.seq。TickFrame 契约
   `(seq_from, seq_to]` 下 runtime 首条 ≤ seq_from 必然违约（组 frame
   即显红）。
3. **PriceTick 事件缺失**：旧管线每连续拍每股产 PriceTick（客户端
   timeseries 依赖，server 集成测试锚"PriceTick 每 step 一定出"）；
   runtime 事件流无（bridge_tests.rs:362 自认）。
4. **step_frame 无 runtime 分叉**：commit.rs:62/:75/:90 全走旧管线
   `execute_authoritative_tick_with_projection`。

## 件 1（前置约束①先行）：Continue 早轮成交恰一次落账

- 修法：`TickRecord` 增 `unsettled_trades`（终轮 evidence 原始
  desk_trades——从未随 Continue 下行落账的尾巴；rounds=0 时=全量）。
  drive 的两个同拍投影改消费 unsettled；事件流/buy_vwap（入场参考价）/
  个人记忆裁剪/dirty 标记保持全量（非叠加面，幂等或纯读）。
- 红证：GroupIn 增 `ForceContinuationForTest`（cfg(test) 一次性通道，
  仿 ForceDue）+ driver 级端到端（对手单早轮成交 → Continue → 断言
  session 公开现金/持仓恰一次入账）+ world 级尾巴分离锚。

## 件 2：seq 分配起点对齐

- 修法：`adopt_session_seq` 改 `next_seq = current_seq + 1`（driver
  take 语义先取后加不变；DayBoundary 段手动 +1 分配同式已一致）。
- 红证：driver 单测断言首条事件 seq = 步前 seq + 1；件 4 parity 兜底。

## 件 3：runtime 事件流补 PriceTick

- 修法：`runtime_step_with_install` 在 install_candles 之后追加（数据源
  view.last_price/depth_bids/asks + session.candle_book.active；seq 经
  set_runtime_seq(+1) 逐条分配；仅 session.phase()==Continuous 每股一条；
  竞价相位已由 desk_auction facts 承载 AuctionTick/AuctionCompleted；
  日结拍相位为 ClosingAuction 不产，与旧管线一致）。
- 红证：断言连续拍事件流含每股恰一条 PriceTick。

## 件 4：step_frame runtime 分叉（核心）

- `GameSession::step_frame_and_commit` 分叉：capture_commit_evidence=
  true → 恒旧管线（Escrow verifier 面，D4 登记语义保持——TickCommitEvidence
  的信封域证据 runtime 无等价承载，批 9 退役时再裁决）；否则
  step_runtime_enabled() → `step_frame_and_commit_via_runtime`。
- 流程：提取 step_via_runtime 的 driver 获取/锁步校验/ingress 冻结/玩家
  意图路由段为公共方法 → seq_from=步前 seq → runtime_step_with_install
  （重构暴露 baseline）→ frame 投影段：`event_facts_with_stable_keys(
  events)`（event_bridge 批 8h·部分2 已有）→ `TickCandidateProjection::
  new(&self, seq_from)` → project 回调 → 失败 = 回滚 + poison。
- ProtocolSession 层零改动：回调内 project_frame_and_runtime 经
  runtime_snapshot()/player_working_orders() 自动读 runtime 读面
  （desk_book_mirror 承载，snapshot_inner 已 runtime-aware）。
- 回滚面扩展（malformed_frame 契约：失败后 save==步前）：现状
  baseline+五域 undo 缺 desk_book_mirror / runtime_desk_serial_cursors /
  plans（set_runtime_plans）/ envelope_ledger / npc_order_lifecycles /
  last_retail_order_events / last_retail_decisions。补写时 move-undo
  （整值替换域零拷贝搬旧值）；envelope 增量分支记录逆操作、全量分支
  快照旧值。

## 件 5：验收

- lib 内 C 类对照：同 fixture 同 seed 两路径（旧 =
  execute_authoritative_tick_with_projection 直调；新 = 分叉入口），
  断言 frame.tick/seq_from/seq_to/事件集合/timeseries 结构/账户终态全等。
- tests/tick_frame_commit.rs 双模式（现有 C 类在默认=runtime 下即测
  runtime 契约；ENGINE_STEP_RUNTIME=0 测旧管线契约）。
- server/desktop 集成默认模式（tests/actor.rs、desktop protocol_tests.rs
  自动覆盖四调用位）。
- 回归门槛：engine lib 双模式全绿（基线 1642/0/11）+ 集成 93 绿 +
  sol-103 家族锚不破。

## 边界坑对照（交接 §5）

- shadow/commit 交接面：本批不改 CommittableSessionState 字段集
  （TickRecord 属 world 域；undo 槽入 state 需同步三处——clone 构造/
  两个 destructure/commit 赋值）。
- 序号游标链/镜像瞬态/组端回执链/市价语义：不动。
- 红证纪律：完整原始输出（含 panic 栈+left/right）落
  .tmp/sol-stepframe-red-evidence/。
