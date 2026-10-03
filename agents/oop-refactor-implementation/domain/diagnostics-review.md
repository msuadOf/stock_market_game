# diagnostics 独立复核

复核者 canonical 身份：`/root/implement_domain/review_diagnostics`，未实施本簇源码。基线：
`b89afb3346743a4b4fccf26c9ac9ff108595f696`。动作依据为
[challenge action-index](../../oop-refactor-audit/challenge-2026-10-03/action-index.md)
中的 domain-R2-N30/N31/N32/N39/N41。

状态：六个源码文件的静态独立复核已完成，未发现该范围内的有效回归。N32 两个可选
microstructure owner 补齐后，已重读最终完整 baseline diff 与实施记录；跨组 N41 caller
缺口已修复并只读复查闭合，产品运行验证仍待 root 安排。本记录不代表整批实施验收完成。

## 范围与方法

已读 AGENTS.md、docs/principles.md、docs/testing.md、docs/architecture.md、
docs/open-questions.md、ADR-0002/0011/0019 及 ADR-0017 的相关阶段、收据与事件顺序契约，
并核对 docs/trading-rules.md 的现行基线和官方来源日期。

逐文件比较 diagnostics.rs、diagnostics/causal.rs、causal/aggregate.rs、
causal/microstructure.rs、verification_evidence/phase_timing.rs、phase_timing_tests.rs
完整 diff；读取相关 DTO、未改统计算法、既有诊断与阶段计时测试。
跨组 Session 文件仅只读检查 causal.rs、execution/records.rs、decision_chain.rs 的
plan-root 接口、pipeline/auction_day_end.rs 的 Fill receipt 接口，以及 authoritative_tick.rs
的阶段采集钩子。不据这些片段替代 Session 组完整 diff 复核。

没有运行 Cargo、产品测试或 Git 写操作；本记录不声称构建、测试或 TDD 红绿运行通过。

## 最终版本绑定

2026-10-03 再次读取最终文件并计算 SHA256；下列六文件与已完成静态复核的版本一致。
microstructure.rs 的指纹包含 DirectionPersistenceAccumulator、QuoteResponseAccumulator
两项可选 owner 及对应短保护测试，不是补齐前的版本。文件内容发生变化时，本结论不能
直接沿用，须按新 diff 复核。

| 完整 diff 复核文件 | SHA256 |
| --- | --- |
| `packages/engine/src/diagnostics.rs` | `198cda3f590efe19039e654dc20bc5f4a2e6661ada4463d43f505406d1eaf124` |
| `packages/engine/src/diagnostics/causal.rs` | `02ecc41e89395271b1bec3976fda2208f51368bd698cbfb5486838c90b61597b` |
| `packages/engine/src/diagnostics/causal/aggregate.rs` | `51ed48325c76650a70cd7a5e67f87169a218ef618c3293460e2f4e564b387788` |
| `packages/engine/src/diagnostics/causal/microstructure.rs` | `9f42ebb516634f1b484732618a0edc8e5f60d877c5d618841c9824fa02f4ccfe` |
| `packages/engine/src/verification_evidence/phase_timing.rs` | `77591e0791ec04eed5665db400d3e922c9f32f59ef53d67ccb534997286d7c52` |
| `packages/engine/src/verification_evidence/phase_timing_tests.rs` | `856331f70fd9209da49ec6dcddf7ae737200ef2b8aef087b28579f4a9902ac8a` |

N41 的以下 caller 指纹绑定本记录所述接线闭合版本；只审查相关接线片段，不据此批准
它们的全文件或其他动作：

| 接线片段所在文件 | SHA256 |
| --- | --- |
| `packages/engine/src/session/causal.rs` | `c2bf7eedfa4d3a97a7f56aa83e16cb0e0613005e5b9284ef07a33e0662520cfb` |
| `packages/engine/src/session/execution/records.rs` | `2006786b472c23e7d904e560c489f4962cd7d4c974d2761b75fe5ed4149301f7` |
| `packages/engine/src/session/decision_chain.rs` | `a24654a76aff7302916d9f36e41f01e7c7b0d29b45abc6604555c0ed104e6fb9` |
| `packages/engine/src/session/pipeline/auction_day_end.rs` | `306eb1faec499f99a1fab984eb2058951c17f3eb344cc8a3bcc44f1652fe7d21` |

## 三门核对

1. **A 股语义与依据：** 本簇为非权威运行投影、事后诊断和 opt-in 证据封装。
   未改沪深交易阶段、T+1、报价规则、证券类别差异或结算；金额仍为分、qty 为股。
   Retail 目标、可执行目标、提交委托、真实成交保持分别统计；Canceled 与 Aborted 在报告中
   分列，后者只在私有逐单释放守恒字段中累计到 canceled_qty。maker/taker 双边参与量与
   单边市场量的 2 倍关系不变。market minute 与 civil seconds 分别校验，facts sequence
   没有成为交易优先级。官方依据沿用 trading-rules.md 最近核对记录，本轮未联网重新核验。
2. **必要性与最小范围：** RetailOrderLedger、SeedDiagnostics/StockRunDiagnostics、
   CausalReportBuilder、PhaseTimingLedger 与 CausalCollector receiver 均对应 action-index
   已登记 owner 和 caller，没有引入依赖、存档字段、第二份市场状态或替换统计模型。
   N39 为授权的可选动作，采样仍为 Rayon registry worker 容量，不表示 OS runnable/CPU 利用率。
   N32 的 DirectionPersistenceAccumulator 与 QuoteResponseAccumulator 各自拥有对应临时
   状态并借用完整 facts；analyze 是真实组合 caller，符合该动作可选小步的授权范围。
3. **边界、跨层与复杂度：** 核心分支错误顺序和部分写入行为逐语句保持；新增短测试
   覆盖 overfill、未知订单、PreOpen 错误前 Trade 累加、未知 taker 前 maker 累加、双边 gross
   对账、双时钟倒退、Sequence 优先于 Budget、恢复后拒绝报告、阶段 metadata 与 counter
   overflow。既有多 seed / 真实生命周期 / 成功提交证据测试保留，运行结果交由 root 统一验证。

## 已核对的关键次序

- RetailOrderLedger：Submitted 先累加 report 再 insert/重复错误；Filled 先写逐单 filled_qty，
  overfill 时不更新 report；Canceled/Aborted 先数量检查，再逐单及 report 累加；finish 消费
  owner，并按原顺序计算 open_shares、filled_share_ratio。
- SeedDiagnostics：真实 runner 仍先 step，再 retail decisions、retail events、公开 Events；
  Trade maker/taker、单边量、分金额、分阶段量的更新顺序不变；四个同键 map 合并未改变
  BTreeMap 迭代次序、continuous no-trade streak 与日界重置。finish 仍先参与量对账，再每股
  summarize_stock，最后 Retail ledger finish；多 seed runner 与 ensemble 未改。
- CausalReportBuilder：Sequence、Budget、Submitted/Fill/Termination/Execution 校验顺序、
  Fill 去重插入位置、overfill 部分写入和 unmatched 双边对账不变；生命周期 market/civil
  两种时间的赋值/错误顺序保留，报告 DTO 和 censored/absent 原文保持。
- microstructure 两个 owner：analyze 每个 index 先 directions.consume 再 quotes.consume，
  与旧单循环一致；方向按 StockCode 分链，side=None 不更新主动方向，比例运算表达式不变。
  Execution 仍从 index+1 切片寻找首个同股有效 Quote，depth loss 仍从当前 index 切片寻找
  恢复，checked 深度与 market-minute 计算、缺失/删失原因保持；两个 finish 均消费 owner。
  新方向测试固定跨股隔离与竞价无主动方向，既有新增 Quote 测试固定两种扫描起点。
- PhaseTimingLedger：ALL/rank/index/name/From 映射、wall→span→sample overflow 后写覆盖，
  缺前置阶段优先于 overflow、finish overflow 优先于 tick 边界与 records 投影均保持；
  authoritative_tick 仍在 P9 前校验、commit 成功后 mark_committed。
- CausalCollector：plan-root batch 先登记当前 facts.len()，空与非 Decision 首项的接受集合
  保持；continuous entry0→append→checked_add 成功才更新；auction checked_sub→entry0→
  chain check→累计更新→append，原错误文本保持。

## 接线发现与再次复核

**D-INT-01（已修复并复核）：plan-root caller 私有字段迁移缺口。**
初次检查 session/decision_chain.rs 的 record_plan_root_diagnostics（检查时第 784–785 行）
仍调用 `self.state.causal.facts.len()` 和 `self.state.causal.decision.insert`，而本簇
causal.rs 已将这两个字段设为 private。该 feature 下会形成 Rust 隐私检查错误；这是
跨组集成缺口，不是交易制度变化。修法：caller 在原位置取得 causal_time，直接委托
`self.state.causal.record_plan_root(time, id, diagnostics.facts)`，保持后续 decision trace
原顺序；不要通过重新公开字段绕过 owner。已通知 domain 协调者。

最终版本再次复核：Session owner 已在 decision_chain.rs 的 record_plan_root_diagnostics 取得 causal_time，委托
record_plan_root(time, id, diagnostics.facts)，后续 record_npc_decision_trace 调用顺序保持。
全 Session 搜索未再出现对 facts/decision/filled_values 的旧直接字段访问；剩余 matches
均为 facts()/decision_for() receiver。D-INT-01 静态接线复查已闭合，未据此声称 feature
编译或产品测试通过。

Session causal.rs/records.rs/auction_day_end.rs 已使用对应 receiver；auction 的
receipt.before/after 参数、调用位置及 map_err(lifecycle_invariant) 与原链一致。

## 结论与限制

六个冻结源码文件静态三门核对无有效发现；跨组发现 D-INT-01 已由 Session owner 修复并
只读复查闭合。未新增交易规则，故本轮不构成新的官方制度
核验。未运行构建、短测试、集成测试或长验收，无法据此证明类型检查成功、测试耗时满足
10 秒限制、真实线程利用率、自由调度或三宿主兼容；这些均交给 root 的统一运行验证。
