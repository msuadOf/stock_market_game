# MarketSnap wire consumer 独立复核

## 复核范围

独立检查 `apps/web/src/host/protocol/wire-values.ts`、
`apps/web/src/save/schema/market.ts`、
`apps/web/src/save/schema/money-wire.test.ts`、相关生成类型和本轮相关 diff，并对照
Engine `MarketSnap`、`SaveMarketSnap` 及其 `CivilDate` / 除息锚字段语义。未审查其它
schema owner 的变更。

## Finding 与关闭复核

存档解析新增的 `parseSaveExReferencePrice` 使用 `civilDate`，而 `civilDate` 仅校验
`YYYY-MM-DD` 形状和公历有效性，没有校验 Engine `CivilDate` 的年份范围
1900..=2199。因此 `parseSnapshot` 会接受例如 `1800-01-01` 的 `ex_date`，而 Rust
Engine 类型无法表达该日期；同一锚点在运行协议 `parseIsoDate` 中又会被拒绝。存档和运行
协议的跨层日期契约因此不一致。

作者在 `parseSaveExReferencePrice` 中对 `civilDate` 的结果增加 1900..=2199 年检查，
错误路径指向 `.ex_date`；新增测试确认 1800 与 2200 在存档 parser 被拒绝，1900 与
2199 边界被存档及运行 parser 接受。与 `parseIsoDate` 和 Engine `CivilDate` 范围一致，
原 finding 已关闭。

## 其余核对

- 三个 MarketSnap 字段都是 exact shape 必填；两个布尔字段经过严格布尔校验，可空锚点只
  接受 JSON `null` 或严格对象，没有缺失/未知/非法值 fallback。
- 锚点对象拒绝未知/缺失字段；`reference_price` 通过规范 Money 字符串校验并要求为正；
  大于 JavaScript 安全整数的价格字符串被原样保留。`ex_date` 不被重写。
- 存档与运行协议都直接传输状态字段，不推断 pending/activity，也不改变市场规则。
- 修复后按 `node scripts/run-with-deadline.mjs 10000 -- node --test --test-timeout=10000 --test-isolation=none apps/web/src/save/schema/money-wire.test.ts`
  实跑：7 cases 通过，0 failures；边界测试包含在新增除息状态 case 中。
- 复核修复后的目标 diff 未发现新的有效 finding；未运行 Cargo 或操作 Git index。
