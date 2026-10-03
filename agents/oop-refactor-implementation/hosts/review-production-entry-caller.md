# production_entry_performance caller 独立复核

- reviewer：未实施本次改动的独立 subagent；日期：2026-10-03。
- baseline：`b89afb3346743a4b4fccf26c9ac9ff108595f696`。
- 范围：完整读取 `packages/engine/examples/production_entry_performance.rs`，审阅相对 baseline 的完整两行 diff；核对 `packages/engine/src/plans/state.rs` 的 TradingPlan 字段含义与三个 getter 实现。沿用本 reviewer 已读的 AGENTS、工程原则、交易规则与相关 ADR。
- 文件 SHA-256：`003bf62d67b6ccff1ead8e528ca943afb5048f2d14529c8e9e69eab932370a85`。

## 三项门禁

1. **大 A 语义：通过本批静态复核。** `account()` 仍返回同一 AccountId；`active_child_order_id()` 仍返回同一 Option<OrderId>；`filled_qty()` 仍返回以股为单位的累计真实成交 u32。`state.rs:175` 将 filled_qty 明确限定为真实成交进度；子单引用与成交数量仍分别检查。`production_entry_performance.rs:197` 仍对指定计划账户判断“存在 active child 或有真实成交”，计数是满足条件的计划个数，不把计划受理、子单存在解释为成交股数。没有改变价格、现金、费用、T+1、交易日或计划有效期。领域依据沿用 docs/trading-rules.md 的现行沪深规则来源及已记核对日期，本次仅等价读 API 迁移，未引入新制度解释，也未重新访问官方网页。
2. **最小必要范围：通过。** 全文件文本比较证明，仅将 plan.account / plan.active_child_order_id / plan.filled_qty 三个字段访问改为现有只读 getter；归一化去掉这三处调用括号后与 baseline 全文字节内容一致。迁移为 TradingPlan 字段私有化后的必要 caller 修复，没有新增业务、依赖或辅助抽象。
3. **边界、跨层漂移与复杂度：通过本批静态复核。** 三个 getter 均直接返回原字段的 Copy 值，没有默认值、隐式校验、计数递增或 mutation。原 OR 条件、账户集合 contains 判定与 plan_children_or_fills 计数完全保留。未发现新增遗漏边界、跨层语义漂移或不必要复杂度。

## 不变项与验证边界

- performance JSON 的 schema、所有字段名、单位、字符串格式及输出路径全部未改。
- OrderAccepted 与 IntentRejected 的事件分流、各来源 counters、三来源活动校验与其错误输出未改；原受理计数没有因 getter 迁移变为成交计数。
- step_wall_ns、run_wall_ns、phase_wall_ns 的计时起止范围、checked_add 与报错未改；计划汇总与最终 save 仍位于原计时范围之外。
- seed、RNG 行为、初始化与 restore、玩家挂单价格与 100 股数量、自然日推进、Rayon pool 配置与 thread 记录未改。
- error propagation、退出码 2、参数及 workload validation 均未改。
- 只执行短只读文本比较；没有运行性能测试、Cargo、编译或完整回归，不声称运行门禁或吞吐验收通过。
- 本范围无有效发现需要修复；结论只覆盖该 example 的三处 caller 迁移，不代替 TradingPlan 生产实现或其他文件的独立验收。
