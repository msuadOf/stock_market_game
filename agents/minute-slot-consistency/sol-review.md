# Sol 独立复核记录

- 日期：2026-11-03
- 审查者：未参与实现的 Sol subagent
- 范围：完整 `git diff 6eaad2c7..7236bd35`，共 7 个文件；未修改产品代码。
- 结论：**通过，无必须修复项。**

## Producer → Host → UI

核查 `packages/engine/src/runtime/protocol.rs:253`、`runtime/desk.rs:2074` 与 `session/pipeline/serial_b2_surfaces.rs:95`：真实调用方以当前 tick−1 为撮合基准，按日取模；连续窗内采用 `floor(cont_tick * 240 / continuous)`，前 120 桶加 570，后 120 桶加 780−120。新 `TradingTimeline.currentMinuteOfDay` 使用相同基准、BigInt 整数除法和映射，没有误套 UI clock 的 237 分钟比例。短局、无开盘窗、无收盘窗与默认局都保持相同公式。

开盘窗和收盘竞价窗返回 null；tick 0 显式返回 null。默认局日末处于收盘竞价，返回 null；下一日开盘窗同样返回 null。无收盘竞价的简化局末 tick 仍是连续窗最后一拍，按 producer 映射返回分钟，并在下一拍按新交易日重新映射；这符合成交基准语义。方法注释中的“日界返回 null”应理解为默认有收盘竞价的配置，并非额外改变无收盘窗契约。

`normalizeMinuteBar` 原样保留墙钟 `minute_of_day`；`normalizeCurrentMinuteHistoryResponse` 复用它且不变换单位。桌面和移动两个调用点均改用新方法及新 prop。`MinuteKlinePanel` 按同一墙钟分钟格式化观察时刻、调用 `activeMinuteKlineBucketMinute` 并匹配真实聚合柱，链路一致。未发现残留 `currentMinuteSlot` 或旧 prop 调用。

## A 股语义与范围

本批修复展示单位，不改变 engine 交易制度、撮合、wire 或存档。ADR-0011 明确 240 桶是游戏盘中时间轴，不是现实 A 股的 240 分钟连续竞价。故 producer 在游戏连续阶段映射到 14:57 之后的现状属于既有已登记简化，本批精确对齐该事实源是合理的。

依据为 ADR-0011 引用的上交所《交易规则（2026年修订）》2.4.2 与深交所同规则 2.3.2，复核日期 2026-09-09。独立访问上交所官方发布页，确认标题及“2026年7月6日起施行”；规则时段及深交所条文沿用仓库已有官方引用，本次未另行下载两所完整附件逐条复核。新文档没有将游戏 240 桶声称为真实连续竞价制度。

改动集中在单位命名、单个映射方法、两个调用点、回归测试与对应文档，没有引入依赖或扩大业务范围。

## 测试与验证

逐项核对完整测试 diff：既有开盘窗口断言改为墙钟分钟是契约纠错；原本将收盘竞价视为形成中分钟的断言被明确的连续末拍与收盘首拍断言替代，没有为通过测试弱化真实要求。新增面板测试同时检查 09:30 展示、真实柱“形成中”及禁止 00:00。

独立运行（显式 concurrency=2；整命令 timeout 10s、每 case 10000ms）：

```text
timeout 10s node --test --test-timeout=10000 --test-concurrency=2 --test-isolation=none --test-force-exit src/components/trading-timeline.test.ts src/components/minute-kline-panel.test.ts
13 pass，0 fail，duration_ms 754.593069
```

`git diff --check 6eaad2c7..7236bd35` 无错误。未运行完整 Web 或 Rust 验收；本批未修改 engine。

可选后续增强：将无收盘窗日末/跨日、默认局连续首拍前一拍和收盘首拍做成表驱动断言，或建立与 producer 共用 fixtures 的契约测试，以防未来两份公式漂移。目前相同公式已可由代码直接确认，现有短局、默认局与面板回归足以支持本批通过。
