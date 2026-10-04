# Parent checkpoint expiry 定向定位

## 范围与结论

- 日期：2026-10-04。只定位 G39 压缩 fixture 发现的边界；没有全回归，没有默认 12 tick/day capture 验收，没有生产改动。
- 已验证的最小真实路径：1 stock `000812`，0 retail / 1 institution / 0 hot，无玩家输入，seed 4，3 ticks/day（opening 1 / continuous 1 / closing 1）。
- 在 tick 2 的 `ClosingAuction`，正常策略形成的 linked parent 仍在 live state，但其 `expires_market_minute == 240`；低层 `GameSession::restore(game.save())` 拒绝它。
- 原 session 不经 restore 继续执行第三个 tick 与 `end_civil_day_update()` 后，公共日终 `SaveSlot` 的 parent、连续与竞价订单均为空；`ProtocolSession::restore()` 成功，恢复后公共 `SaveSlot` 的 JSON 与原档全等。
- 因而本次没有证明 ADR-0025 的公共自然日日终档不可恢复；已证明低层诊断 `SaveSlot` 的特定日内 checkpoint 不可恢复。不得将该结论夸大为所有 seed、配置或公共保存路径均安全。

## 调用契约与根因

1. ADR-0025 第 3、8 条把内存检查点与公共持久档分开；公共档只来自完整自然日日结，且不能含 parent。
2. `ProtocolSession::checkpoint()` 通过 `try_clone_for_checkpoint()` 保存 live state，`rollback()` 直接归还 state，不经过 `SaveSlot` restore。因此此次失败不是正常宿主事务回滚失败。
3. G39 的 `committed.rs::restore_slot()` 则把 `authoritative_checkpoint()` 序列化为 `SaveSlot` 后交给 verification-only `restore_verification_checkpoint()`；该入口仍调用 `GameSession::restore()` 的完整校验。它和 clone checkpoint 是不同能力。
4. `GameSession::current_market_minute()` 按已完成连续交易 tick 映射 240 market minutes，而不是将 3 个 tick 平均分配为 80 minutes。opening 完成时为 0，唯一 continuous tick 完成后、进入 closing 时为 240。
5. `plan_execution/routing.rs::install_plan_parent()` 将 linked parent 截止设为 `(day + 1) * 240`。`persistence.rs` 无条件拒绝 `expires_market_minute <= current_market_minute`。
6. `TradingDayEndTransition::apply()` 在真实收盘订单生命周期完成、T+1 解锁与计划 sweep 后显式 `parent_orders.clear()`；公共日终候选是在成功自然日日结后生成的。

## 已保留的精确证据

`parent-checkpoint-expiry-probe.rs` 不修改生产文件，断言不仅检查错误文本，也检查真实母单数量、阶段、expiry 和未完成目标，并继续验证公共日终恢复。

实测 parent：

```text
tick=2 phase=ClosingAuction
code=000812 side=Buy target_qty=8914500 filled_qty=0 child_qty=500
active_child_order_id=1 linked_plan_id=0 limit_price=885 cents
expires_market_minute=240
```

测试以现有 `.tmp/gap-target/debug/deps/libengine-52de4fe9ae8174ac.rlib` 和对应 `serde_json` 库独立 `rustc --test` 链接运行，没有声称重新编译当前全部 production diff。库及测试二进制 SHA256 记录于 `.tmp/parent-checkpoint-expiry-sha256.log`；编译与运行日志分别为 `.tmp/parent-checkpoint-expiry-build.log`、`.tmp/parent-checkpoint-expiry-run.log`。这些 `.tmp` 证据为本地非提交产物。

- 编译：`runBoundedCommand` 300000ms supervisor，`-C codegen-units=16`；不是普通测试命令。
- 最终短测：`node scripts/run-with-deadline.mjs 10000 -- env RAYON_NUM_THREADS=4 .tmp/parent-checkpoint-expiry-probe --test-threads=4 --nocapture`。
- 结果：1 passed，0 failed，1.80s。单个同步 Rust case 由外部进程树 10000ms deadline 硬约束；Rayon 4 workers，test threads 4。
- fixture 定位先试 seed 17，再将 seed 1–8 以 4 独立进程、每进程 Rayon 2 workers、共享 10000ms supervisor 分批并行探测；除 seed 4 外均在“没有 parent”的前置条件失败，未弱化这些断言。最终固定 seed 4，不把探索失败计作通过。

## 修复必要性与建议

- 本次公共日终保存不需要据此修改；不建议因诊断 fixture 换成 retail-only 就声称 institutional checkpoint 恢复已覆盖。
- 原 G39 3 tick fixture 在第 2 tick 取“intraday quiet point”，实际已经进入 closing expiry 边界。默认 12 tick fixture 的第 2 tick 尚在 opening；本次没有运行它，不能保证它通过。
- 若验收仅承诺可恢复的日内锚点，应显式选择尚未到期的阶段并保留独立 closing 边界记录；不能将 fixture 调整当作 production 修复。
- 若后续明确承诺任意日内 `SaveSlot` checkpoint 可恢复，则这是待修的低层契约缺口：live state 合法保留的到期 parent 与持久投影校验不一致。需由 parent/order 生命周期 owner 决定如何表示“到期但真实 child 尚待收盘生命周期终结”；不得直接删除母单、在 closing 强行撤单、伪造 fill，或删掉 restore 强校验。
- 本次没有变更 A 股交易制度；closing 不可撤与日终委托失效仍走既有权威 finalizer，ADR-0015/0025 为依据。没有引入新的交易所规则推断。

## 复核门禁

已请求未实施本记录与 probe 的 root 独立复核全部两文件 diff；本定位不包含生产修复，也不宣称任意日内 checkpoint 恢复能力完成。
