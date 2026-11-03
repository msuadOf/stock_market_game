# Sol 第二轮独立复核

- 日期：2026-11-03
- 范围：`git diff 7236bd35..435742cd` 与累计 `git diff 6eaad2c7..435742cd`；前轮已审查的产品改动未进一步扩大。
- 结论：**有必须修复项（TDD 证据表述），产品映射未发现必须修复项。**

## 必须修复项

### P2：面板新 prop 被旧组件忽略，不能据此直接证明旧消费单位错误

位置：`agents/minute-slot-consistency/investigation.md:51`–53。

记录已正确说明时间轴测试的 `TypeError` 仅为接口红，但面板“行为红”仍被过度解释。基线 `MinuteKlinePanel` 解构的是 `currentMinute`，新增测试传入的是 `currentMinuteOfDay: 570`；旧组件会忽略该新 prop，并使用 `currentMinute = null`。因此没有 09:30 和 active 标记也可完全由 prop 接口未适配解释，并不能“直接证明旧的消费单位不匹配”。事实上，基线组件若通过旧 prop `currentMinute: 570` 收到该墙钟分钟，会正常显示 09:30 并标记 active；单位错误发生在旧时间轴返回桶索引并经调用点传入的链路。

建议如实将这次面板红测称为新 prop 尚未接线的渲染失败，保留其绿测作为展示契约。另补在旧接口链路上可复现的单位错误证据，例如基线 `currentMinute: timeline.currentMinuteSlot(901)` 得到 0，实际显示 00:00、无法匹配 570 bar；区分新补的追溯复现与原始 TDD 历史，不应改写历史为先前已经执行。也可仅修正记录，不宣称已有独立于接口变化的行为红证据。

## 日界与分钟键

此点已修正。新增测试明确验证含 900 个开盘 tick、无收盘窗的局：`tradeTime(15300) === "15:00:00"`，而 `currentMinuteOfDay(15300) === 899`，下一拍 `15301` 回到开盘窗返回 null。前者是成交交投时钟端点，后者是 producer 采用 tick 覆盖区间首分钟计算的 14:59 桶键，二者没有混淆。新增注释取消无条件“日界返回 null”；公式没有改变。

“跨日才没有当前分钟”测试标题须结合该 fixture 含开盘窗理解；无开盘且无收盘窗时下一日首拍仍会返回 570，现有实现是正确的，没有将 null 泛化到所有 timing。

## A 股语义、必要性与验证

无新增交易制度改变，继续对齐已有 producer 游戏时间轴及 ADR-0011 已登记简化。增量仅补证据说明、注释和边界断言，范围合理；没有弱化或删除断言。

独立执行（整命令 deadline 10s、case timeout 10000ms、concurrency=2）：

```text
timeout 10s node --test --test-timeout=10000 --test-concurrency=2 --test-isolation=none --test-force-exit src/components/trading-timeline.test.ts src/components/minute-kline-panel.test.ts
14 pass，0 fail，duration_ms 656.56368
```

`git diff --check 6eaad2c7..435742cd` 通过。未运行完整 Web/Rust 验收。本轮必须修复项只影响证据诚实性；修正记录后需要再次复核。
