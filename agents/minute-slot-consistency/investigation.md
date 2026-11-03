# 分钟K“形成中”观察分钟单位不一致调查与修复

日期：2026-11-03
基线：`main` / `6eaad2c7`
分支：`codex/fix-minute-slot`

## 结论

`TradingTimeline.currentMinuteSlot` 返回的是盘中分钟桶索引，默认局为 0..239；
`MinuteKlinePanel` 的 `currentMinute` 却按 engine `MinuteBarFact.minute_of_day`
解释为墙钟分钟 570..690 / 780..900。两者单位不同，导致：

- 观察时刻把 `570` 的桶格式化为 `00:00`；
- `activeMinuteKlineBucketMinute(0..239)` 落在连续竞价窗口外，形成中柱永远不标记。

## 真实链路核实

1. **producer（engine）**：`packages/engine/src/runtime/protocol.rs`
   `continuous_minute_of_day` 把连续竞价 tick 映射到墙钟分钟，
   `idx < 120` 映射 `570 + idx`，否则映射 `780 + (idx - 120)`；
   `MinuteBarFact.minute_of_day` 明确为墙钟分钟。
   `packages/engine/src/session.rs` 的
   `current_minute_history_phase_follows_trading_day_shape` 断言首笔成交
   bar 的 `minute_of_day == 570`。
2. **Host**：`apps/web/src/host/current-minute-history.ts` 与
   `market-history.ts` 原样保留 `minute_of_day`，并要求连续竞价分钟落在
   570..=690 / 780..=900；Host 没有把 0..239 桶索引转换成墙钟分钟。
3. **UI 旧实现**：`LocalRefreshViews` 把 `currentMinuteSlot(tick)` 直接传给
   `MinuteKlinePanel`。该值经 `minuteSlot`（展示槽索引）产生，与 producer
   的 `minute_of_day` 不同单位。

## 修复

- `TradingTimeline` 用 `currentMinuteOfDay(tick)` 替换 `currentMinuteSlot`，
  直接复刻 engine `continuous_minute_of_day` 的整数比例映射；开盘窗口、
  收盘竞价窗口、日界和 tick 0 返回 `null`。
- `MinuteKlinePanel` 的 prop 改名为 `currentMinuteOfDay`，明确与
  `MinuteBarFact.minute_of_day` 同口径；桌面与移动两个调用点同步改名。
- `docs/trading-rules.md` 登记展示单位约束与现行交易时段依据。

## TDD 与验证

红测：

- `trading-timeline.test.ts` 因 `currentMinuteOfDay` 不存在失败；
- `minute-kline-panel.test.ts` 在传入墙钟分钟 570 与 570 分钟 bar 后，
  未出现“观察时刻 09:30”，证明旧单元不匹配。

绿测（worktree 内执行，整条命令 10 秒 deadline）：

```text
node --test --test-timeout=10000 --test-concurrency=1 --test-isolation=none --test-force-exit \
  src/components/trading-timeline.test.ts src/components/minute-kline-panel.test.ts
# 13 pass, 0 fail, duration_ms 638.349784
```

类型与 lint：

```text
./node_modules/.bin/tsc -b --pretty false   # exit 0
./node_modules/.bin/oxlint --deny-warnings <changed files>  # exit 0
```

## A 股依据

展示时段沿用 ADR-0011 于 2026-09-09 复核的上交所《交易规则（2026年修订）》
2.4.2 与深交所同规则 2.3.2（2026-07-06 起施行）：09:30–11:30、13:00–14:57
为连续竞价，14:57–15:00 为收盘集合竞价。本批只修正分钟K展示单位，
不改变交易时段、撮合、存档或 wire 契约。
