# EOF 独立全文复核：session lifecycle / persistence-snapshot / plans-strategy

日期：2026-10-03。审查者：`/root/luna73`，未实施本批源码。复核工作树 `implementation-reaudit` 的 `HEAD=a7c7ce357bdc9f88c03633744b2d5815db49e9b2`；指定产品提交 `08e4fc7`（merge 同）仅改 `agents/main-release-validation/summary.md`，不含产品源码变更。只读检查正式规则、3 篇指定记录全文、对应生产 owner 与实际 consumers、已有独立复核和验证台账；本文件为唯一写入。

## 全文覆盖矩阵

| 指定文件/行 | 章节及主张 | 对照实现与结论 |
| --- | --- | --- |
| `session/lifecycle.md:1-16` | 日期、动作范围、owner/调用边界 | 覆盖母单 `ParentOrderPlan`、protocol、K 线簿、NPC attention 及跨组 caller。实际母单连续路径在 `session/execution/records.rs:5-69,71-117`；protocol owner 在 `session/protocol/civil/session.rs:9-80`；K 线 owner 在 `session/candles.rs:90-129`。文字明确把竞价、save/hash/snapshot caller 列为别组，不把本 agent 的代码所有权扩大。 |
| `session/lifecycle.md:18-25` | 跨组 API 与旧行为 | `execution.rs:180-223` 连续实际 fill 先推进再保留既有断言时点；`:225-265` checked auction 路径先校验再写入。`records.rs:34-53` 只以结算后 fill 推母单并添加 linked pending event，`:71-100` 只在路由接受后关联 child；撤单只清匹配 child (`:103-116`)。与记录一致。 |
| `session/lifecycle.md:27-29` | 金额/数量、T+1、撮合、candle 与存档语义 | 规则基线以正式 `docs/trading-rules.md`、ADR-0015/0011/0018 为准。`candles.rs:131-179` 保留零成交占位和第一笔真实成交重置 OHLC；`:181-205` 收盘归档完整历史。`snapshot.rs:215-228` 及 `session.rs:2583-2590` 分别投影完整 `daily_candles` 与 `active_daily_candles`，DTO 未合并。 |
| `session/lifecycle.md:31-54` | 测试交接、诚实边界、L1-L3 初审修复 | 与复核记录 `session/lifecycle-review.md`、`session/implementation.md` 及验证台账交叉检查：L1/L2 paired writer/getter 已修，L3 reverse rebuild fixture 已补；后续独立复核及集中短测结果已在实现总览记录。本文未执行测试。行 48、54 的“尚未/等待”是当时实施记录的时间状态，不可读作最终门禁仍未通过。 |
| `persistence-snapshot.md:1-11` | OpeningFigures、SaveValidationContext、fee audit、snapshot reservation、字段迁移 | `persistence.rs:189-285` 显示派生交易时钟先经 schema/setup 后计算，错误顺序及首次范围校验仍在主流程；`persistence/v2.rs:644-695` 的 `CumulativeFeeAuditV2` 仅验证累计边界，不反推逐笔扣费优先级。`snapshot.rs:65-112,128-228` 在单次 snapshot 临时合并连续/竞价挂单预留并按账户移出，保留 Save DTO。声明与生产调用一致。 |
| `persistence-snapshot.md:13-17` | 正式依据、费用证据限制及领域边界 | `docs/trading-rules.md:9-48,103-123` 明确游戏简化、T+1/零股、费用口径及不支持范围；ADR-0017/0025 是对应正式记录。文档明确没有重新联网核验，且过户费材料沿用原核查日期；没有把 snapshot/envelope 包装冒称真实清算。 |
| `persistence-snapshot.md:19-43` | 测试/验证边界、leaf 发现修复 | `session/leaf-review.md` 的修复后再次复核确认 poison 回 facade、TradingPlan 改 getter、ZiNoise 全参数投影补测；`session/caller-review.md` 复核追加 caller 未弱化断言。旧文中的“待统一执行/通知复核”是实施当时状态；后续复核记录覆盖这些修复。 |
| `plans-strategy.md:1-13` | TradingPlan receiver/getter、review preview、ZiNoise 参数投影、belief 风险 owner、RetailDecisionContext | `plans/state.rs:160-267` getter 与 clone preview 只改副本；receiver 转移保持 guard/赋值。`strategy/zi_noise.rs:40-52` 将 arrival/order/chase 与 dip/stop/take-profit/volume/position-step/base-observation 全量映射，`:291-327` 非默认全参数投影测试逐项守卫。`strategy/beliefs.rs:160-222` 保留 frozen policy、失败衰减、无 peak `None`、pause `>=`/resume `>`。`strategy/retail.rs:139-220` context 仅在单股选中后组织原事实并纯分类，卖出仍读 sellable qty。 |
| `plans-strategy.md:15-22` | 单位、A 股约束、参数与序列化边界 | Money 分、quantity 股、比例 bp 与 T+1/买入整手/卖出零股以 `docs/trading-rules.md` 为准。检查改动是 owner/caller 搬迁；不改费用政策、订单申报参数、serde wire 名称/精确浮点或 RNG 消费顺序。 |
| `plans-strategy.md:24-60` | 测试交接、跨文件 callers、N07 adapter、复核补测 | getter call sites 与 `decision_chain` 的 preview/risk owner caller 实际存在；ZiNoise projection test 覆盖非默认值与 `f64::to_bits`。`leaf-review.md` 后续再次复核已关闭原投影覆盖缺口。旧行 36、60 是实施时报告，不声称当时已运行或复核。 |

## 历史发现复核

- `lifecycle-review.md` 记录的 L1 paired writer、L2 late-failure getter、L3 unlinked Buy→Sell reverse rebuild：当前实现入口/测试存在；review 的再次复核与集中结果在 `session/implementation.md` 有记录。没有把 test fixture 说成真实完整撮合/账户结算。
- `leaf-review.md` 的 poison 路径、TradingPlan caller getter 两项编译阻断：`persistence/v2_tests.rs` 仍操作 facade `session.poison`；`persistence.rs::validate_plan_contract` 读取 TradingPlan/linked getter，ParentOrderPlan 和 Save DTO 仍保留各自字段语义。修复后复核记录关闭两项。
- ZiNoise 投影覆盖缺口：现有非默认实例测试验证 `arrival_rate/order_size_mean/chase_prob/dip/stop/take-profit/volume/position_step/base_observation` 及 profile；浮点按 bits 比较。旧 review 提醒的缺口已被测试补齐并在后续 leaf review 复核。
- caller 迁移旧结论：`caller-review.md` 检查 10 个追加文件，报告 51 个 test 与 190 个 assert 宏基线/现稿数量相同，并逐 diff 核实关键非法 fixture、poison/hash/save 与费用断言；此计数只作辅助证据，未替代其全文审查。

## EOF 候选、反证与结论

未发现新的有效产品语义候选。可能误报的点及反证：

1. 母单连续 fill 先改 `filled_qty`、再 assert target，看似不是原子操作；这是记录与旧语义明确要求保留的连续失败时点，auction 使用另一 checked 候选路径预校验。没有证据支持把它列为本次回归。
2. 首笔真实成交覆盖零量 candle，乍看像丢数据；被覆盖者是盘前昨收建立的零成交占位 OHLC，正式 candle 应从首笔真实成交开始。历史仍在 `AppendOnlyHistory` 完整保留，且对应边界测试存在。
3. 买卖快照 reservation 把现金和卖股数量暂存后投影，可能误读为新增冻结/清算状态；实现是单次 Snapshot 的派生值，DTO 字段既有，真实挂单与资产仍归订单/账户 owner。
4. 实施记录尾部有“待 reviewer/root”措辞；这些是有日期的当时状态，不能覆盖之后 `lifecycle-review.md`、`leaf-review.md`、`caller-review.md` 与实现总览中的已修复/已复核证据。无需改写历史工作记录。

大 A 判断：本组没有创设或修改交易制度，正式规则仍由 `docs/trading-rules.md` 和对应 ADR 承担；未重新核验官网，故不作“本日官方规则已复核”结论。范围判断：实现为 owner/caller 收敛及派生快照/验证 context，没有新依赖或新持久化事实。测试及复核执行：历史 root 台账报告集中短测、编译检查与独立复核完成；本审查者仅做静态复核，没有自行运行测试/build，也不将短测写成完整回归。
