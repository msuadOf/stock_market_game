# Sol 第三轮独立复核

- 日期：2026-11-03
- 范围：增量 `435742cd..ae8c3e2d` 与累计 `6eaad2c7..ae8c3e2d`；结合前两轮完整审查。
- 结论：**通过，无必须修复项。第二轮证据表述问题已修正。**

## 证据诚实性

`investigation.md` 现已分别说明：时间轴红测只证明新接口缺失；面板红测只证明新 prop 尚未接线。记录明确基线面板若经旧 prop 接收 570 本可正常显示，不再把新 prop 被忽略产生的失败解释成单位错误。也明确不存在独立于接口变化的原始行为红测试的既有证据，不再过度解释 TDD 历史。

新加 `minuteSlot(901) === 0` 与 `currentMinuteOfDay(901) === 570` 断言，准确固定旧展示槽与 producer 墙钟分钟的差异。结合基线调用点直接传入 `currentMinuteSlot`、旧方法委托 `minuteSlot` 的代码事实，以及当前面板墙钟 570 对应 09:30/active 的绿测，足以作为追溯解释和回归契约。记录没有将新补断言冒充原始失败测试。

## 日界、A 股语义与范围

上一轮通过的无收盘窗末 tick 断言保持不变：15:00 是交投时钟端点，899 是该 tick 覆盖区间首分钟的分钟K键，下一拍因 fixture 含开盘窗返回 null。不存在 899 与 15:00 混淆；没有把跨日 null 泛化到无开盘窗的配置。

累计产品改动仍为 producer 同口径分钟映射、两个调用点和 prop 命名；engine、wire、存档未改。A 股依据及既有 240 桶游戏简化的判断沿用首轮结论，增量没有引入制度变更。新增断言加强约束，没有弱化既有断言或扩大实现范围。

## 独立验证

显式 concurrency=2，整命令 deadline 10s、每 case timeout 10000ms：

```text
timeout 10s node --test --test-timeout=10000 --test-concurrency=2 --test-isolation=none --test-force-exit src/components/trading-timeline.test.ts src/components/minute-kline-panel.test.ts
14 pass，0 fail，duration_ms 666.181293
```

`git diff --check 6eaad2c7..ae8c3e2d` 通过。未运行完整 Web/Rust 验收；本轮未修改产品代码。
