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
  收盘竞价窗口和 tick 0 返回 `null`。默认含收盘竞价窗口的局，次一自然日
  开盘 tick 返回 `null`；无收盘窗口时，当日末 tick 仍是连续窗最后一拍
  （`tradeTime` 为 15:00），返回该拍覆盖区间的首分钟，不能把“日界返回
  null”无差别套用到所有 timing。
- `MinuteKlinePanel` 的 prop 改名为 `currentMinuteOfDay`，明确与
  `MinuteBarFact.minute_of_day` 同口径；桌面与移动两个调用点同步改名。
- `docs/trading-rules.md` 登记展示单位约束与现行交易时段依据。

## TDD 与验证

红测（区分接口红与接线失败，不把接口变化冒充行为证据）：

- **接口红**：`trading-timeline.test.ts` 引用尚不存在的
  `currentMinuteOfDay`，失败信息是 `TypeError: ... is not a function`。
  这只证明新接口尚未实现，不能证明旧单位错误。
- **接线失败红**：`minute-kline-panel.test.ts` 把新 prop
  `currentMinuteOfDay` 传给基线组件；基线组件解构的是旧 prop
  `currentMinute`，会忽略新 prop 并以 `null` 渲染，所以没有“观察时刻
  09:30”和形成中标记。这只证明新 prop 尚未接线，不能直接证明单位错误；
  基线面板若通过旧 prop 收到墙钟分钟 570，本可正常显示。
- **单位错误的追溯证据**：旧调用链是
  `timeline.currentMinuteSlot(tick)` → `MinuteKlinePanel.currentMinute`；
  对默认局 tick 901，旧展示槽索引为 0，而 engine 墙钟分钟为 570。
  实现后的回归断言同时固定 `minuteSlot(901) === 0` 与
  `currentMinuteOfDay(901) === 570`，并用面板测试固定墙钟 570 显示
  09:30 且标记 active 桶。这是调用链追溯和契约断言，不宣称存在一个
  独立于接口变化的原始行为红测试。
- 实现后另用时间轴断言固定 570/583/781/899 等墙钟分钟；行为证据不由
  接口红或未接线的渲染失败替代。

绿测（worktree 内执行，整条命令 10 秒 deadline）：

```text
node --test --test-timeout=10000 --test-concurrency=1 --test-isolation=none --test-force-exit \
  src/components/trading-timeline.test.ts src/components/minute-kline-panel.test.ts
# 14 pass, 0 fail, duration_ms 664.56964
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
