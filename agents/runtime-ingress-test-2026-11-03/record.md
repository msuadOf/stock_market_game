# runtime ingress 严格一次生产入口测试（2026-11-03）

## 起因

审计指出：原有的 exactly-once 测试
`session::shared_ingress::tests::real_step_chain_delivers_each_external_input_exactly_once_across_forks`
只走旧管线契约入口 `step_pipeline_contract`；原生生产入口 `step_via_runtime`
缺少多输入/多拍不丢不重、以及失败后回灌的直接测试。

## 核实的契约

- ADR-0032：每会话共享 ingress receiver 记录真实接收事实与局部 ordinal；
  cutoff 前入队属于本拍，cutoff 后属于后续拍。
- ADR-0043 第 6 条：失败 tick 的原子回滚与单点裁决。首错封存与 ingress
  terminal 的实现在 `session/failure.rs::poison_failed_step`；ADR-0043
  第 9 条只定义 panic 运输，不作为首错封存依据。
- 代码契约（`session/failure.rs`）：`with_routed_runtime_driver_ref`
  先 `freeze_shared_ingress`，再 `take_runtime_player_intents` →
  `inject_player_intents`；body 失败时 `restore_runtime_player_intents`
  把同一批 `ReceiptBearingIntent`（含原 account/stock ordinal）回灌到
  `pending_player` 队首；同时 `poison_failed_step` 只设置
  `GameSession.poison` 并向 ingress 发布 terminal 并关闭它，
  **不置死 `World.is_terminal`**。注入失败点位于 World 该拍已提交之后
  （`runtime_progress.rs` 的 candidate 准备校验），因此事实是：World 该拍
  已提交，Session 与 ingress 封存首错、禁止经 Session 重放。

## 新增测试（`packages/engine/src/session/shared_ingress.rs`）

1. `runtime_step_delivers_each_external_input_exactly_once_across_ticks`
   - 3 拍 × 每拍 2 笔外部输入，逐拍调用原生 `step_via_runtime`；
   - 每笔输入的 (股票, 限价) 在全测试内唯一：当拍受理多重集必须与当拍
     入队多重集逐笔相等（丢失、重复、跨拍串位都会破坏多重集，不被总数
     抵消），订单 id 全局不重复，无 `IntentRejected`，`pending_player`
     与存档投影均无滞留。
2. `runtime_step_failure_preserves_pending_player_and_poisons_ingress`
   - 第 1 拍成功后入队 1 笔，第 2 拍注入安装失败（冻结与路由已完成）；
   - 断言失败后会话 `pending_player` 保全 1 条，账户/目标股/价格/数量
     不变，且 `account_ordinal` 与 `stock_ordinal` 均为原值 1（回执不重算）；
   - 断言失败封存首错：再次 `step_via_runtime` 返回同一首错、再次入队返回
     `SessionError::Step(首错)`；World 该拍已提交但不置 `is_terminal`，
     本用例只断言 Session/ingress 面，不构造伪造的重试成功。

## 红绿证据

红（临时删除 `inject_player_intents` 与失败回灌，仅用于验证测试判别力，
已还原，`git status` 只余测试文件改动）：

- exactly-once 测试：`assertion left == right failed: 每笔外部输入恰好受理一次（不丢不重）`
- 回灌测试：`assertion left == right failed: 失败拍不得消费外部输入（回灌 pending）`

红（独立复核后加强版：临时把路由输入复制一份，制造“重复受理”形态，
实现已还原）：

```
assertion `left == right` failed: 第 0 拍受理多重集必须与当拍入队多重集逐笔相等
  left: [(StockId(0), Money(990)), (StockId(0), Money(990)), (StockId(1), Money(1000)), (StockId(1), Money(1000))]
 right: [(StockId(0), Money(990)), (StockId(1), Money(1000))]
```

该变异下失败回灌用例仍通过（它针对回灌，不针对重复路由），符合预期。

绿（main 基线 + 本次测试）：

- `cargo test -p engine --lib --no-run`（-j 64/96，多核）：每轮均由
  进程外 `timeout ... 300` 监督；本轮实测 56.2s、71.6s、87.9s、101.1s、105.6s，
  并发争用下有 242.0s，均在 300s 硬上限内。
- `session::shared_ingress::tests::`：14 passed / 0 failed（wall 0.80s，
  进程外 10s 监督）。
- `session::failure_tests::`：22 passed / 0 failed（wall 0.70s，
  进程外 10s 监督）。
- 两个新 case 单跑均 0.19s。
- `cargo clippy -p engine --lib --tests`：仅 main 既有 dead_code 告警，
  本文件无告警；`rustfmt --check`、`git diff --check` 通过。

## 独立复核

由未参与实现的 subagent 只读复核完整 diff，结论：

- 大 A 语义：无漂移（只加测试，未改受理/撮合/回执实现）。
- 阻断发现：初版 exactly-once 用例每拍两笔输入完全相同、只比对总数，
  “一笔丢失 + 另一笔重复但订单 id 不同”仍可通过；已按建议改为每笔
  (股票, 限价) 唯一并逐拍断言受理多重集。
- 建议：失败用例补 `stock_ordinal` 断言（已补）；“失败拍未消费输入”
  表述过强（World 该拍已提交，已改为“会话 pending 保全 + 封存首错禁止
  经 Session 重放”）；record 行尾空格（已清除）。
- 复核确认 poison 后重试/入队返回同一首错，未伪造可重试。

## 结论

未发现生产缺陷：`step_via_runtime` 的多输入/多拍严格一次与失败回灌
已按上述契约工作；本次补的是生产入口的回归锚点，无实现改动。

## 第三轮修订（Sol 指出文案错误）

Sol 指出“World 已 poison”不成立：`poison_failed_step` 只设置
`GameSession.poison` + ingress terminal，`World.is_terminal` 未置死。
本轮把测试注释与工作记录统一改为：World 该拍已提交；Session 与 ingress
封存首错、禁止经 Session 重放。同时把“ADR-0043 第 9 条”从首错封存依据中
移除（该条只定义 panic 运输），依据改指 `session/failure.rs` 的
`poison_failed_step`。仅文案修订，断言与实现不变。
